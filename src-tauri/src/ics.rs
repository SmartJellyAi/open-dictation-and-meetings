//! Meetings to come, read from a calendar's private address in iCal
//! format (Google Calendar, Outlook and iCloud all offer one). No account
//! and no sign-in: the feed is fetched with a plain request.
//!
//! What is understood: one-off events, daily and weekly repeats (with
//! INTERVAL, COUNT, UNTIL, BYDAY and EXDATE), UTC times, all-day dates,
//! and times with a TZID, which are read as this computer's local time.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};

use crate::store::EventRow;

/// How far ahead the feed is read.
pub const AHEAD_DAYS: i64 = 14;

/// Lines of the file, with folded lines (a continuation starts with a
/// space or tab) joined back up.
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

/// "DTSTART;TZID=Europe/London" and its value, split.
fn split_prop(line: &str) -> Option<(String, Vec<(String, String)>, String)> {
    let (head, value) = line.split_once(':')?;
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts.filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))).collect();
    Some((name, params, value.to_string()))
}

fn unescape(v: &str) -> String {
    v.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// A date or date-time value as ms since the epoch, and whether it was a
/// whole day.
fn parse_time(value: &str, params: &[(String, String)]) -> Option<(i64, bool)> {
    let v = value.trim();
    let is_date = params.iter().any(|(k, v)| k == "VALUE" && v.eq_ignore_ascii_case("DATE")) || v.len() == 8;
    if is_date {
        let d = NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok()?;
        let local = Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?;
        return Some((local.timestamp_millis(), true));
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&t).timestamp_millis(), false));
    }
    let t = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    Some((Local.from_local_datetime(&t).earliest()?.timestamp_millis(), false))
}

/// A person on the invite, by name when the invite gives one.
fn attendee_name(params: &[(String, String)], value: &str) -> Option<String> {
    if let Some((_, cn)) = params.iter().find(|(k, _)| k == "CN") {
        let cn = cn.trim();
        if !cn.is_empty() && !cn.contains('@') {
            return Some(cn.to_string());
        }
    }
    let email = value.trim().trim_start_matches("mailto:").trim_start_matches("MAILTO:");
    let local = email.split('@').next().unwrap_or("").trim();
    if local.is_empty() {
        return None;
    }
    Some(local.replace(['.', '_'], " ").split_whitespace().map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" "))
}

/// The first call link in a text: Meet, Zoom, Teams or Webex.
pub fn call_link(text: &str) -> Option<String> {
    const HOSTS: &[&str] = &["meet.google.com/", "zoom.us/", "teams.microsoft.com/", "teams.live.com/", "webex.com/", "whereby.com/"];
    for word in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"' || c == '(' || c == ')') {
        let w = word.trim_matches(|c: char| c == ',' || c == '.' || c == ';');
        if HOSTS.iter().any(|h| w.contains(h)) {
            let url = if w.starts_with("http") { w.to_string() } else { format!("https://{w}") };
            return Some(url);
        }
    }
    None
}

#[derive(Default, Clone)]
struct Raw {
    uid: String,
    summary: String,
    description: String,
    location: String,
    url: Option<String>,
    start: Option<(i64, bool)>,
    end: Option<(i64, bool)>,
    rrule: Option<String>,
    exdates: Vec<i64>,
    attendees: Vec<String>,
    cancelled: bool,
    recurrence_id: Option<i64>,
}

/// Every meeting in the feed between `from` and `from + AHEAD_DAYS`, with
/// repeats expanded. All-day events are left out: they are not calls.
pub fn parse(text: &str, from: i64) -> Vec<EventRow> {
    let until = from + AHEAD_DAYS * 86_400_000;
    let mut raws: Vec<Raw> = Vec::new();
    let mut cur: Option<Raw> = None;
    for line in unfold(text) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(Raw::default());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(r) = cur.take() {
                raws.push(r);
            }
            continue;
        }
        let Some(r) = cur.as_mut() else { continue };
        let Some((name, params, value)) = split_prop(&line) else { continue };
        match name.as_str() {
            "UID" => r.uid = value,
            "SUMMARY" => r.summary = unescape(&value),
            "DESCRIPTION" => r.description = unescape(&value),
            "LOCATION" => r.location = unescape(&value),
            "URL" => r.url = Some(value),
            "DTSTART" => r.start = parse_time(&value, &params),
            "DTEND" => r.end = parse_time(&value, &params),
            "RRULE" => r.rrule = Some(value),
            "EXDATE" => r.exdates.extend(value.split(',').filter_map(|v| parse_time(v, &params)).map(|(t, _)| t)),
            "STATUS" => r.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            "RECURRENCE-ID" => r.recurrence_id = parse_time(&value, &params).map(|(t, _)| t),
            "ATTENDEE" | "ORGANIZER" => {
                if let Some(n) = attendee_name(&params, &value) {
                    if !r.attendees.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
                        r.attendees.push(n);
                    }
                }
            }
            _ => {}
        }
    }
    // A moved or cancelled single occurrence replaces that one occurrence.
    let overrides: Vec<(String, i64)> = raws.iter().filter_map(|r| r.recurrence_id.map(|t| (r.uid.clone(), t))).collect();
    let mut out: Vec<EventRow> = Vec::new();
    for r in &raws {
        let Some((start, all_day)) = r.start else { continue };
        if all_day || r.cancelled {
            continue;
        }
        let length = r.end.map(|(e, _)| e - start).filter(|l| *l > 0).unwrap_or(3_600_000);
        let starts: Vec<i64> = match (&r.rrule, r.recurrence_id) {
            (Some(rule), None) => expand(start, rule, from - length, until),
            _ => vec![start],
        };
        for s in starts {
            if s + length < from || s > until || r.exdates.contains(&s) {
                continue;
            }
            if r.recurrence_id.is_none() && r.rrule.is_some() && overrides.iter().any(|(u, t)| *u == r.uid && *t == s) {
                continue;
            }
            let text = format!("{} {} {}", r.location, r.url.clone().unwrap_or_default(), r.description);
            out.push(EventRow {
                id: format!("cal:{}:{s}", r.uid),
                title: if r.summary.trim().is_empty() { "Meeting".into() } else { r.summary.trim().to_string() },
                starts_at: s,
                ends_at: Some(s + length),
                location: r.location.clone(),
                description: r.description.chars().take(4_000).collect(),
                attendees: r.attendees.clone(),
                link: call_link(&text),
                source: "calendar".into(),
            });
        }
    }
    out.sort_by_key(|e| e.starts_at);
    out
}

fn weekday(code: &str) -> Option<Weekday> {
    // "MO", or "1MO" / "-1FR" for monthly rules, which are not expanded.
    Some(match code.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit()) {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// The start times of a repeating event inside [from, until]. Daily and
/// weekly rules only; any other rule gives the first start alone.
fn expand(start: i64, rule: &str, from: i64, until: i64) -> Vec<i64> {
    let mut freq = "";
    let mut interval: i64 = 1;
    let mut count: Option<usize> = None;
    let mut rule_until: Option<i64> = None;
    let mut byday: Vec<Weekday> = Vec::new();
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = if v.eq_ignore_ascii_case("DAILY") { "daily" } else if v.eq_ignore_ascii_case("WEEKLY") { "weekly" } else { "other" },
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => rule_until = parse_time(v, &[]).map(|(t, _)| t),
            "BYDAY" => byday = v.split(',').filter_map(weekday).collect(),
            _ => {}
        }
    }
    let Some(first) = Local.timestamp_millis_opt(start).single() else { return vec![start] };
    let last = rule_until.map(|u| u.min(until)).unwrap_or(until);
    let mut out = Vec::new();
    let mut n = 0usize;
    // Walk day by day from the first occurrence; a year of days at most.
    for day in 0..(366 * interval.max(1)) {
        let candidate: DateTime<Local> = match Local.from_local_datetime(&(first.naive_local() + Duration::days(day))).earliest() {
            Some(c) => c,
            None => continue,
        };
        let t = candidate.timestamp_millis();
        if t > last {
            break;
        }
        let hit = match freq {
            "daily" => day % interval == 0,
            "weekly" => {
                let week = day / 7;
                let days: Vec<Weekday> = if byday.is_empty() { vec![first.weekday()] } else { byday.clone() };
                week % interval == 0 && days.contains(&candidate.weekday())
            }
            _ => day == 0,
        };
        if !hit {
            continue;
        }
        n += 1;
        if count.is_some_and(|c| n > c) {
            break;
        }
        if t >= from {
            out.push(t);
        }
        if freq == "other" {
            break;
        }
    }
    out
}

/// Fetches the feed. Accepts `webcal://` as the calendars hand it out.
pub fn fetch(url: &str) -> Result<String, String> {
    let url = url.trim();
    let url = url.strip_prefix("webcal://").map(|r| format!("https://{r}")).unwrap_or_else(|| url.to_string());
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("That calendar address does not look right. It should start with https:// or webcal://.".into());
    }
    crate::rt::block_on(async move {
        let resp = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| "Could not start a connection.".to_string())?
            .get(&url)
            .send()
            .await
            .map_err(|_| "The calendar could not be reached. Check your internet connection.".to_string())?;
        if !resp.status().is_success() {
            return Err("The calendar address was refused. Copy the private address in iCal format again.".into());
        }
        let text = resp.text().await.map_err(|_| "The calendar could not be read.".to_string())?;
        if !text.contains("BEGIN:VCALENDAR") {
            return Err("That address is not a calendar feed. Use the private address in iCal format.".into());
        }
        Ok(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap().timestamp_millis()
    }

    const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:one@example\r\nDTSTART:20300105T150000Z\r\nDTEND:20300105T153000Z\r\nSUMMARY:Intro call with Sam\r\nDESCRIPTION:Join: https://meet.google.com/abc-defg-hij\\nAgenda: pricing\r\nATTENDEE;CN=Sam Rivera;ROLE=REQ-PARTICIPANT:mailto:sam@example.com\r\nATTENDEE:mailto:lee.park@example.com\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:allday@example\r\nDTSTART;VALUE=DATE:20300106\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:weekly@example\r\nDTSTART:20300107T100000Z\r\nDTEND:20300107T103000Z\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nSUMMARY:Weekly sync\r\nEXDATE:20300114T100000Z\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:gone@example\r\nDTSTART:20300108T100000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Cancelled\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_feed_becomes_meetings_with_people_and_a_link() {
        let events = parse(FEED, ms(2030, 1, 1, 0, 0));
        let first = &events[0];
        assert_eq!(first.title, "Intro call with Sam");
        assert_eq!(first.starts_at, ms(2030, 1, 5, 15, 0));
        assert_eq!(first.ends_at, Some(ms(2030, 1, 5, 15, 30)));
        assert_eq!(first.attendees, vec!["Sam Rivera".to_string(), "Lee Park".to_string()]);
        assert_eq!(first.link.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert!(first.description.contains("Agenda: pricing"));
        assert!(!events.iter().any(|e| e.title == "Holiday"), "an all-day event is not a call");
        assert!(!events.iter().any(|e| e.title == "Cancelled"));
    }

    #[test]
    fn a_weekly_repeat_is_expanded_without_its_skipped_week() {
        let events = parse(FEED, ms(2030, 1, 1, 0, 0));
        let weekly: Vec<i64> = events.iter().filter(|e| e.title == "Weekly sync").map(|e| e.starts_at).collect();
        assert_eq!(weekly, vec![ms(2030, 1, 7, 10, 0)], "COUNT=3 inside 14 days is the 7th and 14th; the 14th is skipped");
    }

    #[test]
    fn folded_lines_are_joined() {
        let lines = unfold("SUMMARY:Long\r\n  title\r\nUID:x");
        assert_eq!(lines[0], "SUMMARY:Long title");
    }

    #[test]
    fn a_meeting_already_over_is_left_out() {
        let events = parse(FEED, ms(2030, 1, 6, 0, 0));
        assert!(!events.iter().any(|e| e.title == "Intro call with Sam"));
    }

    #[test]
    fn a_call_link_is_found_in_text() {
        assert_eq!(call_link("Join at us02web.zoom.us/j/123456789."), Some("https://us02web.zoom.us/j/123456789".into()));
        assert_eq!(call_link("no link here"), None);
    }
}
