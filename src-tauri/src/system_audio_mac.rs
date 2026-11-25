//! What the Mac plays, heard through a Core Audio process tap (macOS
//! 14.4 and later).
//!
//! The tap runs under System Audio Recording only: no screen access, and
//! the indicator stays the audio one. Four objects, created in order and
//! destroyed in reverse:
//!
//! 1. a process object for this app itself, so it can be left out;
//! 2. a private tap over everything else;
//! 3. an aggregate device that owns the tap and follows the real output
//!    device, so a switch to headphones mid-call keeps working;
//! 4. an IOProc that hands over buffers on Core Audio's own thread.
//!
//! Audio keeps playing normally: the tap reads a copy. Nothing is written
//! to disk; samples pass through a short buffer on their way to the
//! transcriber.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
    kAudioDevicePropertyDeviceUID, kAudioHardwarePropertyDefaultSystemOutputDevice,
    kAudioHardwarePropertyProcessObjectList, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioSubTapDriftCompensationKey,
    kAudioSubTapUIDKey, AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress, CATapDescription,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};

/// `kAudioTapPropertyUID`, spelled `'tuid'`. Not re-exported by the
/// binding crate, so it is written out here as the four-char code it is.
const K_AUDIO_TAP_PROPERTY_UID: u32 = u32::from_be_bytes(*b"tuid");
/// `kAudioTapPropertyFormat`, spelled `'tfmt'`.
const K_AUDIO_TAP_PROPERTY_FORMAT: u32 = u32::from_be_bytes(*b"tfmt");

/// The aggregate-device dictionary keys come through the bindings as C
/// strings, but the dictionary itself is built from `NSString`s.
fn key(k: &std::ffi::CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().expect("Core Audio keys are ASCII literals"))
}

fn global_address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property off a Core Audio object.
///
/// # Safety
/// `T` must be the type Core Audio actually stores for `selector`.
unsafe fn get_property<T>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    qualifier: Option<(&[u8], u32)>,
) -> Result<T, String> {
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let (qual_ptr, qual_size) = match qualifier {
        Some((bytes, len)) => (bytes.as_ptr() as *const c_void, len),
        None => (std::ptr::null(), 0),
    };
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address),
            qual_size,
            qual_ptr,
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr() as *mut c_void).unwrap(),
        )
    };
    if status != 0 {
        return Err(format!("Core Audio property {:#x} failed: {status}", address.mSelector));
    }
    Ok(unsafe { value.assume_init() })
}

/// The Core Audio object representing a running process, found from its
/// PID. Needed so Jelly can exclude *itself* from the tap -- otherwise the
/// voice overlay's own speech comes back in as if a participant had said
/// it.
fn process_object_for_pid(pid: i32) -> Result<AudioObjectID, String> {
    let address = global_address(kAudioHardwarePropertyTranslatePIDToProcessObject);
    let pid_bytes = pid.to_ne_bytes();
    unsafe {
        get_property::<AudioObjectID>(
            kAudioObjectSystemObject as AudioObjectID,
            &address,
            Some((&pid_bytes, std::mem::size_of::<i32>() as u32)),
        )
    }
}

/// UID of the device audio is currently playing out of.
///
/// The aggregate is pinned to this so the tap follows the real output. A
/// tap built against nothing in particular goes silent the moment someone
/// plugs in headphones, which mid-call is indistinguishable from everyone
/// having stopped talking.
fn default_output_uid() -> Result<Retained<NSString>, String> {
    let device: AudioObjectID = unsafe {
        get_property(
            kAudioObjectSystemObject as AudioObjectID,
            &global_address(kAudioHardwarePropertyDefaultSystemOutputDevice),
            None,
        )?
    };
    if device == 0 {
        return Err("no system output device -- nothing is playing anywhere".into());
    }
    let uid: *const NSString = unsafe {
        get_property(device, &global_address(kAudioDevicePropertyDeviceUID), None)?
    };
    NonNull::new(uid as *mut NSString)
        .map(|p| unsafe { Retained::from_raw(p.as_ptr()).unwrap() })
        .ok_or_else(|| "output device has no UID".to_string())
}

/// Whether anything inside `bundle_path` is using the microphone.
///
/// **Not the app's own pid.** Chromium browsers hold the microphone in a
/// HELPER process -- verified live during a Google Meet call on
/// 2026-09-18, where the input was held by
/// `Arc.app/Contents/Frameworks/ArcCore.framework/Helpers/Browser
/// Helper.app/.../Browser Helper` while Arc's own process held nothing.
/// `NSWorkspace` lists applications and never lists that helper, so
/// asking about the app's pid returned false for an entire live call and
/// the liveness signal built on it never fired once.
///
/// Core Audio lists PROCESSES, so the helper is right there -- it just has
/// to be matched by where it lives rather than by which app it is.
pub fn bundle_is_using_microphone(bundle_path: &str, own_bundle: &str) -> bool {
    if bundle_path.is_empty() {
        return false;
    }
    // This app holds the microphone itself while taking notes. Counting
    // that would make this answer "a meeting is live" purely because notes
    // are running -- true forever, and useless.
    //
    // Excluded by BUNDLE rather than by pid, for the same reason the
    // positive match is by bundle: a helper process is a different pid but
    // the same application, and the pid-based version of this check is
    // precisely what missed Arc's helper for a whole call.
    if !own_bundle.is_empty() && bundle_path.starts_with(own_bundle) {
        return false;
    }
    for (pid, running_input) in all_audio_processes() {
        if !running_input {
            continue;
        }
        let Some(path) = executable_path(pid) else { continue };
        if !own_bundle.is_empty() && path.starts_with(own_bundle) {
            continue;
        }
        if path.starts_with(bundle_path) {
            return true;
        }
    }
    false
}

/// The executable behind a pid.
///
/// `proc_pidpath` rather than spawning `ps`: this runs from the capture
/// loop, and a process spawn there would be paid for every check forever.
fn executable_path(pid: i32) -> Option<String> {
    const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;
    let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    let len = unsafe {
        proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, PROC_PIDPATHINFO_MAXSIZE as u32)
    };
    if len <= 0 {
        return None;
    }
    buf.truncate(len as usize);
    String::from_utf8(buf).ok()
}

extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
}

/// Every Core Audio process object, with whether it is running input.
///
/// Diagnostic. `NSWorkspace` lists applications; Core Audio lists
/// PROCESSES, and a Chromium browser holds the microphone in a helper
/// process that is not an application at all.
pub fn all_audio_processes() -> Vec<(i32, bool)> {
    let address = global_address(kAudioHardwarePropertyProcessObjectList);
    let mut size: u32 = 0;
    let status = unsafe {
        objc2_core_audio::AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    let mut objects: Vec<AudioObjectID> = vec![0; count];
    if unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(objects.as_mut_ptr() as *mut c_void).unwrap(),
        )
    } != 0
    {
        return Vec::new();
    }
    objects
        .into_iter()
        .filter_map(|o| {
            let pid: i32 =
                unsafe { get_property(o, &global_address(kAudioProcessPropertyPID), None) }.ok()?;
            let running: u32 = unsafe {
                get_property(o, &global_address(kAudioProcessPropertyIsRunningInput), None)
            }
            .unwrap_or(0);
            Some((pid, running != 0))
        })
        .collect()
}

/// Where tapped audio waits to be picked up by a transcriber.
///
/// **This is the point at which meeting audio stops being thrown away.**
/// Until transcription existed, every buffer was reduced to a peak and
/// dropped; a sink means samples now live, briefly, in memory. They are
/// still never written to disk here -- the drain hands them straight to
/// the on-device recogniser -- but the promise is narrower than it was and
/// the comment should say so rather than quietly going stale.
///
/// Fixed capacity, filled under `try_lock`. The IOProc is a realtime
/// thread: it must never wait on a lock or allocate, so a contended sink
/// drops its buffer and counts the loss instead of blocking the audio the
/// reader is actually listening to. Dropping a few milliseconds of speech
/// costs a word; stalling the IO thread glitches the call itself.
#[derive(Default)]
pub struct AudioSink {
    buffer: std::sync::Mutex<Vec<f32>>,
    /// Buffers lost to contention or overflow. Surfaced rather than hidden
    /// so a transcript with holes in it has a visible cause.
    pub dropped: std::sync::atomic::AtomicU64,
}

/// Roughly eight seconds at 48 kHz mono -- far more than the drain's
/// cadence needs, so overflow means the drain has actually stopped rather
/// than merely fallen behind.
const SINK_CAPACITY: usize = 48_000 * 8;

impl AudioSink {
    pub fn new() -> Self {
        Self {
            buffer: std::sync::Mutex::new(Vec::with_capacity(SINK_CAPACITY)),
            dropped: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Takes everything waiting, leaving the buffer empty and its capacity
    /// intact so the IOProc never has to allocate.
    pub fn drain(&self) -> Vec<f32> {
        let mut held = self.buffer.lock().unwrap();
        std::mem::replace(&mut held, Vec::with_capacity(SINK_CAPACITY))
    }
}

/// A live tap. Dropping it tears the whole chain down.
pub struct SystemAudioTap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    /// Kept alive for exactly as long as the IOProc that calls it.
    _block: RcBlock<
        dyn Fn(
            NonNull<objc2_core_audio_types::AudioTimeStamp>,
            NonNull<objc2_core_audio_types::AudioBufferList>,
            NonNull<objc2_core_audio_types::AudioTimeStamp>,
            NonNull<objc2_core_audio_types::AudioBufferList>,
            NonNull<objc2_core_audio_types::AudioTimeStamp>,
        ),
    >,
}

// The Core Audio object IDs are plain integers and the block is retained
// for the lifetime of the struct; the IOProc runs on its own queue and
// only touches the caller's `Arc`.
unsafe impl Send for SystemAudioTap {}
unsafe impl Sync for SystemAudioTap {}

impl SystemAudioTap {
    /// Starts tapping everything the Mac is playing except Jelly itself.
    ///
    /// `on_peak` is called from Core Audio's IO thread once per buffer
    /// with that buffer's peak amplitude. It must not block: this is a
    /// realtime thread, and stalling it glitches the audio the reader is
    /// actually listening to.
    /// As `start`, but also keeps the audio for a transcriber to collect.
    pub fn start_with_sink(
        on_peak: Arc<dyn Fn(f32) + Send + Sync>,
        sink: Option<Arc<AudioSink>>,
    ) -> Result<Self, String> {
        let own_pid = std::process::id() as i32;
        let own_object = process_object_for_pid(own_pid)
            .unwrap_or(0); // Not fatal: worst case Jelly hears itself.
        let output_uid = default_output_uid()?;

        // -- 1. the tap ----------------------------------------------------
        let excluded = if own_object != 0 {
            NSArray::from_retained_slice(&[NSNumber::new_u32(own_object)])
        } else {
            NSArray::from_retained_slice(&[])
        };
        let description = unsafe {
            let d = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            );
            d.setName(&NSString::from_str("Jelly Voice and Notes"));
            // Private: the tap is ours alone and must never show up in the
            // reader's sound settings as a device they could pick.
            d.setPrivate(true);
            d
        };
        let mut tap: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) };
        if status != 0 || tap == 0 {
            // -4 is kAudio_UnimplementedError, which is what a denied or
            // never-granted System Audio Recording consent looks like from
            // in here -- there is no separate "denied" code.
            eprintln!("[system-audio] could not create the tap ({status})");
            return Err("This app may not hear what your Mac plays yet. Allow it in System Settings, Privacy and Security, System Audio Recording.".into());
        }

        // -- 2. the aggregate device ---------------------------------------
        let tap_uid: Retained<NSString> = unsafe {
            let raw: *const NSString =
                get_property(tap, &global_address(K_AUDIO_TAP_PROPERTY_UID), None)
                    .map_err(|e| {
                        AudioHardwareDestroyProcessTap(tap);
                        e
                    })?;
            match NonNull::new(raw as *mut NSString) {
                Some(p) => Retained::from_raw(p.as_ptr()).unwrap(),
                None => {
                    AudioHardwareDestroyProcessTap(tap);
                    return Err("the tap has no UID".into());
                }
            }
        };

        let sub_tap = NSDictionary::from_slices(
            &[
                &*key(kAudioSubTapUIDKey),
                &*key(kAudioSubTapDriftCompensationKey),
            ],
            &[&*tap_uid as &objc2::runtime::AnyObject, &*NSNumber::new_bool(true)],
        );
        let aggregate_description = NSDictionary::from_slices(
            &[
                &*key(kAudioAggregateDeviceNameKey),
                &*key(kAudioAggregateDeviceUIDKey),
                &*key(kAudioAggregateDeviceMainSubDeviceKey),
                &*key(kAudioAggregateDeviceIsPrivateKey),
                &*key(kAudioAggregateDeviceTapListKey),
            ],
            &[
                &*NSString::from_str("Jelly Voice and Notes") as &objc2::runtime::AnyObject,
                &*NSString::from_str(&format!("app.jellyvoicenotes.tap.{}", NSUUID::new().UUIDString())),
                &*output_uid,
                &*NSNumber::new_bool(true),
                &*NSArray::from_retained_slice(&[sub_tap]),
            ],
        );

        let mut aggregate: AudioObjectID = 0;
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(
                // NSDictionary is toll-free bridged to CFDictionary.
                &*(Retained::as_ptr(&aggregate_description)
                    as *const objc2_core_foundation::CFDictionary),
                NonNull::from(&mut aggregate),
            )
        };
        if status != 0 || aggregate == 0 {
            unsafe { AudioHardwareDestroyProcessTap(tap) };
            return Err(format!("could not create the aggregate device ({status})"));
        }

        // -- 3. the IOProc -------------------------------------------------
        let block = RcBlock::new(
            move |_now: NonNull<objc2_core_audio_types::AudioTimeStamp>,
                  input: NonNull<objc2_core_audio_types::AudioBufferList>,
                  _input_time: NonNull<objc2_core_audio_types::AudioTimeStamp>,
                  _output: NonNull<objc2_core_audio_types::AudioBufferList>,
                  _output_time: NonNull<objc2_core_audio_types::AudioTimeStamp>| {
                // Realtime thread. No allocation, no waiting on a lock:
                // a sink that is busy loses this buffer rather than
                // stalling the audio the reader is listening to.
                let peak = match sink.as_deref() {
                    Some(s) => match s.buffer.try_lock() {
                        Ok(mut held) => {
                            if held.len() + input_frames(&input) > SINK_CAPACITY {
                                s.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                unsafe { peak_of(input.as_ref(), None) }
                            } else {
                                unsafe { peak_of(input.as_ref(), Some(&mut held)) }
                            }
                        }
                        Err(_) => {
                            s.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            unsafe { peak_of(input.as_ref(), None) }
                        }
                    },
                    None => unsafe { peak_of(input.as_ref(), None) },
                };
                on_peak(peak);
            },
        );

        let mut proc_id: AudioDeviceIOProcID = None;
        let status = unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut proc_id),
                aggregate,
                None, // None = Core Audio's own realtime thread.
                RcBlock::as_ptr(&block),
            )
        };
        if status != 0 {
            unsafe {
                AudioHardwareDestroyAggregateDevice(aggregate);
                AudioHardwareDestroyProcessTap(tap);
            }
            return Err(format!("could not attach to the aggregate device ({status})"));
        }

        let status = unsafe { AudioDeviceStart(aggregate, proc_id) };
        if status != 0 {
            unsafe {
                AudioDeviceDestroyIOProcID(aggregate, proc_id);
                AudioHardwareDestroyAggregateDevice(aggregate);
                AudioHardwareDestroyProcessTap(tap);
            }
            return Err(format!("could not start the tap ({status})"));
        }

        Ok(Self { tap, aggregate, proc_id, _block: block })
    }

    /// The format the tap is delivering. Read rather than assumed, because
    /// a wrong guess here reads as silence rather than as an error.
    pub fn format(
        &self,
    ) -> Result<objc2_core_audio_types::AudioStreamBasicDescription, String> {
        unsafe { get_property(self.tap, &global_address(K_AUDIO_TAP_PROPERTY_FORMAT), None) }
    }
}

impl Drop for SystemAudioTap {
    /// Torn down in the exact reverse of construction. An aggregate device
    /// left behind outlives the process and shows up in the reader's sound
    /// settings as a stray input, so this is not merely tidiness.
    fn drop(&mut self) {
        unsafe {
            AudioDeviceStop(self.aggregate, self.proc_id);
            AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
            AudioHardwareDestroyAggregateDevice(self.aggregate);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

/// Frames waiting in a buffer list, for the overflow check. Counts bytes
/// rather than trusting a channel count that may be zero.
fn input_frames(list: &NonNull<objc2_core_audio_types::AudioBufferList>) -> usize {
    let list = unsafe { list.as_ref() };
    let count = list.mNumberBuffers as usize;
    let buffers = list.mBuffers.as_ptr();
    (0..count)
        .map(|i| {
            let b = unsafe { &*buffers.add(i) };
            b.mDataByteSize as usize / std::mem::size_of::<f32>() / b.mNumberChannels.max(1) as usize
        })
        .sum()
}

/// Largest absolute sample in a buffer list, and the audio downmixed to
/// mono for whoever is listening.
///
/// The tap delivers interleaved stereo (`kAudioFormatFlagIsNonInterleaved`
/// is not set in the format it reports), so the two channels alternate
/// within one buffer and the downmix is a pairwise average.
///
/// # Safety
/// `list` must be a buffer list Core Audio just handed us, carrying 32-bit
/// float PCM -- which is what a tap always delivers.
unsafe fn peak_of(
    list: &objc2_core_audio_types::AudioBufferList,
    mut mono: Option<&mut Vec<f32>>,
) -> f32 {
    let mut peak = 0.0f32;
    let count = list.mNumberBuffers as usize;
    // `mBuffers` is a C flexible array member: declared as length 1, and
    // actually `mNumberBuffers` long.
    let buffers = list.mBuffers.as_ptr();
    for i in 0..count {
        let buffer = unsafe { &*buffers.add(i) };
        if buffer.mData.is_null() {
            continue;
        }
        let samples = buffer.mDataByteSize as usize / std::mem::size_of::<f32>();
        let data = buffer.mData as *const f32;
        let channels = buffer.mNumberChannels.max(1) as usize;
        let mut frame_sum = 0.0f32;
        for s in 0..samples {
            let v = unsafe { *data.add(s) };
            // Guards against NaN, which would poison the meter forever.
            if !v.is_finite() {
                continue;
            }
            peak = peak.max(v.abs());
            if let Some(out) = mono.as_deref_mut() {
                frame_sum += v;
                if s % channels == channels - 1 {
                    out.push(frame_sum / channels as f32);
                    frame_sum = 0.0;
                }
            }
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four-char code has to be byte-exact or the UID read silently
    /// fetches the wrong property.
    #[test]
    fn the_tap_uid_selector_is_the_right_four_char_code() {
        assert_eq!(K_AUDIO_TAP_PROPERTY_UID, 0x74756964, "'tuid'");
    }

    /// Jelly must be able to find its own process object, or it cannot
    /// exclude itself and will hear its own voice overlay as a
    /// participant.
    #[test]
    fn jelly_can_find_its_own_audio_process_object() {
        let own = process_object_for_pid(std::process::id() as i32);
        // A process that has never touched audio may legitimately have no
        // object yet, so this asserts the call works, not that it is
        // non-zero.
        assert!(own.is_ok(), "could not translate our own pid: {own:?}");
    }

    /// Proves the tap receives audio rather than starting cleanly and
    /// hearing nothing, which is what a permission problem looks like from
    /// the inside.
    ///
    /// Play something audible, then:
    /// `cargo test --lib live_core_audio_tap -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_core_audio_tap() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let frames = Arc::new(AtomicU64::new(0));
        let loudest = Arc::new(AtomicU64::new(0));
        let (f, l) = (frames.clone(), loudest.clone());
        let tap = match SystemAudioTap::start_with_sink(Arc::new(move |peak: f32| {
            f.fetch_add(1, Ordering::Relaxed);
            l.fetch_max((peak * 10_000.0) as u64, Ordering::Relaxed);
        }), None) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  COULD NOT START: {e}");
                return;
            }
        };
        // A peak of exactly zero can mean silence, a denied permission
        // (Core Audio hands back a working tap full of zeroes rather than
        // an error), or a format this code is misreading. Printing the
        // format and the buffer shape is what tells those three apart.
        match tap.format() {
            Ok(f) => eprintln!(
                "  tap format: {:.0} Hz, {} ch, {} bits, flags={:#x}, id={:?}",
                f.mSampleRate,
                f.mChannelsPerFrame,
                f.mBitsPerChannel,
                f.mFormatFlags,
                std::str::from_utf8(&f.mFormatID.to_be_bytes()).unwrap_or("?")
            ),
            Err(e) => eprintln!("  tap format unavailable: {e}"),
        }
        eprintln!("  tapping for 6 seconds -- play some audio");
        for i in 0..6 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            eprintln!(
                "  {}s  buffers={:<6} peak={:.3}",
                i + 1,
                frames.load(Ordering::Relaxed),
                loudest.load(Ordering::Relaxed) as f32 / 10_000.0
            );
        }
        drop(tap);
        let buffers = frames.load(Ordering::Relaxed);
        let peak = loudest.load(Ordering::Relaxed) as f32 / 10_000.0;
        assert!(buffers > 0, "no buffers at all -- the IOProc never ran");
        // Buffers arriving is NOT proof the tap works. A denied System
        // Audio Recording consent produces a perfectly healthy stream of
        // zeroes -- no error, no silence detection, just nothing. Counting
        // buffers would pass in exactly the case this test exists to
        // catch, so the assertion has to be on the samples.
        assert!(
            peak > 0.0,
            "{buffers} buffers arrived but every sample was zero. That is what a denied \
             System Audio Recording permission looks like from in here -- or the audio \
             really was silent. Check System Settings > Privacy & Security > System Audio \
             Recording. Note a bare `cargo test` binary has no Info.plist, so macOS never \
             prompts and denies silently; this test only means something from the signed \
             app bundle."
        );
    }
}
