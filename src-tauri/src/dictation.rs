//! Dictation: press the key, talk, and the words are typed into whatever
//! text box is focused, in any app.
//!
//! - Toggle (press to start, press again to stop) or hold (talk while the
//!   key is down), chosen in Settings.
//! - Deepgram's streaming model when there is a key: the words show in the
//!   pill as you speak. Otherwise the recording goes to OpenAI when you
//!   stop.
//! - Nothing is typed while you talk. When you stop, the whole dictation is
//!   tidied once with every sentence in view (misheard words, fillers,
//!   self-corrections, "new line"), then typed once. If the tidy fails the
//!   words are typed as heard.
//! - It stops on its own after a long silence, so a press nobody followed
//!   up on does not leave the microphone open.
//!
//! The audio goes to the recogniser and nowhere else. Nothing is stored.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::settings;
use crate::store::Store;
use crate::transcript::{log, wav_from_samples, Speaker, Transcript};

/// What the pill shows while dictating (`dictation-state`).
#[derive(serde::Serialize, Clone, Default)]
pub struct State {
    pub active: bool,
    /// What has been heard so far.
    pub partial: String,
    /// Stopped, settling the last words and typing.
    pub finishing: bool,
    /// Held mode: the words are typed when the key is let go.
    pub held: bool,
    /// Set when the words could not be typed; `text` is then shown to copy.
    pub error: Option<String>,
    pub text: Option<String>,
}

struct Session {
    stop: Arc<AtomicBool>,
}

pub struct Dictation {
    session: Mutex<Option<Session>>,
    last_tap: Mutex<Option<Instant>>,
}

impl Default for Dictation {
    fn default() -> Self {
        Self { session: Mutex::new(None), last_tap: Mutex::new(None) }
    }
}

/// Stop on its own after this long with nothing heard.
const SILENCE_STOP: Duration = Duration::from_secs(60);
/// Never longer than this in one go.
const MAX_LENGTH: Duration = Duration::from_secs(10 * 60);
/// Two taps closer together than this are one tap: a start and a stop
/// 100 ms apart is never meant.
const DEBOUNCE: Duration = Duration::from_millis(400);

fn emit(app: &AppHandle, state: State) {
    let _ = app.emit("dictation-state", state);
    crate::pill::refresh(app);
}

impl Dictation {
    /// The key in toggle mode: starts when nothing is listening, stops
    /// otherwise.
    pub fn toggle(&self, app: &AppHandle) {
        {
            let mut last = self.last_tap.lock().unwrap();
            if last.map(|t| t.elapsed() < DEBOUNCE).unwrap_or(false) {
                return;
            }
            *last = Some(Instant::now());
        }
        if self.session.lock().unwrap().is_some() {
            self.stop();
        } else {
            self.start(app, false);
        }
    }

    pub fn hold_start(&self, app: &AppHandle) {
        if self.session.lock().unwrap().is_none() {
            self.start(app, true);
        }
    }

    pub fn hold_stop(&self) {
        self.stop();
    }

    pub fn stop(&self) {
        if let Some(s) = self.session.lock().unwrap().take() {
            s.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn start(&self, app: &AppHandle, held: bool) {
        let store = app.state::<Arc<Store>>();
        if !store.get_bool(settings::DICTATION_ON, true) {
            return;
        }
        let deepgram = settings::key(&store, settings::DEEPGRAM_KEY);
        let openai = settings::key(&store, settings::OPENAI_KEY);
        if deepgram.is_none() && openai.is_none() {
            emit(app, State { active: false, error: Some("Add a Deepgram or OpenAI key in Settings to dictate.".into()), ..Default::default() });
            return;
        }
        let mut slot = self.session.lock().unwrap();
        if slot.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *slot = Some(Session { stop: stop.clone() });
        drop(slot);
        let cleanup = store.get_bool(settings::DICTATION_CLEANUP, true);
        let mic = store.get_or(settings::MIC_DEVICE, "");
        let name = store.get_or(settings::YOUR_NAME, "");
        let app = app.clone();
        emit(&app, State { active: true, held, ..Default::default() });
        std::thread::Builder::new()
            .name("odm-dictation".into())
            .spawn(move || {
                let result = run(&app, &stop, held, deepgram, openai.clone(), mic, &name);
                // Whatever happened, this session is over (and only this
                // one: a new press may already have started the next).
                {
                    let dictation = app.state::<Dictation>();
                    let mut slot = dictation.session.lock().unwrap();
                    if slot.as_ref().is_some_and(|s| Arc::ptr_eq(&s.stop, &stop)) {
                        slot.take();
                    }
                }
                match result {
                    Ok(heard) if heard.trim().is_empty() => emit(&app, State::default()),
                    Ok(heard) => {
                        emit(&app, State { active: true, finishing: true, partial: heard.clone(), held, ..Default::default() });
                        let text = match (&openai, cleanup) {
                            (Some(key), true) => tidy(key, &heard),
                            _ => apply_commands(&heard),
                        };
                        // The key that started this may still be down; a
                        // moment lets the app in front take keys again.
                        std::thread::sleep(Duration::from_millis(120));
                        match crate::platform::type_text(&text) {
                            Ok(()) => {
                                log(&format!("dictation typed ({} words)", text.split_whitespace().count()));
                                emit(&app, State::default());
                            }
                            Err(e) => emit(&app, State { active: false, error: Some(e), text: Some(text), ..Default::default() }),
                        }
                    }
                    Err(e) => emit(&app, State { active: false, error: Some(e), ..Default::default() }),
                }
            })
            .ok();
    }
}

/// Listens until stopped, and returns what was said.
fn run(app: &AppHandle, stop: &AtomicBool, held: bool, deepgram: Option<String>, openai: Option<String>, mic: String, your_name: &str) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let pending = crate::audio::Capture::open(crate::audio::Source::Microphone((!mic.is_empty()).then_some(mic)))?;
    let rate = pending.sample_rate;
    let capture = pending.start(move |s| {
        let _ = tx.send(s.to_vec());
    })?;
    let transcript = Arc::new(Transcript::default());
    transcript.reset();
    let keyterms: Vec<String> = your_name.split_whitespace().map(str::to_string).collect();
    let mut stream = deepgram.as_ref().map(|key| crate::deepgram::LiveStream::open_dictation(key, rate, transcript.clone(), &keyterms));
    // Kept too while streaming: if the stream drops, the recording goes to
    // OpenAI instead and nothing said is lost.
    let mut recording: Vec<f32> = Vec::new();
    let max_samples = (rate * MAX_LENGTH.as_secs_f64()) as usize;
    let started = Instant::now();
    let mut last_voice = Instant::now();
    let mut last_shown = String::new();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(60));
        while let Ok(s) = rx.try_recv() {
            if crate::audio::peak(&s) > 0.04 {
                last_voice = Instant::now();
            }
            if let Some(st) = &stream {
                st.push(&s);
            }
            if recording.len() < max_samples {
                recording.extend_from_slice(&s);
            }
        }
        if stream.as_ref().is_some_and(|s| s.failed()) {
            log("dictation stream dropped; the recording goes in one piece instead");
            stream = None;
            if openai.is_none() {
                return Err("The live transcript dropped and there is no OpenAI key to fall back on.".into());
            }
        }
        if stream.is_some() {
            let snap = transcript.snapshot();
            let shown = format!("{} {}", transcript.said_by(Speaker::Me), snap.partial_me).trim().to_string();
            if shown != last_shown {
                last_shown = shown.clone();
                let _ = app.emit("dictation-state", State { active: true, partial: shown, held, ..Default::default() });
            }
        }
        if last_voice.elapsed() >= SILENCE_STOP || started.elapsed() >= MAX_LENGTH || capture.failed() {
            break;
        }
    }
    drop(capture);
    let _ = app.emit("dictation-state", State { active: true, finishing: true, partial: last_shown.clone(), held, ..Default::default() });
    if let Some(st) = stream {
        st.close();
        let heard = transcript.said_by(Speaker::Me);
        let tail = transcript.snapshot().partial_me;
        return Ok(format!("{heard} {tail}").trim().to_string());
    }
    let Some(key) = openai else { return Ok(String::new()) };
    if !crate::chunked::has_speech(&recording) {
        return Ok(String::new());
    }
    let wav = wav_from_samples(&recording, rate).ok_or_else(|| "The recording could not be prepared.".to_string())?;
    crate::openai::transcribe(&key, wav, None, None).map(|t| t.trim().to_string())
}

/// The pass over the whole dictation before it is typed.
const TIDY_SYSTEM: &str = "You tidy ONE dictation a person just finished, right before it is typed into the text box \
they are in. Return it corrected.\n\n\
Do:\n\
- Fix only words that are clearly misheard now that the whole text is in view: a word that makes no sense where it \
stands, a punctuation word (\"period\", \"comma\") standing where a real word belongs. Keep every word that is \
plausible as said; this is their text, not yours.\n\
- Treat \"period\", \"comma\", \"full stop\", \"question mark\" as spoken punctuation only where the sentence reads \
complete without that word.\n\
- Drop fillers (um, uh, you know, like as a filler) and false starts. A word or phrase said twice by a stumble \
(\"I want I want to\") appears once.\n\
- Self-corrections, in any language (\"at 5, sorry, 6\", \"no wait\", \"I mean\", \"actually\"): keep only what they \
settled on and drop the slip and the correction words: \"I'll call you at 5, sorry, 6\" is \"I'll call you at 6\". \
Only for a clear correction, never for a new point that merely disagrees with an earlier one.\n\
- Sentence ends: the recogniser ends a sentence at every pause, so a full stop and a capital often land in the \
middle of one sentence. Join those. End each real sentence with the right mark: a question mark for a question, an \
exclamation mark only for a clear exclamation, a full stop otherwise.\n\
- Spoken commands: \"new line\" or \"new paragraph\" become a line break or a blank line; \"delete that\" or \
\"scratch that\" removes the sentence before it.\n\
- Punctuate naturally; keep their tone, wording, order and length. Do not add words, greetings or sign-offs. Do not \
translate. Never write an em dash or an en dash: use a comma, a colon or a full stop.\n\
- If nothing needs fixing, return the text exactly as given.\n\n\
Respond with a single JSON object and nothing else:\n\
{\"text\": \"<the dictation, corrected>\"}";

/// The dictation tidied by the quick model; as heard (with spoken
/// commands applied) on any failure. A slow model never costs the words.
fn tidy(key: &str, heard: &str) -> String {
    let prompt = format!("THE DICTATION: \u{201c}{heard}\u{201d}");
    match crate::openai::generate_json_fast(key, TIDY_SYSTEM, &prompt) {
        Ok(out) => serde_json::from_str::<serde_json::Value>(crate::openai::strip_fence(&out))
            .ok()
            .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|t| unquoted(t, heard)))
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| apply_commands(heard)),
        Err(_) => {
            log("the dictation tidy failed; typing as heard");
            apply_commands(heard)
        }
    }
}

/// The model's text without the quotation marks the prompt wrapped the
/// dictation in, when it echoed them. A dictation that itself began and
/// ended with a quote keeps it.
fn unquoted(out: &str, given: &str) -> String {
    let t = out.trim();
    let given = given.trim();
    let wrapped = |s: &str, open: char, close: char| s.starts_with(open) && s.ends_with(close) && s.chars().count() >= 2;
    for (open, close) in [('\u{201c}', '\u{201d}'), ('"', '"')] {
        if wrapped(t, open, close) && !wrapped(given, open, close) {
            let mut inner: Vec<char> = t.chars().skip(1).collect();
            inner.pop();
            return inner.into_iter().collect::<String>().trim().to_string();
        }
    }
    t.to_string()
}

/// Spoken commands applied without the model: "new line", "new
/// paragraph", and "delete that" taking back the sentence before it.
pub fn apply_commands(heard: &str) -> String {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in heard.chars() {
        current.push(ch);
        if matches!(ch, '.' | '?' | '!') {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    let mut out: Vec<String> = Vec::new();
    for s in sentences {
        let bare = s.trim().trim_end_matches(['.', '!', ',', '?']).trim().to_lowercase();
        match bare.as_str() {
            "delete that" | "scratch that" | "undo that" => {
                out.pop();
            }
            "new line" | "newline" | "line break" => out.push("\n".into()),
            "new paragraph" => out.push("\n\n".into()),
            _ => out.push(s.trim().to_string()),
        }
    }
    let mut text = String::new();
    for piece in out {
        if piece.starts_with('\n') {
            text = text.trim_end().to_string();
            text.push_str(&piece);
            continue;
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push(' ');
        }
        text.push_str(&piece);
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_commands_become_breaks_and_take_backs() {
        assert_eq!(apply_commands("Hello there. New line. Second line."), "Hello there.\nSecond line.");
        assert_eq!(apply_commands("Keep this. Drop this. Delete that. And this."), "Keep this. And this.");
        assert_eq!(apply_commands("One. New paragraph. Two"), "One.\n\nTwo");
        assert_eq!(apply_commands("just words"), "just words");
        assert_eq!(apply_commands(""), "");
    }

    #[test]
    fn echoed_quotes_are_removed_but_real_ones_stay() {
        assert_eq!(unquoted("\u{201c}hello\u{201d}", "hello"), "hello");
        assert_eq!(unquoted("\"hello\"", "\"hello\""), "\"hello\"");
        assert_eq!(unquoted("  plain  ", "plain"), "plain");
    }

    #[test]
    fn the_tidy_prompt_has_no_dashes() {
        assert!(!TIDY_SYSTEM.contains('\u{2014}') && !TIDY_SYSTEM.contains('\u{2013}'));
    }
}
