//! The running transcript of a call, both sides kept apart, and how it is
//! stored and read back.
//!
//! A call has two audio paths and they must stay apart: the microphone is
//! you, everything the computer plays is everyone else. Merging them would
//! give notes that cannot tell "you agreed to do this" from "they agreed
//! to do this", the most important distinction in a meeting note.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Who said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    /// You, from the microphone.
    Me,
    /// Everyone else, from what the computer plays.
    Others,
}

impl Speaker {
    pub fn label(self) -> &'static str {
        match self {
            Speaker::Me => "You",
            Speaker::Others => "Others",
        }
    }
}

/// One settled run of speech from one side.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Line {
    pub speaker: Speaker,
    /// Which of the other participants, 1-based, when the stream told
    /// them apart. Never set for you, who are one person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_index: Option<u32>,
    pub text: String,
    /// The language most of the line's words were heard in, when the
    /// streaming recogniser tagged them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

impl Line {
    /// "You", "Others", or "Speaker 2": the prefix the stored transcript
    /// carries, and what the view shows.
    pub fn label(&self) -> String {
        match (self.speaker, self.speaker_index) {
            (Speaker::Others, Some(n)) => format!("Speaker {n}"),
            (who, _) => who.label().to_string(),
        }
    }
}

/// What the page receives: settled lines, plus each side's live guess.
#[derive(Clone, serde::Serialize)]
pub struct TranscriptUpdate {
    pub lines: Vec<Line>,
    pub partial_me: String,
    pub partial_others: String,
}

#[derive(Default)]
pub struct Transcript {
    lines: Mutex<Vec<Line>>,
    partial_me: Mutex<String>,
    partial_others: Mutex<String>,
    /// Buffers lost on the way, carried up from the audio side.
    pub dropped: AtomicU64,
    /// When anyone last said anything, ms since the epoch. A live guess
    /// counts, so someone mid-sentence is still speaking.
    last_heard_ms: AtomicU64,
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Transcript {
    pub fn snapshot(&self) -> TranscriptUpdate {
        TranscriptUpdate {
            lines: self.lines.lock().unwrap().clone(),
            partial_me: self.partial_me.lock().unwrap().clone(),
            partial_others: self.partial_others.lock().unwrap().clone(),
        }
    }

    fn partial_slot(&self, who: Speaker) -> &Mutex<String> {
        match who {
            Speaker::Me => &self.partial_me,
            Speaker::Others => &self.partial_others,
        }
    }

    /// The whole thing as text, for storage and for the summary.
    /// Speaker-labelled, because an unlabelled transcript of a
    /// conversation is close to useless.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for line in self.lines.lock().unwrap().iter() {
            out.push_str(&line.label());
            out.push_str(": ");
            out.push_str(&line.text);
            out.push('\n');
        }
        // Words still in flight are included, so a call that ends
        // mid-sentence keeps the sentence.
        for who in [Speaker::Me, Speaker::Others] {
            let p = self.partial_slot(who).lock().unwrap();
            if !p.trim().is_empty() {
                out.push_str(who.label());
                out.push_str(": ");
                out.push_str(p.trim());
                out.push('\n');
            }
        }
        out
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.lines.lock().unwrap().is_empty() && self.partial_me.lock().unwrap().trim().is_empty() && self.partial_others.lock().unwrap().trim().is_empty()
    }

    /// The settled lines of one side, joined: what a dictation said.
    pub fn said_by(&self, who: Speaker) -> String {
        self.lines.lock().unwrap().iter().filter(|l| l.speaker == who).map(|l| l.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
    }

    /// How long since either side said anything.
    pub fn silent_for_ms(&self) -> u64 {
        let last = self.last_heard_ms.load(Ordering::Relaxed);
        if last == 0 {
            return 0;
        }
        epoch_ms().saturating_sub(last)
    }

    fn heard(&self) {
        self.last_heard_ms.store(epoch_ms(), Ordering::Relaxed);
    }

    /// A new call must not inherit the last one's words.
    pub fn reset(&self) {
        self.heard();
        self.lines.lock().unwrap().clear();
        self.partial_me.lock().unwrap().clear();
        self.partial_others.lock().unwrap().clear();
        self.dropped.store(0, Ordering::Relaxed);
    }

    pub fn set_partial(&self, who: Speaker, text: &str) {
        if !text.trim().is_empty() {
            self.heard();
        }
        *self.partial_slot(who).lock().unwrap() = text.to_string();
    }

    /// A settled line. The stream's finals do not overlap or revise each
    /// other, so two people saying "yes" in a row are two lines.
    pub fn append_final(&self, who: Speaker, speaker_index: Option<u32>, text: &str) {
        self.append_final_in(who, speaker_index, text, None);
    }

    pub fn append_final_in(&self, who: Speaker, speaker_index: Option<u32>, text: &str, lang: Option<String>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.heard();
        let line = Line { speaker: who, speaker_index, text: text.to_string(), lang };
        self.lines.lock().unwrap().push(line);
        self.partial_slot(who).lock().unwrap().clear();
    }
}

/// The transcript with the other side named: on a call with one other
/// person, "Others:" and "Speaker N:" lines are that person's. Stored text
/// keeps the plain labels; this is how it is read.
pub fn named(transcript: &str, other: &str) -> String {
    let name = other.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::with_capacity(transcript.len());
    for line in transcript.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let nl = if line.ends_with('\n') { "\n" } else { "" };
        if let Some(rest) = body.strip_prefix("Others: ") {
            out.push_str(&format!("{name}: {rest}{nl}"));
        } else if body == "[cloud, Others]" {
            out.push_str(&format!("[cloud, {name}]{nl}"));
        } else if body.starts_with("Speaker ") && body.split_once(": ").is_some_and(|(n, _)| n.strip_prefix("Speaker ").is_some_and(|d| d.chars().all(|c| c.is_ascii_digit()))) {
            out.push_str(&format!("{name}: {}{nl}", body.split_once(": ").map(|(_, r)| r).unwrap_or("")));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Reads a stored transcript back into speaker-labelled lines. A line
/// without a label is kept, attributed to whoever spoke last.
pub fn parse_transcript(stored: &str) -> Vec<Line> {
    let mut current = Speaker::Others;
    let mut out = Vec::new();
    for raw in stored.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "[cloud, You]" => {
                current = Speaker::Me;
                continue;
            }
            "[cloud, Others]" => {
                current = Speaker::Others;
                continue;
            }
            _ => {}
        }
        if let Some(rest) = line.strip_prefix("You: ") {
            out.push(Line { speaker: Speaker::Me, speaker_index: None, text: rest.trim().to_string(), lang: None });
        } else if let Some(rest) = line.strip_prefix("Others: ") {
            out.push(Line { speaker: Speaker::Others, speaker_index: None, text: rest.trim().to_string(), lang: None });
        } else if let Some((n, rest)) = line.strip_prefix("Speaker ").and_then(|r| r.split_once(": ")) {
            match n.parse::<u32>() {
                Ok(n) => out.push(Line { speaker: Speaker::Others, speaker_index: Some(n), text: rest.trim().to_string(), lang: None }),
                Err(_) => out.push(Line { speaker: current, speaker_index: None, text: line.to_string(), lang: None }),
            }
        } else {
            out.push(Line { speaker: current, speaker_index: None, text: line.to_string(), lang: None });
        }
    }
    out
}

/// Mono float samples to a 16 kHz, 16-bit WAV for a transcription request.
///
/// Going down from 48 kHz, each output sample is the average of the input
/// samples it covers: picking every third sample folds sound above 8 kHz
/// back into the speech band as noise.
pub fn wav_from_samples(samples: &[f32], sample_rate: f64) -> Option<Vec<u8>> {
    if samples.is_empty() || sample_rate <= 0.0 {
        return None;
    }
    const TARGET: f64 = 16_000.0;
    let step = (sample_rate / TARGET).max(1.0);
    let mut pcm: Vec<u8> = Vec::with_capacity((samples.len() as f64 / step) as usize * 2);
    let mut pos = 0.0f64;
    while (pos as usize) < samples.len() {
        let start = pos as usize;
        let end = ((pos + step) as usize).min(samples.len());
        let span = &samples[start..end.max(start + 1).min(samples.len())];
        let v = if span.is_empty() { 0.0 } else { span.iter().sum::<f32>() / span.len() as f32 };
        pcm.extend_from_slice(&((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
        pos += step;
    }
    let data_len = pcm.len() as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&(TARGET as u32).to_le_bytes());
    wav.extend_from_slice(&(TARGET as u32 * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm);
    Some(wav)
}

/// A line in the app's log file, for finding out later why a call came
/// out the way it did. Never the words of a call, only what happened.
pub fn log(message: &str) {
    use std::io::Write;
    eprintln!("[odm] {message}");
    if cfg!(test) {
        return;
    }
    let dir = crate::paths::logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("app.log")) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs} {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_on_one_names_the_other_side() {
        let t = "You: hi\nOthers: hello\nSpeaker 1: give me a number\n[cloud, Others]\nsure\n";
        assert_eq!(named(t, "Priya Shah"), "You: hi\nPriya: hello\nPriya: give me a number\n[cloud, Priya]\nsure\n");
        assert_eq!(named(t, "  "), t, "nobody known: unchanged");
    }

    /// The WAV handed to the cloud has to be a WAV.
    #[test]
    fn samples_become_a_real_wav() {
        let samples: Vec<f32> = (0..48_000).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let wav = wav_from_samples(&samples, 48_000.0).expect("one second of audio");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(data_len, wav.len() - 44, "declared length must match the payload");
        assert!((30_000..34_000).contains(&data_len), "a second of 16kHz mono PCM16, got {data_len} bytes");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    }

    /// A 12 kHz tone cannot exist in a 16 kHz recording; averaging keeps
    /// it from folding down into speech as noise.
    #[test]
    fn downsampling_does_not_fold_high_frequencies_into_speech() {
        let rate = 48_000.0;
        let tone: Vec<f32> = (0..48_000).map(|i| (2.0 * std::f32::consts::PI * 12_000.0 * i as f32 / rate as f32).sin() * 0.9).collect();
        let wav = wav_from_samples(&tone, rate).expect("a second of tone");
        let samples: Vec<i16> = wav[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        let rms = (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() / i16::MAX as f64;
        assert!(rms < 0.25, "a 12kHz tone must not survive downsampling (rms {rms:.3})");
    }

    #[test]
    fn an_empty_recording_is_not_sent() {
        assert!(wav_from_samples(&[], 48_000.0).is_none());
    }

    #[test]
    fn a_stored_transcript_reads_back_as_speakers() {
        let lines = parse_transcript("You: we agreed on Friday\nOthers: works for me\nYou: good");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].speaker, Speaker::Me);
        assert_eq!(lines[0].text, "we agreed on Friday");
        assert_eq!(lines[1].speaker, Speaker::Others);
        assert_eq!(lines[2].text, "good");
    }

    #[test]
    fn speaker_numbers_and_cloud_markers_read_back_correctly() {
        let stored = "You: hello\nSpeaker 2: hi there\nSpeaker 1: go on\n\n[cloud, Others]\nthey said this\n\n[cloud, You]\nI said that\n";
        let lines = parse_transcript(stored);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!((lines[1].speaker, lines[1].speaker_index), (Speaker::Others, Some(2)));
        assert_eq!(lines[1].text, "hi there");
        assert_eq!((lines[2].speaker, lines[2].speaker_index), (Speaker::Others, Some(1)));
        assert_eq!((lines[3].speaker, lines[3].text.as_str()), (Speaker::Others, "they said this"));
        assert_eq!((lines[4].speaker, lines[4].text.as_str()), (Speaker::Me, "I said that"));
        let t = Transcript::default();
        t.append_final(Speaker::Others, Some(2), "hi there");
        t.append_final(Speaker::Me, None, "hello");
        assert_eq!(t.text(), "Speaker 2: hi there\nYou: hello\n");
        let back = parse_transcript(&t.text());
        assert_eq!(back[0].speaker_index, Some(2));
        assert_eq!(back[1].speaker, Speaker::Me);
    }

    #[test]
    fn an_unlabelled_line_is_still_kept() {
        let lines = parse_transcript("just some words with no prefix");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "just some words with no prefix");
    }

    #[test]
    fn the_saved_text_includes_words_still_in_flight() {
        let t = Transcript::default();
        t.append_final(Speaker::Me, None, "first");
        t.set_partial(Speaker::Others, "still talk");
        assert_eq!(t.text(), "You: first\nOthers: still talk\n");
        assert!(!t.is_empty());
    }

    #[test]
    fn a_partial_is_replaced_not_appended() {
        let t = Transcript::default();
        t.set_partial(Speaker::Me, "we should");
        t.set_partial(Speaker::Me, "we should ship");
        assert_eq!(t.snapshot().partial_me, "we should ship");
        t.append_final(Speaker::Me, None, "we should ship it");
        assert_eq!(t.snapshot().partial_me, "", "a final clears the guess");
    }

    #[test]
    fn a_new_meeting_starts_empty() {
        let t = Transcript::default();
        t.append_final(Speaker::Me, None, "old call");
        t.reset();
        assert!(t.is_empty());
        assert!(t.silent_for_ms() < 1_000);
    }

    #[test]
    fn what_one_side_said_reads_as_one_text() {
        let t = Transcript::default();
        t.append_final(Speaker::Me, None, "Hello there.");
        t.append_final(Speaker::Others, None, "Hi.");
        t.append_final(Speaker::Me, None, "Second sentence.");
        assert_eq!(t.said_by(Speaker::Me), "Hello there. Second sentence.");
    }
}
