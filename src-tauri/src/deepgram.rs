//! The live transcript, streamed from Deepgram with your own key.
//!
//! One socket per side of the call (and one for a dictation): 16-bit audio
//! up, words back within a second, final results that do not overlap, and
//! on the side that hears the others a speaker number per word, so the
//! people on the call are told apart.
//!
//! A stream writes into the shared `Transcript`: finals as settled lines,
//! interims as that side's live guess. Nothing here touches the database.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::transcript::{log, Speaker, Transcript};

const LISTEN_URL: &str = "wss://api.deepgram.com/v1/listen";

/// Deepgram's general model. `language=multi` is what handles a call that
/// mixes languages; Nova-3 is the tier that offers it for streaming.
pub const MODEL: &str = "nova-3";

/// The listen configuration for one side of a call.
///
/// Diarization only on the side that hears the others: the microphone is
/// one person by construction. `endpointing` and `utterance_end_ms` decide
/// how quickly a final arrives after a pause; 300 ms and 1 s are the
/// guidance for conversation.
pub fn query(sample_rate: f64, diarize: bool) -> String {
    format!(
        "model={MODEL}&language=multi&encoding=linear16&sample_rate={}&channels=1\
         &smart_format=true&punctuate=true&interim_results=true\
         &endpointing=300&utterance_end_ms=1000&diarize={diarize}",
        sample_rate.round() as u32
    )
}

/// Dictation: one voice, numerals as digits, a short endpoint so each
/// sentence lands as it is finished. Key terms are an English-only
/// feature, so dictation with key terms runs on `en`.
pub fn dictation_query(sample_rate: f64, keyterms: &[String]) -> String {
    let language = if keyterms.is_empty() { "multi" } else { "en" };
    let mut q = format!(
        "model={MODEL}&language={language}&encoding=linear16&sample_rate={}&channels=1\
         &smart_format=true&punctuate=true&numerals=true&filler_words=false&interim_results=true\
         &endpointing=300&utterance_end_ms=1000&diarize=false",
        sample_rate.round() as u32
    );
    for k in keyterms.iter().take(60) {
        q.push_str("&keyterm=");
        q.push_str(&urlencode(k));
    }
    q
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One open socket for one side.
pub struct LiveStream {
    /// Audio in. Dropped to say the speech is over.
    tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<f32>>>,
    failed: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
    done: Arc<AtomicBool>,
}

impl LiveStream {
    /// One side of a call.
    pub fn open(key: &str, who: Speaker, sample_rate: f64, transcript: Arc<Transcript>) -> Self {
        let diarize = who == Speaker::Others;
        Self::open_with(key, who, transcript, &query(sample_rate, diarize))
    }

    /// You, dictating into a text box.
    pub fn open_dictation(key: &str, sample_rate: f64, transcript: Arc<Transcript>, keyterms: &[String]) -> Self {
        Self::open_with(key, Speaker::Me, transcript, &dictation_query(sample_rate, keyterms))
    }

    /// Opens the socket on its own thread. A connection failure shows up
    /// as `failed()` a moment later, so the caller polls that.
    fn open_with(key: &str, who: Speaker, transcript: Arc<Transcript>, query: &str) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();
        let failed = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));
        let (failed_t, reason_t, done_t) = (failed.clone(), reason.clone(), done.clone());
        let url = format!("{LISTEN_URL}?{query}");
        let key = key.to_string();
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::rt::block_on(socket_loop(url, key, who, rx, transcript, failed_t.clone()))));
            match res {
                Ok(Ok(())) => log(&format!("live transcript closed ({})", who.label())),
                Ok(Err(e)) => {
                    log(&format!("live transcript failed ({}): {e}", who.label()));
                    *reason_t.lock().unwrap() = Some(e);
                    failed_t.store(true, Ordering::SeqCst);
                }
                Err(_) => {
                    log(&format!("live transcript panicked ({})", who.label()));
                    *reason_t.lock().unwrap() = Some("panicked".into());
                    failed_t.store(true, Ordering::SeqCst);
                }
            }
            done_t.store(true, Ordering::SeqCst);
        });
        Self { tx: Some(tx), failed, reason, done }
    }

    /// Audio at the rate the stream was opened with, mono float samples.
    pub fn push(&self, samples: &[f32]) {
        if self.failed.load(Ordering::SeqCst) {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(samples.to_vec());
        }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    pub fn failure(&self) -> Option<String> {
        self.reason.lock().unwrap().clone()
    }

    /// The speech is over: sends CloseStream and waits a few seconds for
    /// the last finals, which come after it.
    pub fn close(mut self) {
        self.tx.take();
        let started = std::time::Instant::now();
        while !self.done.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Float samples to 16-bit little-endian PCM, which is what `linear16`
/// means on the wire.
pub fn pcm16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// A refused connection as a sentence, from its status.
fn connect_error(e: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match e {
        Error::Http(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => "the Deepgram key was not accepted".into(),
        Error::Http(resp) if resp.status().as_u16() == 402 => "the Deepgram account is out of credit".into(),
        other => other.to_string(),
    }
}

async fn socket_loop(
    url: String,
    key: String,
    who: Speaker,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
    transcript: Arc<Transcript>,
    failed: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut request = url.into_client_request().map_err(|e| format!("request: {e}"))?;
    request.headers_mut().insert("Authorization", format!("Token {key}").parse().map_err(|_| "the Deepgram key has characters a request cannot carry".to_string())?);
    let (ws, _) = tokio_tungstenite::connect_async(request).await.map_err(|e| format!("connect: {}", connect_error(&e)))?;
    let (mut tx, mut rxs) = ws.split();
    log(&format!("live transcript connected ({})", who.label()));
    // Deepgram closes a socket that has sent nothing for about 10 s; a
    // call has silences longer than that.
    let mut keepalive = tokio::time::interval(Duration::from_secs(8));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut closing = false;
    let mut pending = Pending::default();
    loop {
        tokio::select! {
            audio = rx.recv(), if !closing => match audio {
                Some(samples) => {
                    if tx.send(Message::Binary(pcm16(&samples))).await.is_err() {
                        return Err("send failed".into());
                    }
                }
                None => {
                    closing = true;
                    let _ = tx.send(Message::Text(r#"{"type":"CloseStream"}"#.to_string())).await;
                }
            },
            _ = keepalive.tick() => {
                let _ = tx.send(Message::Text(r#"{"type":"KeepAlive"}"#.to_string())).await;
            }
            // After CloseStream the last finals arrive, then the server
            // closes. If it goes quiet instead, four seconds is enough.
            _ = tokio::time::sleep(Duration::from_secs(4)), if closing => {
                pending.flush(who, &transcript);
                return Ok(());
            }
            msg = rxs.next() => match msg {
                Some(Ok(Message::Text(text))) => handle_message(&text, who, &transcript, &mut pending),
                Some(Ok(Message::Close(_))) | None => {
                    pending.flush(who, &transcript);
                    return if closing { Ok(()) } else { Err("closed by the server".into()) };
                }
                Some(Err(e)) => {
                    pending.flush(who, &transcript);
                    return Err(format!("socket: {e}"));
                }
                _ => {}
            },
        }
        if failed.load(Ordering::SeqCst) {
            pending.flush(who, &transcript);
            return Err("marked failed".into());
        }
    }
}

/// Finals not yet written as lines.
///
/// A final arrives every time the endpointer hears a short pause, which in
/// real speech is mid-sentence. `speech_final` marks the end of an
/// utterance; finals are gathered until it, or until `UtteranceEnd`, and
/// only then written, one line per speaker run.
#[derive(Default)]
struct Pending {
    /// (speaker, text, words per detected language)
    segments: Vec<(Option<u32>, String, Langs)>,
}

/// Words per language tag, as the multilingual model labelled them.
type Langs = std::collections::BTreeMap<String, u32>;

/// The language most of a line's words were heard in.
pub fn main_language(langs: &Langs) -> Option<String> {
    langs.iter().max_by_key(|(_, n)| **n).map(|(l, _)| l.clone())
}

impl Pending {
    fn push(&mut self, speaker: Option<u32>, text: &str, langs: &Langs) {
        match self.segments.last_mut() {
            Some((s, t, l)) if *s == speaker => {
                t.push(' ');
                t.push_str(text);
                for (k, n) in langs {
                    *l.entry(k.clone()).or_default() += n;
                }
            }
            _ => self.segments.push((speaker, text.to_string(), langs.clone())),
        }
    }

    fn text(&self) -> String {
        self.segments.iter().map(|(_, t, _)| t.as_str()).collect::<Vec<_>>().join(" ")
    }

    fn flush(&mut self, who: Speaker, transcript: &Transcript) {
        for (speaker, text, langs) in self.segments.drain(..) {
            transcript.append_final_in(who, speaker, &text, main_language(&langs));
        }
        transcript.set_partial(who, "");
    }
}

/// A final this short and this unsure is noise, not speech: a breath or a
/// keyboard read as a word. Real words come with confidence well above.
const NOISE_CONFIDENCE: f64 = 0.55;
const NOISE_MAX_WORDS: usize = 3;

/// One message from the stream. Finals gather in `pending` until the
/// utterance ends; interims are the side's live guess.
fn handle_message(text: &str, who: Speaker, transcript: &Transcript, pending: &mut Pending) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
    match v["type"].as_str().unwrap_or("") {
        "Results" => {
            let alt = &v["channel"]["alternatives"][0];
            let spoken = alt["transcript"].as_str().unwrap_or("").trim();
            let is_final = v["is_final"].as_bool().unwrap_or(false);
            let speech_final = v["speech_final"].as_bool().unwrap_or(false);
            if !is_final {
                if !spoken.is_empty() {
                    let ahead = pending.text();
                    transcript.set_partial(who, &if ahead.is_empty() { spoken.to_string() } else { format!("{ahead} {spoken}") });
                }
                return;
            }
            if !spoken.is_empty() {
                let confidence = alt["confidence"].as_f64().unwrap_or(1.0);
                let words = spoken.split_whitespace().count();
                if !(confidence < NOISE_CONFIDENCE && words <= NOISE_MAX_WORDS) {
                    let segments = segments_with_languages(&alt["words"]);
                    if segments.is_empty() {
                        pending.push(None, spoken, &languages_of(&alt["words"]));
                    } else {
                        for (speaker, segment, langs) in segments {
                            // Numbered from 0 on the wire; people count from 1.
                            pending.push(Some(speaker + 1), &segment, &langs);
                        }
                    }
                }
            }
            if speech_final {
                pending.flush(who, transcript);
            } else {
                transcript.set_partial(who, &pending.text());
            }
        }
        "UtteranceEnd" => pending.flush(who, transcript),
        "Error" => log(&format!("live transcript error ({}): {}", who.label(), v["message"].as_str().unwrap_or(""))),
        _ => {}
    }
}

/// Words per language tag across one result.
fn languages_of(words: &serde_json::Value) -> Langs {
    let mut out = Langs::new();
    for w in words.as_array().into_iter().flatten() {
        if let Some(l) = w["language"].as_str().filter(|l| !l.is_empty()) {
            *out.entry(l.to_string()).or_default() += 1;
        }
    }
    out
}

/// The words of one result grouped by speaker, in order, with each
/// group's words per language. Empty when the result carries no speaker
/// numbers, in which case the whole transcript is one line.
fn segments_with_languages(words: &serde_json::Value) -> Vec<(u32, String, Langs)> {
    let Some(list) = words.as_array() else { return Vec::new() };
    let mut out: Vec<(u32, String, Langs)> = Vec::new();
    for w in list {
        let Some(speaker) = w["speaker"].as_u64() else { return Vec::new() };
        let word = w["punctuated_word"].as_str().or_else(|| w["word"].as_str()).unwrap_or("");
        if word.is_empty() {
            continue;
        }
        let lang = w["language"].as_str().filter(|l| !l.is_empty());
        if !matches!(out.last(), Some((s, _, _)) if *s == speaker as u32) {
            out.push((speaker as u32, String::new(), Langs::new()));
        }
        let (_, text, langs) = out.last_mut().unwrap();
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(word);
        if let Some(l) = lang {
            *langs.entry(l.to_string()).or_default() += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finals_settle_and_interims_stay_partial() {
        let t = Transcript::default();
        let mut me = Pending::default();
        let mut them = Pending::default();
        handle_message(r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"we should","words":[]}]}}"#, Speaker::Me, &t, &mut me);
        assert_eq!(t.snapshot().partial_me, "we should");
        assert!(t.snapshot().lines.is_empty());
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"we should ship Friday.","words":[]}]}}"#, Speaker::Me, &t, &mut me);
        let s = t.snapshot();
        assert_eq!(s.lines.len(), 1);
        assert_eq!(s.lines[0].text, "we should ship Friday.");
        assert_eq!(s.partial_me, "");
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"ok. sure.","words":[{"punctuated_word":"ok.","speaker":1},{"punctuated_word":"sure.","speaker":0}]}]}}"#, Speaker::Others, &t, &mut them);
        let text = t.text();
        assert!(text.contains("Speaker 2: ok."), "{text}");
        assert!(text.contains("Speaker 1: sure."), "{text}");
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"","words":[]}]}}"#, Speaker::Me, &t, &mut me);
        assert_eq!(t.snapshot().lines.len(), 3);
    }

    #[test]
    fn each_line_keeps_the_language_most_of_its_words_were_heard_in() {
        let t = Transcript::default();
        let mut p = Pending::default();
        handle_message(
            r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"Ahora. Ahora. okay","confidence":0.9,"words":[
                {"punctuated_word":"Ahora.","speaker":0,"language":"es"},{"punctuated_word":"Ahora.","speaker":0,"language":"es"},
                {"punctuated_word":"यह","speaker":1,"language":"hi"},{"punctuated_word":"ठीक","speaker":1,"language":"hi"},{"punctuated_word":"okay","speaker":1,"language":"en"}]}]}}"#,
            Speaker::Others,
            &t,
            &mut p,
        );
        let lines = t.snapshot().lines;
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].lang.as_deref(), lines[1].lang.as_deref()), (Some("es"), Some("hi")));
    }

    /// Finals before `speech_final` are one sentence still being said; a
    /// pause in one speaker's run does not split it, a change of speaker
    /// does.
    #[test]
    fn finals_gather_until_the_utterance_ends() {
        let t = Transcript::default();
        let mut p = Pending::default();
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"Rest, I'll","words":[]}]}}"#, Speaker::Me, &t, &mut p);
        assert!(t.snapshot().lines.is_empty());
        assert_eq!(t.snapshot().partial_me, "Rest, I'll");
        handle_message(r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"have to","words":[]}]}}"#, Speaker::Me, &t, &mut p);
        assert_eq!(t.snapshot().partial_me, "Rest, I'll have to");
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"have to add speaker.","words":[]}]}}"#, Speaker::Me, &t, &mut p);
        let s = t.snapshot();
        assert_eq!(s.lines.len(), 1);
        assert_eq!(s.lines[0].text, "Rest, I'll have to add speaker.");
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"units and","words":[{"punctuated_word":"units","speaker":0},{"punctuated_word":"and","speaker":0}]}]}}"#, Speaker::Others, &t, &mut p);
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"measurement. okay.","words":[{"punctuated_word":"measurement.","speaker":0},{"punctuated_word":"okay.","speaker":1}]}]}}"#, Speaker::Others, &t, &mut p);
        handle_message(r#"{"type":"UtteranceEnd"}"#, Speaker::Others, &t, &mut p);
        let text = t.text();
        assert!(text.contains("Speaker 1: units and measurement.\n"), "{text}");
        assert!(text.contains("Speaker 2: okay."), "{text}");
        // A short, unsure final is noise and never becomes a line.
        let before = t.snapshot().lines.len();
        handle_message(r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"Allô.","confidence":0.31,"words":[]}]}}"#, Speaker::Me, &t, &mut p);
        assert_eq!(t.snapshot().lines.len(), before);
    }

    #[test]
    fn audio_goes_out_as_little_endian_pcm16() {
        assert_eq!(pcm16(&[0.0, 1.0, -1.0]), vec![0, 0, 0xff, 0x7f, 0x01, 0x80]);
        assert_eq!(pcm16(&[2.0]), vec![0xff, 0x7f], "clamped");
    }

    #[test]
    fn the_query_names_what_the_call_needs() {
        let q = query(48_000.0, true);
        for k in ["model=nova-3", "language=multi", "encoding=linear16", "sample_rate=48000", "diarize=true", "interim_results=true"] {
            assert!(q.contains(k), "{q}");
        }
        assert!(query(16_000.0, false).contains("diarize=false"));
    }

    #[test]
    fn dictation_key_terms_are_encoded_and_switch_to_english() {
        let q = dictation_query(16_000.0, &["Ana María".to_string()]);
        assert!(q.contains("language=en"), "{q}");
        assert!(q.contains("keyterm=Ana%20Mar%C3%ADa"), "{q}");
        assert!(dictation_query(16_000.0, &[]).contains("language=multi"));
    }
}
