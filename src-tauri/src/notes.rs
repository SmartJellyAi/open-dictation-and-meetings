//! A call's notes: how a meeting is named and shown, the summary written
//! after the call, and the Markdown file it can be saved as.

use crate::store::{MeetingRow, Store};
use crate::summary_md::{self, Block};
use crate::transcript::{named, parse_transcript, Line};

/// One call as the page reads it.
#[derive(Clone, serde::Serialize)]
pub struct MeetingNote {
    pub id: String,
    pub service: String,
    /// "Google Meet with Priya", or the name the person gave the call.
    pub display_title: String,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub transcript: String,
    pub transcript_lines: Vec<Line>,
    pub summary: Option<String>,
    pub summary_blocks: Vec<Block>,
    pub summarised_at: Option<i64>,
    pub my_notes: String,
    pub my_notes_at: Option<i64>,
    pub meeting_code: Option<String>,
    pub participants: Vec<String>,
    /// The one other person on the call, whose lines carry their name.
    pub others_name: Option<String>,
    pub dropped: i64,
}

/// A browser is where a call ran, not what it was.
fn is_browser(name: &str) -> bool {
    ["Safari", "Google Chrome", "Arc", "Firefox", "Microsoft Edge", "Brave Browser", "Opera", "Vivaldi", "Chromium", "Orion"].iter().any(|b| b.eq_ignore_ascii_case(name.trim()))
}

/// A call is named by where it happened and who was on it: "Google Meet
/// with Priya", "Zoom with Sam and Lee". Nobody known: "Zoom call".
pub fn list_title(service: &str, others: &[String]) -> String {
    let app = if service.trim().is_empty() || is_browser(service) || service == "a meeting" { "Call" } else { service.trim() };
    let firsts: Vec<String> = others.iter().map(|p| p.split_whitespace().next().unwrap_or("").to_string()).filter(|p| !p.is_empty()).collect();
    let who = match firsts.as_slice() {
        [] => return if app == "Call" { "Call".into() } else { format!("{app} call") },
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    };
    format!("{app} with {who}")
}

/// The people on the call other than you.
pub fn others_on_call(participants: &[String], your_name: &str) -> Vec<String> {
    let me = your_name.split_whitespace().next().unwrap_or("").to_lowercase();
    participants
        .iter()
        .filter(|p| me.is_empty() || p.split_whitespace().next().unwrap_or("").to_lowercase() != me)
        .cloned()
        .collect()
}

pub fn view(row: &MeetingRow, your_name: &str) -> MeetingNote {
    let others = others_on_call(&row.participants, your_name);
    let display_title = if row.title.trim().is_empty() { list_title(&row.service, &others) } else { row.title.trim().to_string() };
    let mut summary_blocks = row.summary.as_deref().map(summary_md::parse).unwrap_or_default();
    summary_md::mark_mine(&mut summary_blocks, &row.my_notes);
    MeetingNote {
        id: row.id.clone(),
        service: row.service.clone(),
        display_title,
        title: row.title.clone(),
        started_at: row.started_at,
        ended_at: row.ended_at,
        transcript: row.transcript.clone(),
        transcript_lines: parse_transcript(&row.transcript),
        summary: row.summary.clone(),
        summary_blocks,
        summarised_at: row.summarised_at,
        my_notes: row.my_notes.clone(),
        my_notes_at: row.my_notes_at,
        meeting_code: row.meeting_code.clone(),
        participants: row.participants.clone(),
        others_name: match others.as_slice() {
            [one] => one.split_whitespace().next().map(str::to_string),
            _ => None,
        },
        dropped: row.dropped,
    }
}

// ---------------------------------------------------------------- summary

/// Organised by what was actually talked about, not forced into a fixed
/// template: an overview plus only Decisions and Follow-ups had nowhere to
/// put a number mentioned, an idea raised or an open question.
pub const MEETING_SUMMARY_SYSTEM: &str = "\
You summarise a meeting from an automatic transcript, the way a sharp colleague's \
notes would read: organised by what was actually talked about, not forced into a \
fixed template.

WHO SAID WHAT. Every line is labelled. `You:` is the person you are writing for, \
from their own microphone. `Others:` and `Speaker N:` are everyone else in the call. \
On a call with one other person, their lines carry their name instead (`Priya:`), \
and WITH names them: write \"Priya agreed to...\", never \"someone\" for a person who \
is named.

Use those labels. Say \"you agreed to...\" for a You line and \"someone agreed \
to...\" for an Others line. Never say \"one participant\" about something the reader \
themselves said: a note that cannot tell the reader what THEY committed to has lost \
the most useful thing in the meeting.

KNOWN NAMES lists people the reader named for this call. Spell these exactly as \
given. When the transcript has a word that is plainly one of them misheard, write \
the known name. Never change a word to a known name it does not sound like.

The transcript is imperfect: names are often misspelt and some words are wrong or \
missing. Work with what is there. Never invent a detail to make the summary read \
better, and never guess at a name you cannot read; say 'someone' instead. If a \
passage reads as confident gibberish, leave it out rather than inventing a meaning.

STRUCTURE. Open with two or three sentences on what the meeting was about. Then \
organise the rest under however many `##` headings the actual conversation calls \
for, named after its real topics (\"Payout System\", \"Product Feedback\"), not \
generic labels like \"Discussion\". Under each heading, bullets carry the substance, \
with a nested bullet (indent two spaces) for a supporting detail.

Everything genuinely discussed belongs somewhere, not only what got decided or \
assigned: an idea raised, a number mentioned, an open question. Keep a number or an \
unresolved detail exactly as ambiguous as the transcript leaves it.

End with two more sections, always last, under these exact headings: **Decisions** \
(only what was actually settled, each with who settled it) and **Follow-ups** (who \
owes what, and by when if a time was said). Vary the sentence openings across \
bullets. Drop any heading entirely rather than filling it with things the transcript \
does not support, including Decisions or Follow-ups.

THE READER'S OWN NOTES. When the prompt includes `Your notes:`, those are lines the \
reader typed during or after the call, and they are the skeleton of the summary. \
Keep every one of them as its own bullet, word for word, placed under the topic it \
belongs to. Put what the transcript adds around them. Trust their spelling of names.

Never write an em dash or an en dash: use a comma, a colon or a full stop.

Only refuse if there is genuinely no content to work with: a handful of \
disconnected words and nothing else. In that one case reply with exactly NO_CONTENT \
and nothing else. Otherwise a short, hedged summary is far more useful than a \
refusal.";

/// What the model replies when a transcript cannot be summarised. A token
/// rather than a sentence, so a refusal is never stored as the notes.
const NO_CONTENT: &str = "NO_CONTENT";

/// A call long enough that its notes are worth writing unasked.
pub const AUTO_SUMMARY_MIN_WORDS: usize = 40;

/// The request for one call's notes, built from what is stored. Errors
/// when there is too little to write from.
pub fn summary_prompt(row: &MeetingRow, your_name: &str) -> Result<String, String> {
    let transcript = row.transcript.trim();
    let my_notes = row.my_notes.trim();
    let words = transcript.split_whitespace().count();
    if my_notes.is_empty() && words < 12 {
        return Err(format!("Only {words} words were heard, not enough to write notes from. Open the transcript to read what was picked up."));
    }
    let others = others_on_call(&row.participants, your_name);
    let transcript = match others.as_slice() {
        [one] => named(transcript, one),
        _ => transcript.to_string(),
    };
    let mut known: Vec<String> = Vec::new();
    if !your_name.trim().is_empty() {
        known.push(format!("{} (the reader)", your_name.trim()));
    }
    known.extend(others.iter().cloned());
    let with = if others.is_empty() { String::new() } else { format!("\nWITH: {}", others.join(", ")) };
    let known = if known.is_empty() { String::new() } else { format!("\nKNOWN NAMES: {}", known.join("; ")) };
    let what = if row.title.trim().is_empty() { row.service.clone() } else { format!("{} ({})", row.title.trim(), row.service) };
    Ok(if my_notes.is_empty() {
        format!("Meeting on {what}.{with}{known}\n\nTranscript:\n{transcript}")
    } else {
        format!("Meeting on {what}.{with}{known}\n\nYour notes:\n{my_notes}\n\nTranscript:\n{transcript}")
    })
}

/// Writes one call's notes and stores them. Reads under the lock, calls
/// the model unlocked, writes back after.
pub fn summarise(store: &Store, id: &str) -> Result<String, String> {
    let row = store.meeting(id).ok_or_else(|| "That meeting is no longer here.".to_string())?;
    let key = crate::settings::key(store, crate::settings::OPENAI_KEY).ok_or_else(|| "Add an OpenAI key in Settings to write notes.".to_string())?;
    let your_name = store.get_or(crate::settings::YOUR_NAME, "");
    let prompt = summary_prompt(&row, &your_name)?;
    let summary = crate::openai::generate_text(&key, MEETING_SUMMARY_SYSTEM, &prompt)?;
    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err("The notes came back empty. Try again.".to_string());
    }
    if summary.trim_matches(|c: char| !c.is_alphanumeric() && c != '_') == NO_CONTENT {
        return Err("There was not enough in this transcript to write notes from.".to_string());
    }
    store.set_meeting_summary(id, &summary);
    Ok(summary)
}

// ----------------------------------------------------------------- export

/// The meeting as Markdown: when and with whom, your notes, the notes.
pub fn markdown(note: &MeetingNote) -> String {
    use chrono::TimeZone;
    let title = note.display_title.trim();
    let start = chrono::Local.timestamp_millis_opt(note.started_at).single();
    let mut head: Vec<String> = Vec::new();
    if let Some(d) = start {
        head.push(d.format("%A %-d %B %Y, %-I:%M %p").to_string());
    }
    if let Some(end) = note.ended_at {
        head.push(format!("{} min", ((end - note.started_at) / 60_000).max(1)));
    }
    head.push(note.service.clone());
    if let Some(code) = note.meeting_code.as_deref().filter(|c| !c.trim().is_empty()) {
        head.push(code.to_string());
    }
    let mut out = format!("# {title}\n\n{}\n", head.join(" · "));
    if !note.participants.is_empty() {
        out.push_str(&format!("On the call: {}\n", note.participants.join(", ")));
    }
    if !note.my_notes.trim().is_empty() {
        out.push_str("\n## Your notes\n\n");
        out.push_str(note.my_notes.trim());
        out.push('\n');
    }
    if let Some(summary) = note.summary.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // The file has its title; the summary's own top heading is not
        // repeated under it.
        let body = match summary.split_once('\n') {
            Some((first, rest)) if first.starts_with("# ") || first.trim_start_matches('#').trim().eq_ignore_ascii_case(title) => rest.trim(),
            _ => summary,
        };
        out.push_str("\n## Notes\n\n");
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// The transcript as plain text, with a header, for "Save the transcript".
pub fn transcript_text(note: &MeetingNote) -> String {
    let others = note.others_name.clone().unwrap_or_default();
    format!("{}\n\n{}", note.display_title, named(note.transcript.trim(), &others))
}

/// Whether the file would have notes in it.
pub fn has_notes(note: &MeetingNote) -> bool {
    note.summary.as_deref().is_some_and(|s| !s.trim().is_empty()) || !note.my_notes.trim().is_empty()
}

/// "2026-10-21 16.00 Google Meet with Sam.md": sorts by date, two calls
/// on one day stay apart, and the title reads in a file browser.
pub fn filename(note: &MeetingNote, extension: &str) -> String {
    use chrono::TimeZone;
    let day = chrono::Local.timestamp_millis_opt(note.started_at).single().map(|d| d.format("%Y-%m-%d %H.%M").to_string()).unwrap_or_else(|| "undated".to_string());
    let title: String = note.display_title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '"' | '*' | '?' | '<' | '>' | '|' | '\n' | '\r' | '\t') { '-' } else { c }).collect();
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = title.chars().take(80).collect();
    format!("{day} {}.{extension}", if title.is_empty() { "Meeting" } else { title.trim() })
}

pub fn write_file(name: &str, body: &str) -> Result<std::path::PathBuf, String> {
    let dir = crate::paths::export_dir();
    std::fs::create_dir_all(&dir).map_err(|_| "The folder in Downloads could not be made.".to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, body).map_err(|_| "The file could not be written.".to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> MeetingRow {
        MeetingRow {
            id: "m".into(),
            service: "Google Meet".into(),
            participants: vec!["Sam Rivera".into(), "Lee Park".into()],
            started_at: 1_790_000_000_000,
            ended_at: Some(1_790_000_000_000 + 30 * 60_000),
            transcript: "You: the notes are coming today\nOthers: great, thanks".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_call_is_named_by_app_and_people() {
        assert_eq!(list_title("Google Meet", &["Sam Rivera".into()]), "Google Meet with Sam");
        assert_eq!(list_title("Zoom", &["Sam".into(), "Lee".into()]), "Zoom with Sam and Lee");
        assert_eq!(list_title("Zoom", &["A".into(), "B".into(), "C".into()]), "Zoom with A, B and 1 more");
        assert_eq!(list_title("Zoom", &[]), "Zoom call");
        assert_eq!(list_title("Google Chrome", &[]), "Call");
        assert_eq!(list_title("Arc", &["Sam".into()]), "Call with Sam");
    }

    #[test]
    fn you_are_never_one_of_the_others() {
        let people = vec!["Ada Lovelace".to_string(), "Sam Rivera".to_string()];
        assert_eq!(others_on_call(&people, "Ada"), vec!["Sam Rivera".to_string()]);
        assert_eq!(others_on_call(&people, ""), people);
    }

    #[test]
    fn a_one_on_one_names_the_other_side_in_the_view() {
        let mut r = row();
        r.participants = vec!["Ada Lovelace".into(), "Sam Rivera".into()];
        let v = view(&r, "Ada");
        assert_eq!(v.display_title, "Google Meet with Sam");
        assert_eq!(v.others_name.as_deref(), Some("Sam"));
        assert_eq!(v.transcript_lines.len(), 2);
    }

    #[test]
    fn a_meeting_becomes_one_readable_markdown_file() {
        let mut r = row();
        r.participants = vec!["Ada".into(), "Sam Rivera".into()];
        r.summary = Some("# Google Meet with Sam\n\n**What was decided**\n- A two-week pilot in November.".into());
        r.my_notes = "ask about the pilot team".into();
        let note = view(&r, "Ada");
        let md = markdown(&note);
        assert!(md.starts_with("# Google Meet with Sam\n"), "{md}");
        assert!(md.contains("On the call: Ada, Sam Rivera"));
        assert!(md.contains("## Your notes\n\nask about the pilot team"));
        assert!(md.contains("## Notes\n\n**What was decided**"), "the summary's own title is not repeated: {md}");
        assert!(!md.contains("the notes are coming today"), "the transcript is not in the notes file: {md}");
        assert!(has_notes(&note));
        let name = filename(&note, "md");
        assert!(name.ends_with(" Google Meet with Sam.md"), "{name}");
        assert!(name.chars().take(10).all(|c| c.is_ascii_digit() || c == '-'), "{name}");
    }

    #[test]
    fn a_file_name_never_carries_a_character_a_system_refuses() {
        let mut r = row();
        r.title = "Q3: plan / review? <draft>".into();
        let name = filename(&view(&r, ""), "md");
        assert!(!name.contains(':') && !name.contains('/') && !name.contains('?') && !name.contains('<'), "{name}");
    }

    #[test]
    fn too_little_to_write_from_says_so_before_any_request() {
        let mut r = row();
        r.transcript = "You: hello there".into();
        assert!(summary_prompt(&r, "").is_err());
        r.my_notes = "talked pricing".into();
        assert!(summary_prompt(&r, "").is_ok(), "your own notes are enough to write from");
    }

    #[test]
    fn the_prompt_names_the_one_other_person() {
        let mut r = row();
        r.participants = vec!["Ada".into(), "Sam Rivera".into()];
        r.transcript = "You: one two three four five six\nOthers: seven eight nine ten eleven twelve thirteen".into();
        let p = summary_prompt(&r, "Ada").unwrap();
        assert!(p.contains("Sam: seven eight"), "{p}");
        assert!(p.contains("WITH: Sam Rivera"), "{p}");
        assert!(p.contains("KNOWN NAMES: Ada (the reader); Sam Rivera"), "{p}");
    }

    #[test]
    fn the_summary_prompt_has_no_dashes_a_person_would_read() {
        assert!(!MEETING_SUMMARY_SYSTEM.contains('\u{2014}') && !MEETING_SUMMARY_SYSTEM.contains('\u{2013}'));
    }
}
