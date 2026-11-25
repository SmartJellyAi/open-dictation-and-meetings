//! Open Dictation & Meetings: dictation into any app, meeting notes (a live
//! transcript and notes written after the call), and notes before a
//! meeting. Standalone: your own API keys, your own local database.

mod audio;
mod chunked;
mod commands;
mod deepgram;
mod detect;
mod dictation;
mod hotkey;
mod ics;
mod meeting;
mod notes;
mod openai;
mod paths;
mod pill;
mod platform;
mod prep;
mod rt;
mod settings;
mod store;
mod summary_md;
mod system_audio;
#[cfg(target_os = "macos")]
mod system_audio_mac;
mod transcript;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager};

/// Brings the main window forward, optionally on one page.
pub fn show_main(app: &AppHandle, view: Option<&str>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(view) = view {
        let _ = app.emit_to("main", "navigate", view.to_string());
    }
}

fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open Open Dictation & Meetings", true, None::<&str>)?;
    let dictate = MenuItem::with_id(app, "dictate", "Start or stop dictation", true, None::<&str>)?;
    let notes = MenuItem::with_id(app, "notes", "Take meeting notes", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop meeting notes", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&open, &sep, &dictate, &notes, &stop, &sep2, &settings, &quit])
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app, None),
        "dictate" => app.state::<dictation::Dictation>().toggle(app),
        "notes" => {
            if let Err(e) = app.state::<meeting::Notetaker>().start(app, "Meeting", None, true, None) {
                let _ = app.emit("meeting-transcribe-error", e);
                show_main(app, Some("meetings"));
            }
        }
        "stop" => app.state::<meeting::Notetaker>().stop(app),
        "settings" => show_main(app, Some("settings")),
        "quit" => {
            app.state::<meeting::Notetaker>().stop(app);
            app.exit(0);
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = match store::Store::open(&paths::db_path()) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("[odm] the database could not be opened ({e}); running without saving");
            Arc::new(store::Store::open_in_memory())
        }
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(hotkey::plugin())
        .manage(store)
        .manage(meeting::Notetaker::default())
        .manage(dictation::Dictation::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_setting,
            commands::set_api_key,
            commands::check_api_key,
            commands::list_microphones,
            commands::set_dictation_key,
            commands::set_calendar_url,
            commands::request_accessibility,
            commands::open_privacy_settings,
            commands::list_meeting_notes,
            commands::get_meeting_state,
            commands::get_live_transcript,
            commands::get_meeting_offer,
            commands::start_meeting_notes,
            commands::stop_meeting_notes,
            commands::accept_meeting_offer,
            commands::decline_meeting_offer,
            commands::save_meeting_my_notes,
            commands::summarise_meeting,
            commands::rename_meeting,
            commands::set_meeting_people,
            commands::delete_meeting,
            commands::meeting_markdown,
            commands::export_meeting,
            commands::export_all_meetings,
            commands::upcoming_meetings,
            commands::add_meeting,
            commands::delete_upcoming_meeting,
            commands::refresh_calendar,
            commands::build_meeting_prep,
            commands::answer_prep_question,
            commands::drop_prep_point,
            commands::add_prep_point,
            commands::set_prep_people,
            commands::toggle_dictation,
            commands::show_main,
            pill::pill_visible,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            if let Some(tray) = app.tray_by_id("main") {
                let menu = tray_menu(&handle)?;
                tray.set_menu(Some(menu))?;
                tray.on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
            }
            hotkey::apply_saved(&handle);
            meeting::watch(handle.clone());
            prep::watch(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps the app in the tray, so the
            // dictation key and the notetaker keep working.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main(app, None);
            }
            let _ = (app, &event);
        });
}
