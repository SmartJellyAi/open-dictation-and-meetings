//! What the pages can ask of the app. Anything that touches the database
//! or the network runs off the main thread, so a click never waits.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::meeting::{MeetingOffer, MeetingState, Notetaker};
use crate::notes::{self, MeetingNote};
use crate::prep::{self, MeetingPrep};
use crate::settings::{self, SettingsView};
use crate::store::{EventRow, Store};
use crate::transcript::TranscriptUpdate;

type Db<'a> = State<'a, Arc<Store>>;

/// Runs blocking work (a request, a file) off the async pool.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "That did not finish. Try again.".to_string())?
}

// ---------------------------------------------------------------- settings

#[tauri::command(async)]
pub fn get_settings(store: Db) -> SettingsView {
    settings::view(&store)
}

/// A switch or a text setting. Keys and the dictation key have their own
/// commands, because they are checked first.
#[tauri::command(async)]
pub fn set_setting(app: AppHandle, store: Db, key: String, value: serde_json::Value) -> Result<SettingsView, String> {
    const SWITCHES: &[&str] = &[
        settings::DICTATION_ON,
        settings::DICTATION_CLEANUP,
        settings::NOTETAKER_ON,
        settings::MEETING_DETECTION,
        settings::PRE_MEETING_NOTES,
        settings::WEB_RESEARCH,
    ];
    const TEXTS: &[&str] = &[settings::MIC_DEVICE, settings::DICTATION_MODE, settings::YOUR_NAME, settings::ABOUT_YOU];
    if SWITCHES.contains(&key.as_str()) {
        let on = value.as_bool().ok_or("Expected on or off.")?;
        store.set_bool(&key, on);
        if key == settings::DICTATION_ON {
            if on {
                crate::hotkey::apply_saved(&app);
            } else {
                crate::hotkey::clear(&app);
            }
        }
        if key == settings::NOTETAKER_ON && !on {
            app.state::<Notetaker>().decline(&app);
        }
    } else if TEXTS.contains(&key.as_str()) {
        let text = value.as_str().ok_or("Expected text.")?;
        if key == settings::DICTATION_MODE && text != "toggle" && text != "hold" {
            return Err("Pick press to start and stop, or hold to talk.".into());
        }
        store.set(&key, text);
    } else {
        return Err("That setting does not exist.".into());
    }
    Ok(settings::view(&store))
}

/// Saves a key ("openai" or "deepgram"); an empty one removes it.
#[tauri::command(async)]
pub fn set_api_key(store: Db, provider: String, key: String) -> Result<SettingsView, String> {
    let name = match provider.as_str() {
        "openai" => settings::OPENAI_KEY,
        "deepgram" => settings::DEEPGRAM_KEY,
        _ => return Err("Unknown provider.".into()),
    };
    store.set(name, key.trim());
    Ok(settings::view(&store))
}

/// Checks a saved key against its provider.
#[tauri::command]
pub async fn check_api_key(app: AppHandle, provider: String) -> Result<(), String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>();
        match provider.as_str() {
            "openai" => {
                let key = settings::key(&store, settings::OPENAI_KEY).ok_or("No OpenAI key is saved.")?;
                crate::openai::check_key(&key)
            }
            "deepgram" => {
                let key = settings::key(&store, settings::DEEPGRAM_KEY).ok_or("No Deepgram key is saved.")?;
                check_deepgram(&key)
            }
            _ => Err("Unknown provider.".into()),
        }
    })
    .await
}

fn check_deepgram(key: &str) -> Result<(), String> {
    let key = key.to_string();
    crate::rt::block_on(async move {
        let resp = reqwest::Client::new()
            .get("https://api.deepgram.com/v1/projects")
            .header("Authorization", format!("Token {key}"))
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| "Could not reach Deepgram. Check your internet connection.".to_string())?;
        match resp.status().as_u16() {
            200..=299 => Ok(()),
            401 | 403 => Err("Your Deepgram key was not accepted. Check it and save it again.".into()),
            _ => Err("Deepgram did not answer just now. Try again in a minute.".into()),
        }
    })
}

#[tauri::command]
pub async fn list_microphones() -> Result<Vec<String>, String> {
    blocking(|| Ok(crate::audio::input_devices())).await
}

#[tauri::command(async)]
pub fn set_dictation_key(app: AppHandle, store: Db, key: String) -> Result<SettingsView, String> {
    crate::hotkey::parse(&key)?;
    if store.get_bool(settings::DICTATION_ON, true) {
        crate::hotkey::apply(&app, &key)?;
    }
    store.set(settings::DICTATION_KEY, key.trim());
    Ok(settings::view(&store))
}

/// Saves the calendar address after reading it once; an empty one removes
/// the calendar and its meetings.
#[tauri::command]
pub async fn set_calendar_url(app: AppHandle, url: String) -> Result<usize, String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>().inner().clone();
        let url = url.trim().to_string();
        if url.is_empty() {
            store.set(settings::CALENDAR_URL, "");
            store.replace_calendar_events(&[]);
            let _ = app.emit("meetings-upcoming-changed", ());
            return Ok(0);
        }
        let text = crate::ics::fetch(&url)?;
        let events = crate::ics::parse(&text, chrono::Utc::now().timestamp_millis() - 3_600_000);
        store.set(settings::CALENDAR_URL, &url);
        store.replace_calendar_events(&events);
        store.set("calendar_read_at", &chrono::Utc::now().timestamp_millis().to_string());
        let _ = app.emit("meetings-upcoming-changed", ());
        Ok(events.len())
    })
    .await
}

#[tauri::command]
pub fn request_accessibility() -> bool {
    crate::platform::request_accessibility()
}

#[tauri::command]
pub fn open_privacy_settings(pane: String) -> bool {
    crate::platform::open_privacy_settings(&pane)
}

// --------------------------------------------------------------- meetings

#[tauri::command(async)]
pub fn list_meeting_notes(store: Db) -> Vec<MeetingNote> {
    let your_name = store.get_or(settings::YOUR_NAME, "");
    store.list_meetings(500).iter().map(|m| notes::view(m, &your_name)).collect()
}

#[tauri::command]
pub fn get_meeting_state(notetaker: State<Notetaker>) -> MeetingState {
    notetaker.state()
}

#[tauri::command]
pub fn get_live_transcript(notetaker: State<Notetaker>) -> TranscriptUpdate {
    notetaker.transcript.snapshot()
}

#[tauri::command]
pub fn get_meeting_offer(notetaker: State<Notetaker>) -> Option<MeetingOffer> {
    notetaker.current_offer().map(|key| MeetingOffer { service: key.split(':').next().unwrap_or("a meeting").to_string(), key })
}

#[tauri::command(async)]
pub fn start_meeting_notes(app: AppHandle) -> Result<String, String> {
    app.state::<Notetaker>().start(&app, "Meeting", None, true, None)
}

#[tauri::command(async)]
pub fn stop_meeting_notes(app: AppHandle) {
    app.state::<Notetaker>().stop(&app);
}

#[tauri::command(async)]
pub fn accept_meeting_offer(app: AppHandle) -> Result<String, String> {
    app.state::<Notetaker>().accept_offer(&app)
}

#[tauri::command(async)]
pub fn decline_meeting_offer(app: AppHandle) {
    app.state::<Notetaker>().decline(&app);
}

/// Your own notes on a call, saved as you type.
#[tauri::command(async)]
pub fn save_meeting_my_notes(store: Db, id: String, text: String) -> bool {
    store.set_meeting_my_notes(&id, &text)
}

#[tauri::command]
pub async fn summarise_meeting(app: AppHandle, id: String) -> Result<String, String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>().inner().clone();
        let out = notes::summarise(&store, &id);
        let _ = app.emit("meeting-notes-changed", ());
        out
    })
    .await
}

#[tauri::command(async)]
pub fn rename_meeting(app: AppHandle, store: Db, id: String, title: String) -> bool {
    let ok = store.set_meeting_title(&id, &title);
    let _ = app.emit("meeting-notes-changed", ());
    ok
}

#[tauri::command(async)]
pub fn set_meeting_people(app: AppHandle, store: Db, id: String, people: Vec<String>) -> bool {
    let people: Vec<String> = people.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    let ok = store.set_meeting_participants(&id, &people);
    let _ = app.emit("meeting-notes-changed", ());
    ok
}

#[tauri::command(async)]
pub fn delete_meeting(app: AppHandle, store: Db, id: String) -> bool {
    let ok = store.delete_meeting(&id);
    let _ = app.emit("meeting-notes-changed", ());
    ok
}

fn note_by_id(store: &Store, id: &str) -> Result<MeetingNote, String> {
    let row = store.meeting(id).ok_or_else(|| "That meeting is gone.".to_string())?;
    Ok(notes::view(&row, &store.get_or(settings::YOUR_NAME, "")))
}

/// The notes as Markdown, written first when there is a transcript but
/// no notes yet.
fn markdown_written(app: &AppHandle, id: &str) -> Result<(MeetingNote, String), String> {
    let store = app.state::<Arc<Store>>().inner().clone();
    let mut note = note_by_id(&store, id)?;
    if !notes::has_notes(&note) {
        if note.transcript.trim().is_empty() {
            return Err("There are no notes for this meeting yet, and nothing was heard to write them from.".into());
        }
        notes::summarise(&store, id)?;
        let _ = app.emit("meeting-notes-changed", ());
        note = note_by_id(&store, id)?;
    }
    let md = notes::markdown(&note);
    Ok((note, md))
}

#[tauri::command]
pub async fn meeting_markdown(app: AppHandle, id: String) -> Result<String, String> {
    blocking(move || markdown_written(&app, &id).map(|(_, md)| md)).await
}

/// One meeting to a file in Downloads, shown in the file manager. `what`
/// is "notes" or "transcript".
#[tauri::command]
pub async fn export_meeting(app: AppHandle, id: String, what: String) -> Result<String, String> {
    blocking(move || {
        let (name, body) = if what == "transcript" {
            let store = app.state::<Arc<Store>>().inner().clone();
            let note = note_by_id(&store, &id)?;
            if note.transcript.trim().is_empty() {
                return Err("Nothing was transcribed for this meeting.".into());
            }
            (notes::filename(&note, "txt").replace(".txt", " transcript.txt"), notes::transcript_text(&note))
        } else {
            let (note, md) = markdown_written(&app, &id)?;
            (notes::filename(&note, "md"), md)
        };
        let path = notes::write_file(&name, &body)?;
        let _ = tauri_plugin_opener::reveal_item_in_dir(&path);
        Ok(path.to_string_lossy().to_string())
    })
    .await
}

#[derive(serde::Serialize)]
pub struct ExportReport {
    pub count: usize,
    /// Calls skipped for having no notes yet.
    pub skipped: usize,
    pub folder: String,
}

/// Every call that has notes, each to its own file, then the folder opens.
#[tauri::command]
pub async fn export_all_meetings(app: AppHandle) -> Result<ExportReport, String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>().inner().clone();
        let your_name = store.get_or(settings::YOUR_NAME, "");
        let (mut count, mut skipped) = (0, 0);
        for row in store.list_meetings(100_000) {
            let note = notes::view(&row, &your_name);
            if !notes::has_notes(&note) {
                skipped += 1;
                continue;
            }
            if notes::write_file(&notes::filename(&note, "md"), &notes::markdown(&note)).is_ok() {
                count += 1;
            }
        }
        let folder = crate::paths::export_dir();
        let _ = std::fs::create_dir_all(&folder);
        let _ = tauri_plugin_opener::open_path(folder.to_string_lossy().to_string(), None::<&str>);
        Ok(ExportReport { count, skipped, folder: folder.to_string_lossy().to_string() })
    })
    .await
}

// -------------------------------------------------------------------- prep

#[derive(serde::Serialize)]
pub struct UpcomingMeeting {
    pub event: EventRow,
    pub prep: Option<MeetingPrep>,
}

#[tauri::command(async)]
pub fn upcoming_meetings(store: Db) -> Vec<UpcomingMeeting> {
    let now = chrono::Utc::now().timestamp_millis();
    store.upcoming_events(now, 40).into_iter().map(|e| UpcomingMeeting { prep: prep::load(&store, &e.id), event: e }).collect()
}

/// A meeting typed in by hand, for when there is no calendar.
#[tauri::command(async)]
pub fn add_meeting(app: AppHandle, store: Db, title: String, starts_at: i64, minutes: Option<i64>, attendees: Vec<String>, description: String, link: Option<String>) -> Result<EventRow, String> {
    if title.trim().is_empty() {
        return Err("Give the meeting a name.".into());
    }
    let event = EventRow {
        id: format!("manual:{}", uuid::Uuid::new_v4()),
        title: title.trim().to_string(),
        starts_at,
        ends_at: Some(starts_at + minutes.unwrap_or(30).max(5) * 60_000),
        location: String::new(),
        description: description.trim().to_string(),
        attendees: attendees.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
        link: link.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).or_else(|| crate::ics::call_link(&description)),
        source: "manual".into(),
    };
    store.upsert_event(&event);
    let _ = app.emit("meetings-upcoming-changed", ());
    Ok(event)
}

#[tauri::command(async)]
pub fn delete_upcoming_meeting(app: AppHandle, store: Db, id: String) -> bool {
    let ok = store.delete_event(&id);
    let _ = app.emit("meetings-upcoming-changed", ());
    ok
}

#[tauri::command]
pub async fn refresh_calendar(app: AppHandle) -> Result<usize, String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>().inner().clone();
        let n = prep::refresh_calendar(&store)?;
        let _ = app.emit("meetings-upcoming-changed", ());
        Ok(n)
    })
    .await
}

/// Runs one prep change off the pool and tells the pages.
async fn prep_change(app: AppHandle, id: String, f: impl FnOnce(&Store, &str) -> Result<MeetingPrep, String> + Send + 'static) -> Result<MeetingPrep, String> {
    blocking(move || {
        let store = app.state::<Arc<Store>>().inner().clone();
        let out = f(&store, &id);
        let _ = app.emit("meeting-prep-changed", id.clone());
        out
    })
    .await
}

#[tauri::command]
pub async fn build_meeting_prep(app: AppHandle, event_id: String, fresh: bool) -> Result<MeetingPrep, String> {
    prep_change(app, event_id, move |s, id| prep::build(s, id, fresh)).await
}

#[tauri::command]
pub async fn answer_prep_question(app: AppHandle, event_id: String, index: usize, answer: Option<String>) -> Result<MeetingPrep, String> {
    prep_change(app, event_id, move |s, id| prep::answer(s, id, index, answer)).await
}

#[tauri::command]
pub async fn drop_prep_point(app: AppHandle, event_id: String, index: usize) -> Result<MeetingPrep, String> {
    prep_change(app, event_id, move |s, id| prep::drop_prediction(s, id, index)).await
}

#[tauri::command]
pub async fn add_prep_point(app: AppHandle, event_id: String, point: String) -> Result<MeetingPrep, String> {
    prep_change(app, event_id, move |s, id| prep::add_prediction(s, id, &point)).await
}

#[tauri::command]
pub async fn set_prep_people(app: AppHandle, event_id: String, people: Vec<String>) -> Result<MeetingPrep, String> {
    prep_change(app, event_id, move |s, id| prep::set_participants(s, id, people)).await
}

// --------------------------------------------------------------- dictation

#[tauri::command(async)]
pub fn toggle_dictation(app: AppHandle) {
    app.state::<crate::dictation::Dictation>().toggle(&app);
}

// ---------------------------------------------------------------- windows

/// Brings the main window forward, on a page ("meetings", "prep",
/// "settings").
#[tauri::command]
pub fn show_main(app: AppHandle, view: Option<String>) {
    crate::show_main(&app, view.as_deref());
}
