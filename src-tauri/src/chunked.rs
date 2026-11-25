//! Transcribing a call in pieces with OpenAI, for when there is no
//! Deepgram key (or the live stream dropped for good).
//!
//! A piece closes at the first pause after the target length, never on the
//! clock: cutting a monologue mid-sentence makes the model guess at half a
//! word on each side of the seam. The previous piece's text goes along as
//! context, so names and topic carry over. Each piece's text becomes one
//! line of the transcript, under the side it came from.

use std::sync::Mutex;

/// How much audio to gather before looking for a place to cut.
pub const TARGET_SECONDS: f64 = 45.0;

/// A piece is never allowed past this, pause or no pause.
pub const MAX_SECONDS: f64 = 75.0;

/// Below this peak, a stretch counts as a pause worth cutting at.
const QUIET_PEAK: f32 = 0.02;

/// How long a stretch has to be quiet to be a sentence boundary rather
/// than the gap between two words.
const PAUSE_SECONDS: f64 = 0.45;

/// Audio waiting to be sent.
pub struct Chunker {
    sample_rate: f64,
    pending: Mutex<Vec<f32>>,
}

impl Chunker {
    pub fn new(sample_rate: f64) -> Self {
        Self { sample_rate, pending: Mutex::new(Vec::new()) }
    }

    pub fn push(&self, samples: &[f32]) {
        self.pending.lock().unwrap().extend_from_slice(samples);
    }

    fn samples_for(&self, seconds: f64) -> usize {
        (self.sample_rate * seconds) as usize
    }

    /// Takes a piece if one is ready, leaving the rest for next time.
    /// `final_flush` takes whatever is left, however short.
    pub fn take_chunk(&self, final_flush: bool) -> Option<Vec<f32>> {
        let mut pending = self.pending.lock().unwrap();
        let target = self.samples_for(TARGET_SECONDS);
        if pending.is_empty() || (pending.len() < target && !final_flush) {
            return None;
        }
        let cut = if final_flush { pending.len() } else { find_cut(&pending, target, self.samples_for(MAX_SECONDS), self.samples_for(PAUSE_SECONDS)) };
        let chunk: Vec<f32> = pending.drain(..cut).collect();
        Some(chunk)
    }
}

/// Whether a piece holds anything worth sending: silence costs money and
/// comes back as an invented sentence.
pub fn has_speech(samples: &[f32]) -> bool {
    samples.iter().any(|s| s.abs() >= QUIET_PEAK * 2.0)
}

/// Where to cut, given the audio so far: the first quiet stretch at or
/// after `target`, else `max`, because a piece that grows forever is worse
/// than one seam in a sentence.
fn find_cut(samples: &[f32], target: usize, max: usize, pause_len: usize) -> usize {
    let hard = max.min(samples.len());
    if samples.len() <= target {
        return samples.len();
    }
    let window = pause_len.max(1);
    let mut i = target;
    while i + window <= hard {
        let quiet = samples[i..i + window].iter().all(|s| s.abs() < QUIET_PEAK);
        if quiet {
            // In the middle of the pause, so neither side clips the speech
            // on its own edge.
            return i + window / 2;
        }
        i += window / 2;
    }
    hard
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 16_000.0;

    fn speech(seconds: f64) -> Vec<f32> {
        (0..(RATE * seconds) as usize).map(|i| ((i % 100) as f32 / 50.0 - 1.0) * 0.6).collect()
    }
    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; (RATE * seconds) as usize]
    }

    #[test]
    fn a_chunk_ends_at_a_pause_not_on_the_clock() {
        let mut audio = speech(TARGET_SECONDS + 5.0);
        audio.extend(silence(1.0));
        audio.extend(speech(30.0));
        let cut = find_cut(&audio, (RATE * TARGET_SECONDS) as usize, (RATE * MAX_SECONDS) as usize, (RATE * PAUSE_SECONDS) as usize);
        let cut_seconds = cut as f64 / RATE;
        assert!((TARGET_SECONDS + 5.0..TARGET_SECONDS + 6.5).contains(&cut_seconds), "cut at {cut_seconds:.1}s");
    }

    #[test]
    fn an_unbroken_monologue_is_cut_at_the_ceiling() {
        let audio = speech(MAX_SECONDS + 60.0);
        let cut = find_cut(&audio, (RATE * TARGET_SECONDS) as usize, (RATE * MAX_SECONDS) as usize, (RATE * PAUSE_SECONDS) as usize);
        assert_eq!(cut, (RATE * MAX_SECONDS) as usize);
    }

    #[test]
    fn a_short_meeting_waits_until_it_ends() {
        let t = Chunker::new(RATE);
        t.push(&speech(20.0));
        assert!(t.take_chunk(false).is_none(), "20 seconds is not a chunk yet");
        let last = t.take_chunk(true).expect("the final flush takes whatever is left");
        assert!((last.len() as f64 / RATE - 20.0).abs() < 0.1, "{}", last.len());
    }

    /// Audio is consumed once: nothing is paid for twice, nothing is lost.
    #[test]
    fn every_second_is_sent_exactly_once() {
        let t = Chunker::new(RATE);
        let mut audio = speech(TARGET_SECONDS + 3.0);
        audio.extend(silence(1.0));
        audio.extend(speech(20.0));
        let spoken = audio.len();
        t.push(&audio);
        let first = t.take_chunk(false).expect("a full chunk");
        assert!(t.take_chunk(false).is_none(), "the remainder is not a chunk on its own");
        let second = t.take_chunk(true).expect("the remainder");
        assert_eq!(first.len() + second.len(), spoken);
    }

    #[test]
    fn silence_is_not_worth_sending() {
        assert!(!has_speech(&silence(2.0)));
        assert!(has_speech(&speech(0.5)));
    }
}
