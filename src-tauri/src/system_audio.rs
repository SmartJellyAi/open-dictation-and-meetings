//! The other side of a call: what the computer is playing.
//!
//! macOS taps it with Core Audio (`system_audio_mac.rs`), Windows reads a
//! loopback of the default speakers through cpal. Linux has no
//! implementation yet; notes there carry your microphone only, and the
//! page says so.

/// A running capture of what the computer plays. Dropping it stops it.
pub struct SystemCapture {
    pub sample_rate: f64,
    #[cfg(target_os = "macos")]
    _tap: crate::system_audio_mac::SystemAudioTap,
    #[cfg(target_os = "macos")]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(windows)]
    _capture: crate::audio::Capture,
}

/// Whether this system can hear the other side at all.
pub fn available() -> bool {
    cfg!(any(target_os = "macos", windows))
}

impl SystemCapture {
    /// Starts hearing what the computer plays. `on_mono` gets mono float
    /// samples at `sample_rate`, off the audio thread on macOS and on it
    /// on Windows; it must not block.
    #[cfg(target_os = "macos")]
    pub fn start(mut on_mono: impl FnMut(&[f32]) + Send + 'static) -> Result<Self, String> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let sink = Arc::new(crate::system_audio_mac::AudioSink::new());
        let tap = crate::system_audio_mac::SystemAudioTap::start_with_sink(Arc::new(|_peak: f32| {}), Some(sink.clone()))?;
        let sample_rate = tap.format().map(|f| f.mSampleRate).ok().filter(|r| *r > 0.0).unwrap_or(48_000.0);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        // The tap's own thread is realtime; the samples are collected here,
        // every tenth of a second, and handed on.
        std::thread::Builder::new()
            .name("jelly-system-audio".into())
            .spawn(move || {
                while !stop_t.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let samples = sink.drain();
                    if !samples.is_empty() {
                        on_mono(&samples);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { sample_rate, _tap: tap, stop })
    }

    #[cfg(windows)]
    pub fn start(on_mono: impl FnMut(&[f32]) + Send + 'static) -> Result<Self, String> {
        let pending = crate::audio::Capture::open(crate::audio::Source::Loopback)?;
        let sample_rate = pending.sample_rate;
        let capture = pending.start(on_mono)?;
        Ok(Self { sample_rate, _capture: capture })
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn start(_on_mono: impl FnMut(&[f32]) + Send + 'static) -> Result<Self, String> {
        Err("Hearing the other people on a call is not available on this system yet. Your own voice is still written down.".into())
    }
}

#[cfg(target_os = "macos")]
impl Drop for SystemCapture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
