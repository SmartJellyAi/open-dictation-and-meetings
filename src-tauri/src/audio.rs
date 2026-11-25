//! Audio in: the microphone on every system, and what the speakers play
//! on Windows (a loopback of the default output). macOS hears the speakers
//! through its own tap (`system_audio_mac.rs`); Linux has no system side
//! yet.
//!
//! The arithmetic (downmix, resampling) is plain Rust and tested on any
//! system. The device code goes through cpal, which speaks Core Audio,
//! WASAPI and ALSA.
//!
//! No echo cancellation here: with speakers rather than headphones, the
//! microphone also hears the other side of a call.

/// Interleaved frames to mono.
///
/// `live_only` averages only the channels that carry signal in this
/// buffer, as the Mac's microphone tap does: a microphone wired to one
/// side of a stereo input would otherwise come out at half volume.
pub fn downmix_into(interleaved: &[f32], channels: usize, live_only: bool, out: &mut Vec<f32>) {
    out.clear();
    let ch = channels.max(1);
    let clean = |v: f32| if v.is_finite() { v } else { 0.0 };
    if ch == 1 {
        out.extend(interleaved.iter().map(|v| clean(*v)));
        return;
    }
    let mut live = [true; 32];
    let mut live_count = ch.min(32);
    if live_only {
        let mut peaks = [0.0f32; 32];
        for frame in interleaved.chunks_exact(ch) {
            for (c, v) in frame.iter().enumerate().take(32) {
                peaks[c] = peaks[c].max(clean(*v).abs());
            }
        }
        live_count = 0;
        for c in 0..ch.min(32) {
            live[c] = peaks[c] > 1e-4;
            if live[c] {
                live_count += 1;
            }
        }
    }
    if live_count == 0 {
        out.resize(interleaved.len() / ch, 0.0);
        return;
    }
    for frame in interleaved.chunks_exact(ch) {
        let mut sum = 0.0f32;
        for (c, v) in frame.iter().enumerate().take(32) {
            if live[c] {
                sum += clean(*v);
            }
        }
        out.push(sum / live_count as f32);
    }
}

pub use device::{input_devices, Capture, Source};

mod device {
    use super::downmix_into;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    /// Which device to listen to.
    #[derive(Clone, PartialEq, Eq, Debug)]
    pub enum Source {
        /// A microphone by name; None is the system's default.
        Microphone(Option<String>),
        /// What the default speakers or headphones are playing. Windows only.
        Loopback,
    }

    type OnMono = Box<dyn FnMut(&[f32]) + Send + 'static>;

    /// The names of the microphones the system offers.
    pub fn input_devices() -> Vec<String> {
        let host = cpal::default_host();
        let mut out: Vec<String> = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for d in devices {
                if let Ok(name) = d.name() {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
        out
    }

    /// The thread that owns a stream. A stream cannot move between
    /// threads, so it is built, kept and dropped on one of its own.
    struct Owner {
        stop: Arc<AtomicBool>,
        failed: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// A device that is open and whose rate is known, not yet running.
    /// Two steps because callers build their pipeline from the rate
    /// before the first buffer arrives.
    pub struct Pending {
        pub sample_rate: f64,
        go: mpsc::Sender<OnMono>,
        started: mpsc::Receiver<Result<(), String>>,
        owner: Owner,
    }

    /// A running input stream. Dropping it stops and closes the device.
    pub struct Capture {
        owner: Owner,
    }

    impl Capture {
        pub fn open(source: Source) -> Result<Pending, String> {
            if source == Source::Loopback && !cfg!(windows) {
                return Err("Hearing what the speakers play is not available on this system.".into());
            }
            let (info_tx, info_rx) = mpsc::channel::<Result<(f64, String), String>>();
            let (go_tx, go_rx) = mpsc::channel::<OnMono>();
            let (started_tx, started_rx) = mpsc::channel::<Result<(), String>>();
            let stop = Arc::new(AtomicBool::new(false));
            let failed = Arc::new(AtomicBool::new(false));
            let (stop_t, failed_t) = (stop.clone(), failed.clone());
            let thread = std::thread::Builder::new()
                .name("jelly-audio-in".into())
                .spawn(move || capture_thread(source, info_tx, go_rx, started_tx, stop_t, failed_t))
                .map_err(|e| e.to_string())?;
            let owner = Owner { stop, failed, thread: Some(thread) };
            match info_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok((sample_rate, device_name))) => {
                    eprintln!("[audio] listening to {device_name} at {sample_rate} Hz");
                    Ok(Pending { sample_rate, go: go_tx, started: started_rx, owner })
                }
                Ok(Err(e)) => Err(e),
                Err(_) => {
                    // Not joined: a device that hangs must not hang the caller.
                    let mut owner = owner;
                    owner.stop.store(true, Ordering::SeqCst);
                    owner.thread.take();
                    Err("The microphone did not answer.".into())
                }
            }
        }

        /// True once the device reported an error (unplugged, or the
        /// default changed under it). The stream gives nothing after that.
        pub fn failed(&self) -> bool {
            self.owner.failed.load(Ordering::SeqCst)
        }
    }

    impl Pending {
        /// Starts the stream. `on_mono` is called on the audio thread with
        /// each buffer as mono float samples at `sample_rate`; it must not
        /// block.
        pub fn start(self, on_mono: impl FnMut(&[f32]) + Send + 'static) -> Result<Capture, String> {
            let Pending { go, started, owner, .. } = self;
            go.send(Box::new(on_mono)).map_err(|_| "The microphone closed.".to_string())?;
            match started.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(())) => Ok(Capture { owner }),
                Ok(Err(e)) => Err(e),
                Err(_) => {
                    let mut owner = owner;
                    owner.stop.store(true, Ordering::SeqCst);
                    owner.thread.take();
                    Err("The microphone did not start.".into())
                }
            }
        }
    }

    fn find_input(host: &cpal::Host, name: Option<&str>) -> Option<cpal::Device> {
        if let Some(want) = name.filter(|n| !n.trim().is_empty()) {
            if let Ok(mut devices) = host.input_devices() {
                if let Some(d) = devices.find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
                    return Some(d);
                }
            }
            eprintln!("[audio] the chosen microphone is not connected; using the default");
        }
        host.default_input_device()
    }

    fn capture_thread(
        source: Source,
        info_tx: mpsc::Sender<Result<(f64, String), String>>,
        go_rx: mpsc::Receiver<OnMono>,
        started_tx: mpsc::Sender<Result<(), String>>,
        stop: Arc<AtomicBool>,
        failed: Arc<AtomicBool>,
    ) {
        let host = cpal::default_host();
        let device = match &source {
            Source::Microphone(name) => find_input(&host, name.as_deref()),
            Source::Loopback => host.default_output_device(),
        };
        let Some(device) = device else {
            let _ = info_tx.send(Err(match source {
                Source::Microphone(_) => "No microphone was found.".to_string(),
                Source::Loopback => "No speakers or headphones were found.".to_string(),
            }));
            return;
        };
        // Loopback reads the output device in its own mix format.
        let config = match source {
            Source::Microphone(_) => device.default_input_config(),
            Source::Loopback => device.default_output_config(),
        };
        let config = match config {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[audio] reading the input format failed: {e}");
                let _ = info_tx.send(Err("The microphone could not be read. Check that this app may use it.".into()));
                return;
            }
        };
        let rate = config.sample_rate().0 as f64;
        let channels = config.channels() as usize;
        let name = device.name().unwrap_or_default();
        if info_tx.send(Ok((rate, name))).is_err() {
            return;
        }
        let Ok(mut on_mono) = go_rx.recv() else { return };
        let live_only = matches!(source, Source::Microphone(_));
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let failed_cb = failed.clone();
        let on_err = move |e: cpal::StreamError| {
            eprintln!("[audio] input stream error: {e}");
            failed_cb.store(true, Ordering::SeqCst);
        };
        let mut scratch: Vec<f32> = Vec::new();
        let mut mono: Vec<f32> = Vec::new();
        let built = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    downmix_into(data, channels, live_only, &mut mono);
                    on_mono(&mono);
                },
                on_err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    scratch.clear();
                    scratch.extend(data.iter().map(|s| *s as f32 / 32768.0));
                    downmix_into(&scratch, channels, live_only, &mut mono);
                    on_mono(&mono);
                },
                on_err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    scratch.clear();
                    scratch.extend(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0));
                    downmix_into(&scratch, channels, live_only, &mut mono);
                    on_mono(&mono);
                },
                on_err,
                None,
            ),
            other => {
                eprintln!("[audio] input sample format not handled: {other:?}");
                let _ = started_tx.send(Err("The microphone uses a format this app cannot read.".into()));
                return;
            }
        };
        let stream = match built {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[audio] opening the input stream failed: {e}");
                let _ = started_tx.send(Err("The microphone could not be opened. Check that this app may use it.".into()));
                return;
            }
        };
        if let Err(e) = stream.play() {
            eprintln!("[audio] starting the input stream failed: {e}");
            let _ = started_tx.send(Err("The microphone could not be started.".into()));
            return;
        }
        let _ = started_tx.send(Ok(()));
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(40));
        }
        drop(stream);
    }
}

/// Peak of a buffer, for the level meter.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| if s.is_finite() { m.max(s.abs()) } else { m })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_becomes_the_average_of_both_sides() {
        let mut out = Vec::new();
        downmix_into(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, false, &mut out);
        assert_eq!(out, vec![0.5, 0.5, 0.0]);
    }

    /// A microphone on one side of a stereo input keeps its level.
    #[test]
    fn a_one_sided_microphone_is_not_halved() {
        let mut out = Vec::new();
        downmix_into(&[0.4, 0.0, -0.2, 0.0], 2, true, &mut out);
        assert_eq!(out, vec![0.4, -0.2]);
        downmix_into(&[0.0, 0.0, 0.0, 0.0], 2, true, &mut out);
        assert_eq!(out, vec![0.0, 0.0]);
    }

    #[test]
    fn a_bad_sample_becomes_silence_not_noise() {
        let mut out = Vec::new();
        downmix_into(&[f32::NAN, 0.5], 1, false, &mut out);
        assert_eq!(out, vec![0.0, 0.5]);
    }
}
