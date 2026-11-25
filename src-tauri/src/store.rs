//! The app's own small SQLite store: settings and keys, meeting notes,
//! upcoming meetings and their preparation. One file in the app's data
//! folder. Nothing here is ever sent anywhere.
//!
//! The lock is only ever held for one statement or one short batch. Model
//! and network calls read what they need first, let go, and write back
//! after, so the window never waits on the database while a request runs.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub struct Store {
    conn: Mutex<Connection>,
}

/// One call's notes as stored.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingRow {
    pub id: String,
    /// Where the call happened: "Zoom", "Google Meet", or "Meeting" when
    /// it was started by hand.
    pub service: String,
    /// A name the person gave the call, or the calendar event's title.
    pub title: String,
    pub participants: Vec<String>,
    pub meeting_code: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub transcript: String,
    pub my_notes: String,
    pub my_notes_at: Option<i64>,
    pub summary: Option<String>,
    pub summarised_at: Option<i64>,
    pub dropped: i64,
    pub event_id: Option<String>,
}

/// A meeting to come, from a calendar feed or typed in by hand.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct EventRow {
    pub id: String,
    pub title: String,
    pub starts_at: i64,
    pub ends_at: Option<i64>,
    pub location: String,
    pub description: String,
    pub attendees: Vec<String>,
    pub link: Option<String>,
    /// "calendar" or "manual".
    pub source: String,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        // Keys live in this file, so only its owner may read it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        let store = Self { conn: Mutex::new(conn) };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Self {
        let store = Self { conn: Mutex::new(Connection::open_in_memory().expect("in-memory database")) };
        store.migrate().expect("schema");
        store
    }

    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meetings (
                id TEXT PRIMARY KEY,
                service TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                participants TEXT NOT NULL DEFAULT '[]',
                meeting_code TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                transcript TEXT NOT NULL DEFAULT '',
                my_notes TEXT NOT NULL DEFAULT '',
                my_notes_at INTEGER,
                summary TEXT,
                summarised_at INTEGER,
                dropped INTEGER NOT NULL DEFAULT 0,
                event_id TEXT
             );
             CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                starts_at INTEGER NOT NULL,
                ends_at INTEGER,
                location TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                attendees TEXT NOT NULL DEFAULT '[]',
                link TEXT,
                source TEXT NOT NULL DEFAULT 'manual'
             );
             CREATE TABLE IF NOT EXISTS preps (
                event_id TEXT PRIMARY KEY,
                body TEXT NOT NULL,
                updated_at INTEGER NOT NULL
             );",
        )
        .map_err(|e| e.to_string())
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn)
    }

    // ---- settings ------------------------------------------------------

    pub fn get(&self, key: &str) -> Option<String> {
        self.with(|c| c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten())
    }

    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or_else(|| default.to_string())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.get(key).as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") => false,
            _ => default,
        }
    }

    pub fn set(&self, key: &str, value: &str) {
        self.with(|c| {
            let _ = c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            );
        });
    }

    pub fn set_bool(&self, key: &str, value: bool) {
        self.set(key, if value { "1" } else { "0" });
    }

    // ---- meetings ------------------------------------------------------

    pub fn insert_meeting(&self, m: &MeetingRow) {
        self.with(|c| {
            let _ = c.execute(
                "INSERT INTO meetings (id, service, title, participants, meeting_code, started_at, ended_at, transcript, my_notes, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    m.id,
                    m.service,
                    m.title,
                    serde_json::to_string(&m.participants).unwrap_or_else(|_| "[]".into()),
                    m.meeting_code,
                    m.started_at,
                    m.ended_at,
                    m.transcript,
                    m.my_notes,
                    m.event_id
                ],
            );
        });
    }

    fn meeting_from_row(r: &rusqlite::Row) -> rusqlite::Result<MeetingRow> {
        let participants: String = r.get(3)?;
        Ok(MeetingRow {
            id: r.get(0)?,
            service: r.get(1)?,
            title: r.get(2)?,
            participants: serde_json::from_str(&participants).unwrap_or_default(),
            meeting_code: r.get(4)?,
            started_at: r.get(5)?,
            ended_at: r.get(6)?,
            transcript: r.get(7)?,
            my_notes: r.get(8)?,
            my_notes_at: r.get(9)?,
            summary: r.get(10)?,
            summarised_at: r.get(11)?,
            dropped: r.get(12)?,
            event_id: r.get(13)?,
        })
    }

    const MEETING_COLUMNS: &'static str = "id, service, title, participants, meeting_code, started_at, ended_at, transcript, my_notes, my_notes_at, summary, summarised_at, dropped, event_id";

    pub fn list_meetings(&self, limit: usize) -> Vec<MeetingRow> {
        self.with(|c| {
            let sql = format!("SELECT {} FROM meetings ORDER BY started_at DESC LIMIT ?1", Self::MEETING_COLUMNS);
            c.prepare(&sql)
                .and_then(|mut q| q.query_map([limit as i64], Self::meeting_from_row).map(|rows| rows.filter_map(Result::ok).collect()))
                .unwrap_or_default()
        })
    }

    pub fn meeting(&self, id: &str) -> Option<MeetingRow> {
        self.with(|c| {
            let sql = format!("SELECT {} FROM meetings WHERE id = ?1", Self::MEETING_COLUMNS);
            c.query_row(&sql, [id], Self::meeting_from_row).optional().ok().flatten()
        })
    }

    pub fn set_meeting_transcript(&self, id: &str, transcript: &str, dropped: i64) {
        self.with(|c| {
            let _ = c.execute("UPDATE meetings SET transcript = ?2, dropped = ?3 WHERE id = ?1", params![id, transcript, dropped]);
        });
    }

    pub fn end_meeting(&self, id: &str, ended_at: i64) {
        self.with(|c| {
            let _ = c.execute("UPDATE meetings SET ended_at = ?2 WHERE id = ?1", params![id, ended_at]);
        });
    }

    pub fn set_meeting_my_notes(&self, id: &str, text: &str) -> bool {
        self.with(|c| c.execute("UPDATE meetings SET my_notes = ?2, my_notes_at = ?3 WHERE id = ?1", params![id, text, now_ms()]).map(|n| n > 0).unwrap_or(false))
    }

    pub fn set_meeting_summary(&self, id: &str, summary: &str) {
        self.with(|c| {
            let _ = c.execute("UPDATE meetings SET summary = ?2, summarised_at = ?3 WHERE id = ?1", params![id, summary, now_ms()]);
        });
    }

    pub fn set_meeting_title(&self, id: &str, title: &str) -> bool {
        self.with(|c| c.execute("UPDATE meetings SET title = ?2 WHERE id = ?1", params![id, title.trim()]).map(|n| n > 0).unwrap_or(false))
    }

    pub fn set_meeting_participants(&self, id: &str, people: &[String]) -> bool {
        let json = serde_json::to_string(people).unwrap_or_else(|_| "[]".into());
        self.with(|c| c.execute("UPDATE meetings SET participants = ?2 WHERE id = ?1", params![id, json]).map(|n| n > 0).unwrap_or(false))
    }

    pub fn delete_meeting(&self, id: &str) -> bool {
        self.with(|c| c.execute("DELETE FROM meetings WHERE id = ?1", [id]).map(|n| n > 0).unwrap_or(false))
    }

    // ---- upcoming meetings ---------------------------------------------

    pub fn upsert_event(&self, e: &EventRow) {
        self.with(|c| {
            let _ = c.execute(
                "INSERT INTO events (id, title, starts_at, ends_at, location, description, attendees, link, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET title = excluded.title, starts_at = excluded.starts_at, ends_at = excluded.ends_at,
                   location = excluded.location, description = excluded.description, attendees = excluded.attendees,
                   link = excluded.link, source = excluded.source",
                params![
                    e.id,
                    e.title,
                    e.starts_at,
                    e.ends_at,
                    e.location,
                    e.description,
                    serde_json::to_string(&e.attendees).unwrap_or_else(|_| "[]".into()),
                    e.link,
                    e.source
                ],
            );
        });
    }

    /// Replaces every calendar event with what the feed holds now, so a
    /// meeting removed from the calendar leaves the list. Events typed in
    /// by hand are untouched.
    pub fn replace_calendar_events(&self, events: &[EventRow]) {
        self.with(|c| {
            let _ = c.execute("DELETE FROM events WHERE source = 'calendar'", []);
        });
        for e in events {
            self.upsert_event(e);
        }
    }

    fn event_from_row(r: &rusqlite::Row) -> rusqlite::Result<EventRow> {
        let attendees: String = r.get(6)?;
        Ok(EventRow {
            id: r.get(0)?,
            title: r.get(1)?,
            starts_at: r.get(2)?,
            ends_at: r.get(3)?,
            location: r.get(4)?,
            description: r.get(5)?,
            attendees: serde_json::from_str(&attendees).unwrap_or_default(),
            link: r.get(7)?,
            source: r.get(8)?,
        })
    }

    /// Meetings from `from` on, soonest first.
    pub fn upcoming_events(&self, from: i64, limit: usize) -> Vec<EventRow> {
        self.with(|c| {
            c.prepare("SELECT id, title, starts_at, ends_at, location, description, attendees, link, source FROM events WHERE COALESCE(ends_at, starts_at + 3600000) >= ?1 ORDER BY starts_at ASC LIMIT ?2")
                .and_then(|mut q| q.query_map(params![from, limit as i64], Self::event_from_row).map(|rows| rows.filter_map(Result::ok).collect()))
                .unwrap_or_default()
        })
    }

    pub fn event(&self, id: &str) -> Option<EventRow> {
        self.with(|c| {
            c.query_row("SELECT id, title, starts_at, ends_at, location, description, attendees, link, source FROM events WHERE id = ?1", [id], Self::event_from_row)
                .optional()
                .ok()
                .flatten()
        })
    }

    pub fn delete_event(&self, id: &str) -> bool {
        self.with(|c| {
            let _ = c.execute("DELETE FROM preps WHERE event_id = ?1", [id]);
            c.execute("DELETE FROM events WHERE id = ?1", [id]).map(|n| n > 0).unwrap_or(false)
        })
    }

    // ---- preparation ---------------------------------------------------

    pub fn prep_json(&self, event_id: &str) -> Option<String> {
        self.with(|c| c.query_row("SELECT body FROM preps WHERE event_id = ?1", [event_id], |r| r.get(0)).optional().ok().flatten())
    }

    pub fn set_prep_json(&self, event_id: &str, body: &str) {
        self.with(|c| {
            let _ = c.execute(
                "INSERT INTO preps (event_id, body, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(event_id) DO UPDATE SET body = excluded.body, updated_at = excluded.updated_at",
                params![event_id, body, now_ms()],
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let s = Store::open_in_memory();
        assert_eq!(s.get("x"), None);
        s.set("x", "1");
        assert!(s.get_bool("x", false));
        s.set_bool("x", false);
        assert!(!s.get_bool("x", true));
        assert_eq!(s.get_or("missing", "d"), "d");
    }

    #[test]
    fn a_meeting_is_written_and_read_back() {
        let s = Store::open_in_memory();
        let m = MeetingRow { id: "m1".into(), service: "Zoom".into(), participants: vec!["Priya Shah".into()], started_at: 10, ..Default::default() };
        s.insert_meeting(&m);
        s.set_meeting_transcript("m1", "You: hello\n", 2);
        s.set_meeting_my_notes("m1", "ship it");
        s.set_meeting_summary("m1", "## Topic\n- point");
        s.end_meeting("m1", 20);
        let back = s.meeting("m1").unwrap();
        assert_eq!(back.transcript, "You: hello\n");
        assert_eq!(back.dropped, 2);
        assert_eq!(back.my_notes, "ship it");
        assert_eq!(back.ended_at, Some(20));
        assert_eq!(back.participants, vec!["Priya Shah".to_string()]);
        assert!(back.summary.is_some());
        assert_eq!(s.list_meetings(10).len(), 1);
        assert!(s.delete_meeting("m1"));
        assert!(s.meeting("m1").is_none());
    }

    #[test]
    fn a_calendar_refresh_keeps_meetings_typed_by_hand() {
        let s = Store::open_in_memory();
        let manual = EventRow { id: "a".into(), title: "By hand".into(), starts_at: 100, source: "manual".into(), ..Default::default() };
        let cal = EventRow { id: "b".into(), title: "From the feed".into(), starts_at: 200, source: "calendar".into(), ..Default::default() };
        s.upsert_event(&manual);
        s.upsert_event(&cal);
        s.replace_calendar_events(&[]);
        let left = s.upcoming_events(0, 10);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "a");
    }

    #[test]
    fn a_meeting_that_ended_leaves_the_upcoming_list() {
        let s = Store::open_in_memory();
        s.upsert_event(&EventRow { id: "old".into(), title: "Old".into(), starts_at: 0, ends_at: Some(1_000), source: "manual".into(), ..Default::default() });
        s.upsert_event(&EventRow { id: "new".into(), title: "New".into(), starts_at: 5_000, source: "manual".into(), ..Default::default() });
        let ids: Vec<String> = s.upcoming_events(2_000, 10).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["new".to_string()]);
    }
}
