//! The notetaker: hears both sides of a call, writes the transcript as it
//! goes, and writes the notes when the call ends.
//!
//! Your microphone is "You"; what the computer plays is "Others". Each
//! side goes to its own live stream (Deepgram) when there is a key for it,
//! else to OpenAI in pieces. A dropped stream is reopened up to three
//! times, then that side carries on in pieces. No audio is written to
//! disk; only the transcript is kept, for a call you said yes to.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::chunked::{has_speech, Chunker};
use crate::deepgram::LiveStream;
use crate::settings;
use crate::store::{MeetingRow, Store};
use crate::transcript::{log, wav_from_samples, Speaker, Transcript};

/// How many times a dropped live stream is reopened before that side
/// falls back to pieces for good. A reset mid-call is ordinary (a Wi-Fi
/// blip); three tries cover a blip without hammering an outage.
const STREAM_REOPEN_ATTEMPTS: u32 = 3;
const STREAM_REOPEN_PAUSE: Duration = Duration::from_millis(900);

/// How often the transcript is written to the database while a call runs.
const CHECKPOINT_EVERY: Duration = Duration::from_secs(5);

/// A call that was noticed on screen ends this long after its window was
/// last seen, once nobody has spoken for a minute; and in any case after
/// fifteen minutes unseen.
const UNSEEN_AND_SILENT: Duration = Duration::from_secs(120);
const SILENT_FOR_MS: u64 = 60_000;
const UNSEEN_BACKSTOP: Duration = Duration::from_secs(15 * 60);
/// Notes started by hand have no window to watch; silence is the only sign
/// the call is over, and the bar is high because demos have quiet spells.
const BY_HAND_SILENT_MS: u64 = 10 * 60_000;

/// A "Not now" holds for the same call for this long.
const DECLINE_HOLDS: Duration = Duration::from_secs(3 * 3600);

/// What the page and the pill are told about the notetaker.
#[derive(Clone, serde::Serialize)]
pub struct MeetingState {
    pub listening: bool,
    pub service: Option<String>,
    pub id: Option<String>,
    /// Said plainly when one side cannot be heard or the stream fell back.
    pub warning: Option<String>,
}

/// A call noticed on screen, offered in the pill.
#[derive(Clone, serde::Serialize)]
pub struct MeetingOffer {
    pub key: String,
    pub service: String,
}

struct Session {
    id: String,
    service: String,
    by_hand: bool,
    last_seen: Instant,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

pub struct Notetaker {
    session: Mutex<Option<Session>>,
    pub transcript: Arc<Transcript>,
    warning: Arc<Mutex<Option<String>>>,
    declined: Mutex<HashMap<String, Instant>>,
    offered: Mutex<Option<String>>,
}

impl Default for Notetaker {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
            transcript: Arc::new(Transcript::default()),
            warning: Arc::new(Mutex::new(None)),
            declined: Mutex::new(HashMap::new()),
            offered: Mutex::new(None),
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Notetaker {
    pub fn state(&self) -> MeetingState {
        let s = self.session.lock().unwrap();
        MeetingState {
            listening: s.is_some(),
            service: s.as_ref().map(|s| s.service.clone()),
            id: s.as_ref().map(|s| s.id.clone()),
            warning: self.warning.lock().unwrap().clone(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    /// Starts taking notes. `service` is where the call is ("Zoom"), or
    /// "Meeting" by hand; `code` the call's own code when the window
    /// showed one.
    pub fn start(&self, app: &AppHandle, service: &str, code: Option<String>, by_hand: bool, event_name: Option<String>) -> Result<String, String> {
        let store = app.state::<Arc<Store>>();
        if !store.get_bool(settings::NOTETAKER_ON, true) {
            return Err("The meeting notetaker is off. Turn it on in Settings.".into());
        }
        let deepgram = settings::key(&store, settings::DEEPGRAM_KEY);
        let openai = settings::key(&store, settings::OPENAI_KEY);
        if deepgram.is_none() && openai.is_none() {
            return Err("Add a Deepgram or OpenAI key in Settings to take notes.".into());
        }
        let mut slot = self.session.lock().unwrap();
        if let Some(s) = slot.as_ref() {
            return Ok(s.id.clone());
        }
        // A meeting on the calendar right now names the call and its people.
        let now = now_ms();
        let event = store.upcoming_events(now - 3 * 3_600_000, 20).into_iter().find(|e| e.starts_at - 10 * 60_000 <= now && now <= e.ends_at.unwrap_or(e.starts_at + 3_600_000));
        let mut participants: Vec<String> = event.as_ref().map(|e| e.attendees.clone()).unwrap_or_default();
        if participants.is_empty() {
            if let Some(name) = event_name.as_deref() {
                participants = crate::detect::people_in_event_name(name);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        store.insert_meeting(&MeetingRow {
            id: id.clone(),
            service: service.to_string(),
            title: event.as_ref().map(|e| e.title.clone()).unwrap_or_default(),
            participants,
            meeting_code: code,
            started_at: now,
            event_id: event.as_ref().map(|e| e.id.clone()),
            ..Default::default()
        });
        self.transcript.reset();
        *self.warning.lock().unwrap() = None;
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (app, id, stop, transcript, warning) = (app.clone(), id.clone(), stop.clone(), self.transcript.clone(), self.warning.clone());
            let mic = store.get_or(settings::MIC_DEVICE, "");
            std::thread::Builder::new()
                .name("jelly-notetaker".into())
                .spawn(move || run(app, id, stop, transcript, warning, deepgram, openai, mic))
                .map_err(|e| e.to_string())?
        };
        log(&format!("notes started ({service}{})", if by_hand { ", by hand" } else { "" }));
        *slot = Some(Session { id: id.clone(), service: service.to_string(), by_hand, last_seen: Instant::now(), stop, worker: Some(worker) });
        *self.offered.lock().unwrap() = None;
        drop(slot);
        let _ = app.emit("meeting-offer", Option::<MeetingOffer>::None);
        let _ = app.emit("meeting-state", self.state());
        crate::pill::refresh(app);
        Ok(id)
    }

    /// Stops the notes. The worker finishes the last words, saves, and
    /// writes the notes on its own thread; this returns at once.
    pub fn stop(&self, app: &AppHandle) {
        let session = self.session.lock().unwrap().take();
        let Some(mut s) = session else { return };
        s.stop.store(true, Ordering::SeqCst);
        let worker = s.worker.take();
        let app2 = app.clone();
        let id = s.id.clone();
        std::thread::spawn(move || {
            if let Some(w) = worker {
                let _ = w.join();
            }
            finish(&app2, &id);
        });
        log("notes stopped");
        let _ = app.emit("meeting-state", self.state());
        crate::pill::refresh(app);
    }

    /// The pill's "Not now": this call is not offered again for a while.
    pub fn decline(&self, app: &AppHandle) {
        if let Some(key) = self.offered.lock().unwrap().take() {
            self.declined.lock().unwrap().insert(key, Instant::now());
        }
        let _ = app.emit("meeting-offer", Option::<MeetingOffer>::None);
        crate::pill::refresh(app);
    }

    pub fn current_offer(&self) -> Option<String> {
        self.offered.lock().unwrap().clone()
    }

    /// One look at the screen, every couple of seconds: offer notes on a
    /// call that appeared, keep a running call alive while it is seen, and
    /// end one that is gone.
    pub fn tick(&self, app: &AppHandle) {
        let store = app.state::<Arc<Store>>();
        let detecting = store.get_bool(settings::NOTETAKER_ON, true) && store.get_bool(settings::MEETING_DETECTION, true);
        let found = if detecting || self.is_running() { crate::detect::call_in_front() } else { None };
        if let Some(s) = self.session.lock().unwrap().as_mut() {
            if found.is_some() {
                s.last_seen = Instant::now();
            }
        }
        // End a call that is over.
        let end = {
            let s = self.session.lock().unwrap();
            s.as_ref().is_some_and(|s| {
                let silent = self.transcript.silent_for_ms();
                if s.by_hand {
                    silent >= BY_HAND_SILENT_MS
                } else {
                    let unseen = s.last_seen.elapsed();
                    (unseen >= UNSEEN_AND_SILENT && silent >= SILENT_FOR_MS) || unseen >= UNSEEN_BACKSTOP
                }
            })
        };
        if end {
            log("the call looks over; stopping the notes");
            self.stop(app);
            return;
        }
        if self.is_running() || !detecting {
            return;
        }
        // Offer notes on a call that appeared.
        let offer = found.map(|(service, title, by_mic)| {
            let key = if by_mic { format!("{service}:mic") } else { crate::detect::meeting_key(&service, title.as_deref()) };
            (key, service)
        });
        let mut offered = self.offered.lock().unwrap();
        match offer {
            Some((key, service)) => {
                self.declined.lock().unwrap().retain(|_, at| at.elapsed() < DECLINE_HOLDS);
                if self.declined.lock().unwrap().contains_key(&key) || offered.as_deref() == Some(key.as_str()) {
                    return;
                }
                *offered = Some(key.clone());
                drop(offered);
                log(&format!("a call was noticed ({service})"));
                let _ = app.emit("meeting-offer", Some(MeetingOffer { key, service }));
                crate::pill::refresh(app);
            }
            None => {
                if offered.take().is_some() {
                    drop(offered);
                    let _ = app.emit("meeting-offer", Option::<MeetingOffer>::None);
                    crate::pill::refresh(app);
                }
            }
        }
    }

    /// "Take notes" on the pill's offer: the call in front now.
    pub fn accept_offer(&self, app: &AppHandle) -> Result<String, String> {
        let found = crate::detect::call_in_front();
        let (service, code, event_name) = match found {
            Some((service, title, _)) => {
                let code = title.as_deref().and_then(crate::detect::code_in_title);
                let event = title.as_deref().and_then(crate::detect::meet_event_name);
                (service, code, event)
            }
            None => ("Meeting".to_string(), None, None),
        };
        self.start(app, &service, code, false, event_name)
    }
}

/// One side of the call on its way to text.
enum Side {
    Live { stream: LiveStream, attempts: u32 },
    Pieces { chunker: Chunker, tx: mpsc::Sender<Vec<f32>>, worker: std::thread::JoinHandle<()> },
    Off,
}

/// A thread that turns pieces of one side's audio into lines, in order.
fn piece_worker(key: String, who: Speaker, rate: f64, transcript: Arc<Transcript>, warning: Arc<Mutex<Option<String>>>) -> (mpsc::Sender<Vec<f32>>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let handle = std::thread::spawn(move || {
        let mut context = String::new();
        while let Ok(chunk) = rx.recv() {
            if !has_speech(&chunk) {
                continue;
            }
            let Some(wav) = wav_from_samples(&chunk, rate) else { continue };
            match crate::openai::transcribe(&key, wav, None, (!context.is_empty()).then_some(context.as_str())) {
                Ok(text) => {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        transcript.append_final(who, None, &text);
                        context = text;
                    }
                }
                Err(e) => {
                    log(&format!("a piece of the call was not transcribed ({})", who.label()));
                    *warning.lock().unwrap() = Some(e);
                }
            }
        }
    });
    (tx, handle)
}

fn pieces(openai: &Option<String>, who: Speaker, rate: f64, transcript: &Arc<Transcript>, warning: &Arc<Mutex<Option<String>>>) -> Side {
    match openai {
        Some(key) => {
            let (tx, worker) = piece_worker(key.clone(), who, rate, transcript.clone(), warning.clone());
            Side::Pieces { chunker: Chunker::new(rate), tx, worker }
        }
        None => Side::Off,
    }
}

impl Side {
    fn open(deepgram: &Option<String>, openai: &Option<String>, who: Speaker, rate: f64, transcript: &Arc<Transcript>, warning: &Arc<Mutex<Option<String>>>) -> Side {
        match deepgram {
            Some(key) => Side::Live { stream: LiveStream::open(key, who, rate, transcript.clone()), attempts: 0 },
            None => pieces(openai, who, rate, transcript, warning),
        }
    }

    fn push(&mut self, samples: &[f32]) {
        match self {
            Side::Live { stream, .. } => stream.push(samples),
            Side::Pieces { chunker, tx, .. } => {
                chunker.push(samples);
                if let Some(chunk) = chunker.take_chunk(false) {
                    let _ = tx.send(chunk);
                }
            }
            Side::Off => {}
        }
    }

    /// A stream that dropped is reopened, or this side falls back to
    /// pieces, or (no OpenAI key) stops with a warning.
    fn heal(&mut self, deepgram: &Option<String>, openai: &Option<String>, who: Speaker, rate: f64, transcript: &Arc<Transcript>, warning: &Arc<Mutex<Option<String>>>) {
        let Side::Live { stream, attempts } = self else { return };
        if !stream.failed() {
            return;
        }
        let reason = stream.failure().unwrap_or_default();
        if *attempts < STREAM_REOPEN_ATTEMPTS && !reason.contains("not accepted") && !reason.contains("out of credit") {
            *attempts += 1;
            log(&format!("live transcript dropped ({}), reconnecting {}/{}", who.label(), attempts, STREAM_REOPEN_ATTEMPTS));
            std::thread::sleep(STREAM_REOPEN_PAUSE);
            if let Some(key) = deepgram {
                let tries = *attempts;
                *self = Side::Live { stream: LiveStream::open(key, who, rate, transcript.clone()), attempts: tries };
            }
            return;
        }
        let plain = if reason.contains("not accepted") {
            "Your Deepgram key was not accepted. Check it in Settings."
        } else if reason.contains("out of credit") {
            "Your Deepgram account is out of credit."
        } else {
            "The live transcript dropped."
        };
        let next = pieces(openai, who, rate, transcript, warning);
        let said = match next {
            Side::Off => format!("{plain} Add an OpenAI key in Settings so notes carry on if this happens."),
            _ => format!("{plain} Notes carry on, a little behind."),
        };
        *warning.lock().unwrap() = Some(said);
        *self = next;
    }

    /// The call is over: the last words are settled.
    fn finish(self) {
        match self {
            Side::Live { stream, .. } => stream.close(),
            Side::Pieces { chunker, tx, worker } => {
                if let Some(chunk) = chunker.take_chunk(true) {
                    let _ = tx.send(chunk);
                }
                drop(tx);
                let _ = worker.join();
            }
            Side::Off => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(app: AppHandle, id: String, stop: Arc<AtomicBool>, transcript: Arc<Transcript>, warning: Arc<Mutex<Option<String>>>, deepgram: Option<String>, openai: Option<String>, mic: String) {
    let store = app.state::<Arc<Store>>().inner().clone();
    let emit_error = |message: String| {
        *warning.lock().unwrap() = Some(message.clone());
        let _ = app.emit("meeting-transcribe-error", message);
    };

    // You: the microphone.
    let (mic_tx, mic_rx) = mpsc::channel::<Vec<f32>>();
    let mic_capture = crate::audio::Capture::open(crate::audio::Source::Microphone((!mic.is_empty()).then_some(mic))).and_then(|p| {
        let rate = p.sample_rate;
        p.start(move |s| {
            let _ = mic_tx.send(s.to_vec());
        })
        .map(|c| (c, rate))
    });
    // Others: what the computer plays.
    let (sys_tx, sys_rx) = mpsc::channel::<Vec<f32>>();
    let system = crate::system_audio::SystemCapture::start(move |s| {
        let _ = sys_tx.send(s.to_vec());
    });

    let mut me = match &mic_capture {
        Ok((_, rate)) => Side::open(&deepgram, &openai, Speaker::Me, *rate, &transcript, &warning),
        Err(e) => {
            emit_error(format!("{e} Your own voice is not in these notes."));
            Side::Off
        }
    };
    let mut others = match &system {
        Ok(cap) => Side::open(&deepgram, &openai, Speaker::Others, cap.sample_rate, &transcript, &warning),
        Err(e) => {
            *warning.lock().unwrap() = Some(e.clone());
            Side::Off
        }
    };
    let mic_rate = mic_capture.as_ref().map(|(_, r)| *r).unwrap_or(48_000.0);
    let sys_rate = system.as_ref().map(|c| c.sample_rate).unwrap_or(48_000.0);
    if mic_capture.is_err() && system.is_err() {
        emit_error("Nothing could be heard: no microphone and no system audio. Check the permissions in Settings.".into());
    }
    let _ = app.emit("meeting-state", app.state::<Notetaker>().state());

    let mut last_save = Instant::now();
    let mut last_emit = String::new();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
        while let Ok(s) = mic_rx.try_recv() {
            me.push(&s);
        }
        while let Ok(s) = sys_rx.try_recv() {
            others.push(&s);
        }
        me.heal(&deepgram, &openai, Speaker::Me, mic_rate, &transcript, &warning);
        others.heal(&deepgram, &openai, Speaker::Others, sys_rate, &transcript, &warning);
        let snapshot = transcript.snapshot();
        let fingerprint = format!("{}|{}|{}", snapshot.lines.len(), snapshot.partial_me, snapshot.partial_others);
        if fingerprint != last_emit {
            last_emit = fingerprint;
            let _ = app.emit("meeting-transcript", snapshot);
        }
        if last_save.elapsed() >= CHECKPOINT_EVERY {
            last_save = Instant::now();
            store.set_meeting_transcript(&id, &transcript.text(), transcript.dropped.load(Ordering::Relaxed) as i64);
        }
    }
    // The devices close first, so nothing new arrives while the last
    // words settle.
    drop(mic_capture);
    drop(system);
    while let Ok(s) = mic_rx.try_recv() {
        me.push(&s);
    }
    while let Ok(s) = sys_rx.try_recv() {
        others.push(&s);
    }
    let a = std::thread::spawn(move || me.finish());
    others.finish();
    let _ = a.join();
    store.set_meeting_transcript(&id, &transcript.text(), transcript.dropped.load(Ordering::Relaxed) as i64);
    let _ = app.emit("meeting-transcript", transcript.snapshot());
}

/// After the worker is done: the call is closed in the database, and its
/// notes are written when there is enough to write from.
fn finish(app: &AppHandle, id: &str) {
    let store = app.state::<Arc<Store>>().inner().clone();
    store.end_meeting(id, now_ms());
    let _ = app.emit("meeting-notes-changed", ());
    let Some(row) = store.meeting(id) else { return };
    let words = row.transcript.split_whitespace().count();
    if words < crate::notes::AUTO_SUMMARY_MIN_WORDS {
        if row.transcript.trim().is_empty() && row.my_notes.trim().is_empty() {
            // Nothing was heard and nothing typed: not worth a row.
            store.delete_meeting(id);
            let _ = app.emit("meeting-notes-changed", ());
        }
        return;
    }
    if settings::key(&store, settings::OPENAI_KEY).is_none() {
        return;
    }
    let _ = app.emit("meeting-summary-writing", id.to_string());
    match crate::notes::summarise(&store, id) {
        Ok(_) => log("notes written after the call"),
        Err(e) => {
            log("the notes could not be written after the call");
            let _ = app.emit("meeting-summary-error", serde_json::json!({ "id": id, "message": e }));
        }
    }
    let _ = app.emit("meeting-notes-changed", ());
}

/// Starts the screen watch: every two seconds, one look.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("jelly-meeting-watch".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            let notetaker = app.state::<Notetaker>();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notetaker.tick(&app)));
        })
        .ok();
}
