//! The dictation key: one system-wide shortcut, chosen in Settings.
//!
//! In toggle mode a press starts and the next press stops. In hold mode
//! the words are heard while the key is down and typed when it is let go.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings;
use crate::store::Store;

static CURRENT: Mutex<Option<Shortcut>> = Mutex::new(None);

/// The plugin, with what a press and a release do.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            let ours = CURRENT.lock().unwrap().as_ref().is_some_and(|s| s == shortcut);
            if !ours {
                return;
            }
            let app = app.clone();
            let hold = app.state::<Arc<Store>>().get_or(settings::DICTATION_MODE, "toggle") == "hold";
            let dictation = app.state::<crate::dictation::Dictation>();
            match (event.state(), hold) {
                (ShortcutState::Pressed, true) => dictation.hold_start(&app),
                (ShortcutState::Released, true) => dictation.hold_stop(),
                (ShortcutState::Pressed, false) => dictation.toggle(&app),
                (ShortcutState::Released, false) => {}
            }
        })
        .build()
}

/// Whether a shortcut is written in a form the system can register.
pub fn parse(key: &str) -> Result<Shortcut, String> {
    key.trim().parse::<Shortcut>().map_err(|_| "That key combination is not one this app can use. Pick another.".to_string())
}

/// Registers `key` as the dictation key, replacing the one before. On
/// failure the old key stays.
pub fn apply(app: &AppHandle, key: &str) -> Result<(), String> {
    let shortcut = parse(key)?;
    let gs = app.global_shortcut();
    let mut current = CURRENT.lock().unwrap();
    if current.as_ref() == Some(&shortcut) && gs.is_registered(shortcut) {
        return Ok(());
    }
    if let Err(e) = gs.register(shortcut) {
        eprintln!("[hotkey] register failed: {e}");
        return Err("That key is already used by another app or by the system. Pick another.".into());
    }
    if let Some(old) = current.take() {
        if old != shortcut {
            let _ = gs.unregister(old);
        }
    }
    *current = Some(shortcut);
    Ok(())
}

/// Turns the key off (dictation switched off in Settings).
pub fn clear(app: &AppHandle) {
    if let Some(old) = CURRENT.lock().unwrap().take() {
        let _ = app.global_shortcut().unregister(old);
    }
}

/// The key as stored, registered at launch when dictation is on.
pub fn apply_saved(app: &AppHandle) {
    let store = app.state::<Arc<Store>>();
    if !store.get_bool(settings::DICTATION_ON, true) {
        return;
    }
    let key = store.get_or(settings::DICTATION_KEY, settings::DEFAULT_DICTATION_KEY);
    if let Err(e) = apply(app, &key) {
        crate::transcript::log(&format!("the dictation key could not be set: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_keys_parse() {
        assert!(parse("Alt+Space").is_ok());
        assert!(parse("Ctrl+Alt+Space").is_ok());
        assert!(parse("CommandOrControl+Shift+D").is_ok());
        assert!(parse("not a key").is_err());
    }
}
