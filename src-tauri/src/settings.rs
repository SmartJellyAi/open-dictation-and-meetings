//! Setting names, their defaults, and the view of them the Settings page
//! reads and writes. Everything is stored in the app's own database.

use serde::{Deserialize, Serialize};

use crate::store::Store;

pub const OPENAI_KEY: &str = "openai_key";
pub const DEEPGRAM_KEY: &str = "deepgram_key";
/// The microphone by name; empty is the system's default.
pub const MIC_DEVICE: &str = "mic_device";
/// The dictation shortcut, in the global-shortcut syntax ("Alt+Space").
pub const DICTATION_KEY: &str = "dictation_key";
/// "toggle": press to start, press again to stop. "hold": talk while held.
pub const DICTATION_MODE: &str = "dictation_mode";
/// Tidy the words with the model before they are typed.
pub const DICTATION_CLEANUP: &str = "dictation_cleanup";
pub const DICTATION_ON: &str = "dictation_on";
/// Notice a call in a meeting app and offer to take notes.
pub const MEETING_DETECTION: &str = "meeting_detection";
pub const NOTETAKER_ON: &str = "notetaker_on";
/// Prepare for meetings on the calendar ahead of time.
pub const PRE_MEETING_NOTES: &str = "pre_meeting_notes";
/// Look the people on a meeting up on the web while preparing.
pub const WEB_RESEARCH: &str = "web_research";
/// A calendar's private address in iCal format.
pub const CALENDAR_URL: &str = "calendar_ics_url";
pub const YOUR_NAME: &str = "your_name";
pub const ABOUT_YOU: &str = "about_you";

#[cfg(target_os = "macos")]
pub const DEFAULT_DICTATION_KEY: &str = "Alt+Space";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DICTATION_KEY: &str = "Ctrl+Alt+Space";

/// What the Settings page shows. Keys are never sent back to the page in
/// full: it only learns whether one is set and its last four characters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub openai_key_hint: Option<String>,
    pub deepgram_key_hint: Option<String>,
    pub mic_device: String,
    pub dictation_on: bool,
    pub dictation_key: String,
    pub dictation_mode: String,
    pub dictation_cleanup: bool,
    pub notetaker_on: bool,
    pub meeting_detection: bool,
    pub pre_meeting_notes: bool,
    pub web_research: bool,
    pub calendar_url_set: bool,
    pub your_name: String,
    pub about_you: String,
    /// What this system can do, so the page can say what is missing.
    pub platform: crate::platform::Support,
}

fn hint(key: Option<String>) -> Option<String> {
    let key = key?.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    Some(format!("ends in {tail}"))
}

pub fn view(store: &Store) -> SettingsView {
    SettingsView {
        openai_key_hint: hint(store.get(OPENAI_KEY)),
        deepgram_key_hint: hint(store.get(DEEPGRAM_KEY)),
        mic_device: store.get_or(MIC_DEVICE, ""),
        dictation_on: store.get_bool(DICTATION_ON, true),
        dictation_key: store.get_or(DICTATION_KEY, DEFAULT_DICTATION_KEY),
        dictation_mode: store.get_or(DICTATION_MODE, "toggle"),
        dictation_cleanup: store.get_bool(DICTATION_CLEANUP, true),
        notetaker_on: store.get_bool(NOTETAKER_ON, true),
        meeting_detection: store.get_bool(MEETING_DETECTION, true),
        pre_meeting_notes: store.get_bool(PRE_MEETING_NOTES, true),
        web_research: store.get_bool(WEB_RESEARCH, true),
        calendar_url_set: store.get(CALENDAR_URL).is_some_and(|u| !u.trim().is_empty()),
        your_name: store.get_or(YOUR_NAME, ""),
        about_you: store.get_or(ABOUT_YOU, ""),
        platform: crate::platform::support(),
    }
}

/// A key as stored, or None when not set.
pub fn key(store: &Store, name: &str) -> Option<String> {
    store.get(name).map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_only_ever_shown_by_its_last_four() {
        assert_eq!(hint(Some("sk-abcdefgh1234".into())), Some("ends in 1234".into()));
        assert_eq!(hint(Some("   ".into())), None);
        assert_eq!(hint(None), None);
    }

    #[test]
    fn defaults_are_on_for_a_new_install() {
        let s = Store::open_in_memory();
        let v = view(&s);
        assert!(v.dictation_on && v.notetaker_on && v.meeting_detection && v.pre_meeting_notes);
        assert_eq!(v.dictation_mode, "toggle");
        assert_eq!(v.dictation_key, DEFAULT_DICTATION_KEY);
        assert!(v.openai_key_hint.is_none());
    }
}
