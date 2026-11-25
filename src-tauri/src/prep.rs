//! Pre-meeting notes: before a meeting on your calendar, a short brief on
//! who you are meeting and what will probably come up.
//!
//! Material, in order: the meeting itself (title, time, invite text,
//! people), who you are (Settings), what the web says about the people
//! (when web research is on), the last call with them from your own notes,
//! and what you have answered or corrected on the page. One request writes
//! the brief, the questions worth asking you, and the points to be ready
//! for. Every point carries the fragment of the material it came from.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings;
use crate::store::{EventRow, Store};
use crate::transcript::log;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrepQuestion {
    pub question: String,
    #[serde(default)]
    pub answer: Option<String>,
}

/// One thing that will probably come up, why, and a line to use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prediction {
    pub point: String,
    pub why: String,
    pub source: String,
    #[serde(default)]
    pub say: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeetingPrep {
    pub event_id: String,
    pub participants: Vec<String>,
    /// The short message before the call: who, when, who they are, what
    /// it is likely about.
    pub brief: String,
    pub agenda: String,
    pub questions: Vec<PrepQuestion>,
    pub predictions: Vec<Prediction>,
    /// The material had too little to prepare from.
    pub thin: bool,
    #[serde(default)]
    pub last_time: Option<String>,
    /// Points you said will not come up: never predicted again.
    #[serde(default)]
    pub dropped: Vec<String>,
    /// Pages the web research cited, as (title, url).
    #[serde(default)]
    pub sources: Vec<(String, String)>,
    #[serde(default)]
    pub research: Option<String>,
    pub updated_at: i64,
}

/// The source a point carries when you typed it in yourself.
pub const READER_SOURCE: &str = "you";

/// How far ahead meetings are prepared.
const AHEAD_MS: i64 = 16 * 3_600_000;
/// A prep ready this close to the meeting is announced.
const ANNOUNCE_WITHIN_MS: i64 = 3 * 3_600_000;
/// How often the calendar is read again.
const CALENDAR_EVERY_MS: i64 = 15 * 60_000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn date(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|d| d.format("%-d %b").to_string()).unwrap_or_default()
}

pub fn when(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|d| d.format("%A %-d %B, %-I:%M %p").to_string()).unwrap_or_default()
}

pub fn load(store: &Store, event_id: &str) -> Option<MeetingPrep> {
    store.prep_json(event_id).and_then(|j| serde_json::from_str(&j).ok())
}

fn save(store: &Store, prep: &MeetingPrep) {
    if let Ok(j) = serde_json::to_string(prep) {
        store.set_prep_json(&prep.event_id, &j);
    }
}

const RESEARCH_SYSTEM: &str = "You look up the people on an upcoming meeting for the person attending it. For each \
person, in one or two lines: who they are and what their company does, from the web. Say plainly when you could not \
tell which person this is (a common name, no company to go on) rather than guessing, and never invent a fact. Then \
one line on what the meeting is likely about, from the invite and what you found. Plain text, short lines, no \
headings, under 160 words. Refer to people by name, never by he or she. Never write an em dash or an en dash.";

const PREP_SYSTEM: &str = "You prepare the reader for a meeting to come, the way a sharp chief of staff would. The \
material has THE MEETING (title, time, invite text, people), WHO THE READER IS, RESEARCH (what the web says about the \
people, with sources), LAST MEETING WITH THEM (from the reader's own notes), and what the reader has answered or \
corrected. Produce:\n\
- brief: one short message the reader reads before the call, in this order: who and when (\"You meet Sam Rivera \
on Tuesday at 3:00 PM on Google Meet.\"); who they are, from RESEARCH (say plainly when it could not tell which person \
this is); what the call is likely about, from the invite and the research; one line of advice fitted to the reader's \
own work, only when the material shows a concrete link. Plain text, short lines, under 120 words.\n\
- agenda: one or two plain sentences on what the call is for.\n\
- thin: true only when the material says nothing useful about this call beyond its title; then questions and \
predictions are empty.\n\
- questions: at most 3, only for gaps the material cannot fill and only if the answer would change the preparation \
(\"What do you want from this call?\"). Never ask what the material answers. Never ask again what the reader answered \
or declined.\n\
- predictions: up to 6 points that will probably come up or that the reader should bring up, each with why (one \
sentence), source (a fragment copied verbatim from the material) and say (one line the reader might use, or null). No \
point without a source. No generic points that do not serve the reader's goal. Never a point the reader said will not \
come up.\n\
- last_time: one sentence on the last meeting with these people, starting with when (\"12 Oct: you agreed to ...\"), \
only from LAST MEETING WITH THEM; null otherwise.\n\
Refer to other people by name, never by he or she: their pronouns are not known. Plain words, no preamble. Never \
write an em dash or an en dash: use a comma, a colon or a full stop. Never invent a fact, a title or a company. \
Output JSON only: {\"brief\": \"...\", \"agenda\": \"...\", \"thin\": false, \"questions\": [\"...\"], \"predictions\": \
[{\"point\": \"...\", \"why\": \"...\", \"source\": \"...\", \"say\": null}], \"last_time\": null}";

/// The people on a meeting other than you.
fn people_for(event: &EventRow, prior: Option<&MeetingPrep>, your_name: &str) -> Vec<String> {
    if let Some(p) = prior.filter(|p| !p.participants.is_empty()) {
        return p.participants.clone();
    }
    crate::notes::others_on_call(&event.attendees, your_name)
}

/// Everything the request is written from, as one text.
pub fn material(event: &EventRow, people: &[String], about_you: &str, your_name: &str, research: Option<&str>, last: Option<(i64, String)>, prior: Option<&MeetingPrep>) -> String {
    let mut m = String::new();
    m.push_str("## THE MEETING\n");
    m.push_str(&format!("Title: {}\nWhen: {}\n", event.title, when(event.starts_at)));
    if let Some(link) = &event.link {
        m.push_str(&format!("Call link: {link}\n"));
    }
    if !event.location.trim().is_empty() {
        m.push_str(&format!("Where: {}\n", event.location.trim()));
    }
    m.push_str(&format!("People: {}\n", if people.is_empty() { "(not known)".to_string() } else { people.join(", ") }));
    if !event.description.trim().is_empty() {
        m.push_str(&format!("Invite text:\n{}\n", clip(event.description.trim(), 3_000)));
    }
    if !your_name.trim().is_empty() || !about_you.trim().is_empty() {
        m.push_str("\n## WHO THE READER IS\n");
        if !your_name.trim().is_empty() {
            m.push_str(&format!("Name: {}\n", your_name.trim()));
        }
        if !about_you.trim().is_empty() {
            m.push_str(&format!("{}\n", clip(about_you.trim(), 2_000)));
        }
    }
    if let Some(r) = research.filter(|r| !r.trim().is_empty()) {
        m.push_str(&format!("\n## RESEARCH\n{}\n", clip(r.trim(), 4_000)));
    }
    if let Some((at, summary)) = last {
        m.push_str(&format!("\n## LAST MEETING WITH THEM ({})\n{}\n", date(at), clip(&summary, 1_500)));
    }
    if let Some(p) = prior {
        let answered: Vec<&PrepQuestion> = p.questions.iter().filter(|q| q.answer.as_deref().is_some_and(|a| !a.trim().is_empty())).collect();
        if !answered.is_empty() {
            m.push_str("\n## THE READER HAS ANSWERED\n");
            for q in answered {
                m.push_str(&format!("- Q: {}\n  A: {}\n", q.question, q.answer.clone().unwrap_or_default()));
            }
        }
        let skipped: Vec<&PrepQuestion> = p.questions.iter().filter(|q| q.answer.as_deref().is_some_and(|a| a.trim().is_empty())).collect();
        if !skipped.is_empty() {
            m.push_str("\n## THE READER DECLINED THESE; DO NOT ASK AGAIN\n");
            for q in skipped {
                m.push_str(&format!("- {}\n", q.question));
            }
        }
        let added: Vec<&Prediction> = p.predictions.iter().filter(|x| x.source == READER_SOURCE).collect();
        if !added.is_empty() {
            m.push_str("\n## THE READER EXPECTS THESE TO COME UP\n");
            for x in added {
                m.push_str(&format!("- {}\n", x.point));
            }
        }
        if !p.dropped.is_empty() {
            m.push_str("\n## THE READER SAYS THESE WILL NOT COME UP; NEVER PREDICT THEM\n");
            for d in &p.dropped {
                m.push_str(&format!("- {d}\n"));
            }
        }
    }
    m
}

/// Reads the model's reply into a prep, keeping what you typed in and
/// your answers, and leaving out what you dropped.
pub fn from_reply(raw: &str, event_id: &str, people: Vec<String>, prior: Option<&MeetingPrep>) -> MeetingPrep {
    let v: serde_json::Value = serde_json::from_str(crate::openai::strip_fence(raw)).unwrap_or(serde_json::Value::Null);
    let thin = v["thin"].as_bool().unwrap_or(raw.trim().is_empty());
    let dropped: Vec<String> = prior.map(|p| p.dropped.clone()).unwrap_or_default();
    let is_dropped = |point: &str| dropped.iter().any(|d| d.eq_ignore_ascii_case(point.trim()));
    let mut questions: Vec<PrepQuestion> = if thin {
        Vec::new()
    } else {
        v["questions"].as_array().map(|a| a.iter().filter_map(|q| q.as_str()).map(|q| PrepQuestion { question: q.trim().to_string(), answer: None }).filter(|q| !q.question.is_empty()).take(3).collect()).unwrap_or_default()
    };
    if let Some(p) = prior {
        for q in &mut questions {
            if let Some(old) = p.questions.iter().find(|o| o.question == q.question) {
                q.answer = old.answer.clone();
            }
        }
        for old in p.questions.iter().filter(|o| o.answer.is_some()) {
            if !questions.iter().any(|q| q.question == old.question) {
                questions.push(old.clone());
            }
        }
    }
    let mut predictions: Vec<Prediction> = prior.map(|p| p.predictions.iter().filter(|x| x.source == READER_SOURCE && !is_dropped(&x.point)).cloned().collect()).unwrap_or_default();
    if !thin {
        for x in v["predictions"].as_array().cloned().unwrap_or_default() {
            let point = x["point"].as_str().unwrap_or("").trim().to_string();
            let source = x["source"].as_str().unwrap_or("").trim().to_string();
            if point.is_empty() || source.is_empty() || is_dropped(&point) || predictions.iter().any(|p| p.point.eq_ignore_ascii_case(&point)) {
                continue;
            }
            predictions.push(Prediction {
                point,
                why: x["why"].as_str().unwrap_or("").trim().to_string(),
                source,
                say: x["say"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            });
            if predictions.len() >= 8 {
                break;
            }
        }
    }
    MeetingPrep {
        event_id: event_id.to_string(),
        participants: people,
        brief: v["brief"].as_str().unwrap_or("").trim().to_string(),
        agenda: v["agenda"].as_str().unwrap_or("").trim().to_string(),
        questions,
        predictions,
        thin,
        last_time: v["last_time"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        dropped,
        sources: prior.map(|p| p.sources.clone()).unwrap_or_default(),
        research: prior.and_then(|p| p.research.clone()),
        updated_at: now_ms(),
    }
}

/// The last call with any of these people that has notes.
fn last_meeting_with(store: &Store, people: &[String]) -> Option<(i64, String)> {
    if people.is_empty() {
        return None;
    }
    let firsts: Vec<String> = people.iter().filter_map(|p| p.split_whitespace().next()).map(|f| f.to_lowercase()).collect();
    store.list_meetings(200).into_iter().find_map(|m| {
        let summary = m.summary.clone().filter(|s| !s.trim().is_empty())?;
        let on_it = m.participants.iter().any(|q| firsts.iter().any(|f| q.to_lowercase().split_whitespace().next() == Some(f.as_str())))
            || firsts.iter().any(|f| m.title.to_lowercase().contains(f.as_str()));
        on_it.then_some((m.started_at, summary))
    })
}

/// Builds (or rebuilds) the prep for one meeting. Reads under the lock,
/// calls the model unlocked, writes back after.
pub fn build(store: &Store, event_id: &str, fresh_research: bool) -> Result<MeetingPrep, String> {
    let event = store.event(event_id).ok_or_else(|| "That meeting is no longer on the list.".to_string())?;
    let key = settings::key(store, settings::OPENAI_KEY).ok_or_else(|| "Add an OpenAI key in Settings to prepare for meetings.".to_string())?;
    let your_name = store.get_or(settings::YOUR_NAME, "");
    let about_you = store.get_or(settings::ABOUT_YOU, "");
    let research_on = store.get_bool(settings::WEB_RESEARCH, true);
    let prior = load(store, event_id);
    let people = people_for(&event, prior.as_ref(), &your_name);
    let last = last_meeting_with(store, &people);

    // What the web says about them, once per meeting unless asked again.
    let mut research = prior.as_ref().and_then(|p| p.research.clone());
    let mut sources = prior.as_ref().map(|p| p.sources.clone()).unwrap_or_default();
    if research_on && !people.is_empty() && (research.is_none() || fresh_research) {
        let input = format!("Meeting: {}\nPeople: {}\nInvite text: {}", event.title, people.join(", "), clip(event.description.trim(), 1_500));
        match crate::openai::web_search(&key, RESEARCH_SYSTEM, &input) {
            Ok((text, cites)) => {
                research = Some(text);
                sources = cites.into_iter().take(5).collect();
            }
            Err(e) => log(&format!("web research for a meeting failed: {e}")),
        }
    }

    let material = material(&event, &people, &about_you, &your_name, research.as_deref(), last, prior.as_ref());
    let prompt = format!("PARTICIPANTS: {}\n\n{}", if people.is_empty() { "(unknown)".to_string() } else { people.join(", ") }, clip(&material, 16_000));
    let raw = crate::openai::generate_json(&key, PREP_SYSTEM, &prompt)?;
    let mut prep = from_reply(&raw, event_id, people, prior.as_ref());
    prep.research = research;
    prep.sources = sources;
    save(store, &prep);
    Ok(prep)
}

/// Your answer to one of the questions, then the prep is written again.
pub fn answer(store: &Store, event_id: &str, index: usize, answer: Option<String>) -> Result<MeetingPrep, String> {
    let mut prep = load(store, event_id).ok_or_else(|| "There is no preparation for this meeting yet.".to_string())?;
    let q = prep.questions.get_mut(index).ok_or_else(|| "That question is gone.".to_string())?;
    // A skip is kept as an empty answer, so it is not asked again.
    q.answer = Some(answer.unwrap_or_default());
    save(store, &prep);
    build(store, event_id, false)
}

/// A point that will not come up: gone now and never predicted again.
pub fn drop_prediction(store: &Store, event_id: &str, index: usize) -> Result<MeetingPrep, String> {
    let mut prep = load(store, event_id).ok_or_else(|| "There is no preparation for this meeting yet.".to_string())?;
    if index < prep.predictions.len() {
        let gone = prep.predictions.remove(index);
        if !prep.dropped.iter().any(|d| d.eq_ignore_ascii_case(&gone.point)) {
            prep.dropped.push(gone.point);
        }
    }
    prep.updated_at = now_ms();
    save(store, &prep);
    Ok(prep)
}

/// A point you expect to come up, kept first.
pub fn add_prediction(store: &Store, event_id: &str, point: &str) -> Result<MeetingPrep, String> {
    let point = point.trim();
    if point.is_empty() {
        return Err("Write the point first.".into());
    }
    let mut prep = load(store, event_id).unwrap_or_else(|| MeetingPrep { event_id: event_id.to_string(), ..Default::default() });
    prep.predictions.insert(0, Prediction { point: point.to_string(), why: String::new(), source: READER_SOURCE.into(), say: None });
    prep.dropped.retain(|d| !d.eq_ignore_ascii_case(point));
    prep.updated_at = now_ms();
    save(store, &prep);
    Ok(prep)
}

pub fn set_participants(store: &Store, event_id: &str, people: Vec<String>) -> Result<MeetingPrep, String> {
    let mut prep = load(store, event_id).unwrap_or_else(|| MeetingPrep { event_id: event_id.to_string(), ..Default::default() });
    prep.participants = people.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    // New people, new research.
    prep.research = None;
    prep.sources.clear();
    save(store, &prep);
    build(store, event_id, true)
}

/// Reads the calendar now. Meetings typed in by hand are kept.
pub fn refresh_calendar(store: &Store) -> Result<usize, String> {
    let Some(url) = store.get(settings::CALENDAR_URL).filter(|u| !u.trim().is_empty()) else { return Ok(0) };
    let text = crate::ics::fetch(&url)?;
    let events = crate::ics::parse(&text, now_ms() - 3_600_000);
    store.replace_calendar_events(&events);
    store.set("calendar_read_at", &now_ms().to_string());
    Ok(events.len())
}

/// One pass of the background work: read the calendar when due, and
/// prepare the next meeting that has no prep yet (one per pass, so the
/// requests stay few).
pub fn tick(app: &AppHandle) {
    let store = app.state::<Arc<Store>>().inner().clone();
    let now = now_ms();
    let read_at: i64 = store.get("calendar_read_at").and_then(|v| v.parse().ok()).unwrap_or(0);
    if now - read_at >= CALENDAR_EVERY_MS {
        match refresh_calendar(&store) {
            Ok(n) if n > 0 => {
                let _ = app.emit("meetings-upcoming-changed", ());
            }
            Ok(_) => {}
            Err(e) => log(&format!("the calendar could not be read: {e}")),
        }
    }
    if !store.get_bool(settings::PRE_MEETING_NOTES, true) || settings::key(&store, settings::OPENAI_KEY).is_none() {
        return;
    }
    let next = store.upcoming_events(now, 20).into_iter().find(|e| e.starts_at > now - 5 * 60_000 && e.starts_at - now <= AHEAD_MS && load(&store, &e.id).is_none());
    let Some(event) = next else { return };
    match build(&store, &event.id, false) {
        Ok(prep) => {
            let _ = app.emit("meeting-prep-changed", event.id.clone());
            if event.starts_at - now <= ANNOUNCE_WITHIN_MS && !prep.thin {
                notify(app, &format!("Ready for {}", event.title), "Your notes for this meeting are ready. Open them before the call.");
            }
        }
        Err(e) => log(&format!("a meeting could not be prepared: {e}")),
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Starts the background work: soon after launch, then every five minutes.
pub fn watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("odm-prep".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            loop {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(&app)));
                std::thread::sleep(std::time::Duration::from_secs(5 * 60));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> EventRow {
        EventRow {
            id: "e1".into(),
            title: "Intro with Sam".into(),
            starts_at: 1_900_000_000_000,
            ends_at: Some(1_900_000_000_000 + 1_800_000),
            description: "Talk about the pilot".into(),
            attendees: vec!["Ada Lovelace".into(), "Sam Rivera".into()],
            link: Some("https://meet.google.com/abc-defg-hij".into()),
            source: "calendar".into(),
            ..Default::default()
        }
    }

    #[test]
    fn you_are_not_among_the_people_prepared_for() {
        assert_eq!(people_for(&event(), None, "Ada"), vec!["Sam Rivera".to_string()]);
        let prior = MeetingPrep { participants: vec!["Lee".into()], ..Default::default() };
        assert_eq!(people_for(&event(), Some(&prior), "Ada"), vec!["Lee".to_string()], "what you set wins");
    }

    #[test]
    fn the_material_carries_answers_drops_and_your_own_points() {
        let prior = MeetingPrep {
            questions: vec![PrepQuestion { question: "What do you want?".into(), answer: Some("A pilot".into()) }, PrepQuestion { question: "Budget?".into(), answer: Some(String::new()) }],
            predictions: vec![Prediction { point: "Pricing".into(), source: READER_SOURCE.into(), ..Default::default() }],
            dropped: vec!["Hiring".into()],
            ..Default::default()
        };
        let m = material(&event(), &["Sam Rivera".into()], "I run a small studio.", "Ada", Some("Sam runs a design firm."), Some((1_800_000_000_000, "Agreed to a demo.".into())), Some(&prior));
        for want in ["Title: Intro with Sam", "People: Sam Rivera", "Call link: https://meet.google.com", "I run a small studio.", "## RESEARCH", "## LAST MEETING WITH THEM", "A: A pilot", "- Budget?", "- Pricing", "- Hiring"] {
            assert!(m.contains(want), "missing {want:?} in:\n{m}");
        }
    }

    #[test]
    fn a_reply_keeps_answers_and_your_points_and_leaves_out_drops() {
        let prior = MeetingPrep {
            questions: vec![PrepQuestion { question: "What do you want?".into(), answer: Some("A pilot".into()) }],
            predictions: vec![Prediction { point: "My point".into(), source: READER_SOURCE.into(), ..Default::default() }],
            dropped: vec!["Hiring".into()],
            ..Default::default()
        };
        let raw = r#"{"brief":"You meet Sam.","agenda":"A pilot.","thin":false,"questions":["What do you want?","Who decides?"],
            "predictions":[{"point":"Hiring","why":"x","source":"y"},{"point":"Timeline","why":"They asked","source":"Talk about the pilot","say":"We can start in May."},{"point":"No source","why":"x","source":""}],"last_time":null}"#;
        let p = from_reply(raw, "e1", vec!["Sam".into()], Some(&prior));
        assert_eq!(p.brief, "You meet Sam.");
        assert_eq!(p.questions[0].answer.as_deref(), Some("A pilot"));
        assert_eq!(p.questions.len(), 2);
        let points: Vec<&str> = p.predictions.iter().map(|x| x.point.as_str()).collect();
        assert_eq!(points, vec!["My point", "Timeline"], "your point first, the dropped and unsourced ones gone");
        assert_eq!(p.predictions[1].say.as_deref(), Some("We can start in May."));
        assert_eq!(p.dropped, vec!["Hiring".to_string()]);
    }

    #[test]
    fn a_thin_reply_asks_nothing_and_predicts_nothing() {
        let p = from_reply(r#"{"brief":"x","agenda":"","thin":true,"questions":["q"],"predictions":[{"point":"p","why":"w","source":"s"}]}"#, "e", vec![], None);
        assert!(p.thin && p.questions.is_empty() && p.predictions.is_empty());
    }

    #[test]
    fn dropping_and_adding_points_needs_no_request() {
        let store = Store::open_in_memory();
        store.upsert_event(&event());
        let p = add_prediction(&store, "e1", "Pricing").unwrap();
        assert_eq!(p.predictions[0].source, READER_SOURCE);
        let p = drop_prediction(&store, "e1", 0).unwrap();
        assert!(p.predictions.is_empty());
        assert_eq!(p.dropped, vec!["Pricing".to_string()]);
        let p = add_prediction(&store, "e1", "pricing").unwrap();
        assert!(p.dropped.is_empty(), "adding it back takes it off the dropped list");
    }

    #[test]
    fn the_last_call_with_them_is_found_in_your_notes() {
        let store = Store::open_in_memory();
        store.insert_meeting(&crate::store::MeetingRow { id: "m".into(), service: "Zoom".into(), participants: vec!["Sam Rivera".into()], started_at: 5, ..Default::default() });
        assert!(last_meeting_with(&store, &["Sam Rivera".into()]).is_none(), "no notes yet");
        store.set_meeting_summary("m", "Agreed to a demo.");
        assert_eq!(last_meeting_with(&store, &["Sam Rivera".into()]).map(|(_, s)| s), Some("Agreed to a demo.".into()));
        assert!(last_meeting_with(&store, &["Lee".into()]).is_none());
    }

    #[test]
    fn the_prompts_have_no_dashes() {
        for p in [PREP_SYSTEM, RESEARCH_SYSTEM] {
            assert!(!p.contains('\u{2014}') && !p.contains('\u{2013}'));
        }
    }
}
