//! Noticing a call: which meeting app or page is in front.
//!
//! Detection is by what is visibly running, never by listening: the app
//! must not start hearing a room in order to work out whether it is in a
//! meeting.
//!
//! The meeting code is the signal, not the brand name. Every service
//! issues one (Meet's `abc-defg-hij`, Zoom's 9 to 11 digits, Webex's
//! `MTID`, Teams' `meetup-join`), and a page carrying a code is a call,
//! while a page carrying only the brand is a home page or an email about a
//! meeting. URL shapes follow Recall.ai's published list of meeting URL
//! formats.

/// Splits a title into candidate tokens: runs of letters, digits, hyphens
/// and underscores. Everything else is a separator, so a code is found the
/// same way in a URL and in a bare window title.
fn tokens(lower_title: &str) -> impl Iterator<Item = &str> {
    lower_title
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .filter(|t| !t.is_empty())
}

/// The event's name in a Meet tab's title, when the call has one: Meet
/// titles a calendar call "Meet – Ada Lovelace and Alan Turing", and the
/// browser adds "– Camera and microphone recording" while it is live. None
/// for a bare code ("Meet - abc-defg-hij").
pub fn meet_event_name(title: &str) -> Option<String> {
    let t = title.trim();
    let rest = t.strip_prefix("Meet – ").or_else(|| t.strip_prefix("Meet - "))?;
    let name = rest.split(" – ").next()?.split(" - ").next()?.trim();
    if name.is_empty() || has_meet_code(&name.to_lowercase()) {
        return None;
    }
    Some(name.to_string())
}

/// The people an event name lists, when it is nothing but names ("A and
/// B", "A, B and C"): the default name a calendar gives a call between
/// people. Anything else ("Weekly sync") gives nobody.
pub fn people_in_event_name(name: &str) -> Vec<String> {
    let parts: Vec<String> = name
        .replace(" & ", " and ")
        .split(" and ")
        .flat_map(|p| p.split(", ").map(str::to_string).collect::<Vec<_>>())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let is_name = |s: &str| {
        let words: Vec<&str> = s.split_whitespace().collect();
        (1..=4).contains(&words.len())
            && words.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase()) && w.chars().all(|c| c.is_alphabetic() || matches!(c, '.' | '\'' | '-')))
    };
    if parts.len() >= 2 && parts.iter().all(|p| is_name(p)) { parts } else { Vec::new() }
}

fn has_meet_code(lower_title: &str) -> bool {
    tokens(lower_title).any(|t| {
        let g: Vec<&str> = t.split('-').collect();
        g.len() == 3
            && [g[0].len(), g[1].len(), g[2].len()] == [3, 4, 3]
            && g.iter().all(|p| p.chars().all(|c| c.is_ascii_lowercase()))
    })
}

/// Zoom meeting IDs are 9 to 11 digits, usually shown grouped in threes
/// and fours (`836 5462 7214`) but unbroken inside a URL.
fn has_zoom_id(lower_title: &str) -> bool {
    tokens(lower_title).any(|t| (9..=11).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether a meeting is on screen, and which service. Browser cases are
/// matched on the window title, because the app is only ever the browser
/// and the meeting is a tab inside it.
pub fn meeting_on_screen(app_name: &str, window_title: Option<&str>) -> Option<&'static str> {
    let title = window_title.unwrap_or("").to_lowercase();

    // -- Native clients -------------------------------------------------
    //
    // Being frontmost in the app is NOT being in a call: Zoom Workplace
    // and Teams both sit open all day showing chat, contacts and settings.
    // So a native client still has to show a call in its title.
    match app_name {
        "zoom.us" | "Zoom" | "Zoom Workplace" => {
            if title.contains("meeting") || title.contains("webinar") || has_zoom_id(&title) {
                return Some("Zoom");
            }
        }
        "Microsoft Teams" | "Microsoft Teams (work or school)" | "Teams" => {
            if title.contains("meeting") || title.contains("call with") {
                return Some("Microsoft Teams");
            }
        }
        "Webex" | "Cisco Webex Meetings" | "Webex Meetings" => {
            if title.contains("meeting") || title.contains("webex meeting") {
                return Some("Webex");
            }
        }
        // Huddles and Discord voice have no code and no distinguishing
        // title that I have a real sample of. Left out on purpose --
        // guessing a rule is exactly what broke Meet.
        _ => {}
    }

    if title.is_empty() {
        return None;
    }

    // -- Screen sharing -------------------------------------------------
    // "meet.google.com is sharing your screen." -- the title Meet gives
    // the window WHILE SHARING, with the meeting code gone entirely.
    // Sharing a screen is only possible from inside a call, so this is
    // proof of one. Found live 2026-09-17: an hour-long call went
    // undetected from the moment sharing started, because every rule here
    // was looking for a code that was no longer in the title.
    //
    // Deliberately not the "Choose what to share with ..." picker, which
    // is a dialog that can be open without sharing having begun.
    if title.contains("is sharing your screen") {
        for (host, service) in [
            ("meet.google.com", "Google Meet"),
            ("zoom", "Zoom"),
            ("teams", "Microsoft Teams"),
            ("webex", "Webex"),
        ] {
            if title.contains(host) {
                return Some(service);
            }
        }
    }

    // -- Google Meet ----------------------------------------------------
    // meet.google.com/xxx-xxxx-xxx, or the bare code once in the call.
    if (title.contains("meet.google.com/") || title.starts_with("meet")) && has_meet_code(&title) {
        return Some("Google Meet");
    }
    // A calendar call's tab is named after the event, with no code
    // ("Meet – Ada Lovelace and Alan Turing"); Chrome marks it
    // "recording" while the call has the camera or microphone. Live
    // 2026-10-17: an hour-long call was found only by the microphone and
    // saved as "Google Chrome", with nobody on it.
    if (title.starts_with("meet – ") || title.starts_with("meet - ")) && title.contains("recording") {
        return Some("Google Meet");
    }

    // -- Zoom -----------------------------------------------------------
    // /j/ join, /s/ start, /wc/ web client, /w/ webinar, /my/ personal
    // room; zoomgov.com for government tenants.
    for path in ["zoom.us/j/", "zoom.us/s/", "zoom.us/wc/", "zoom.us/w/", "zoom.us/my/",
                 "zoom.com/wc/join/", "zoomgov.com/j/"] {
        if title.contains(path) {
            return Some("Zoom");
        }
    }
    // The web client sets the tab title to exactly "Zoom Meeting". Matched
    // as the whole title, never as a substring: an email whose subject is
    // "Please join Zoom meeting in progress" gives Gmail a tab title
    // containing that phrase, and reading an invitation is not attending.
    if title.trim() == "zoom meeting" || title.trim() == "zoom workplace meeting" {
        return Some("Zoom");
    }

    // -- Microsoft Teams ------------------------------------------------
    for path in ["teams.microsoft.com/l/meetup-join/", "teams.microsoft.com/meet/",
                 "teams.live.com/meet/", "teams.microsoft.com/light-meetings/launch"] {
        if title.contains(path) {
            return Some("Microsoft Teams");
        }
    }

    // -- Webex ----------------------------------------------------------
    // The classic form carries MTID; personal rooms use /meet/ or /join/.
    if title.contains("webex.com/")
        && (title.contains("mtid=") || title.contains("/meet/") || title.contains("/join/"))
    {
        return Some("Webex");
    }

    // -- GoTo -----------------------------------------------------------
    for path in ["gotomeet.me/", "app.goto.com/meeting/", "meet.goto.com/",
                 "global.gotomeeting.com/join/"] {
        if title.contains(path) {
            return Some("GoTo Meeting");
        }
    }

    // -- Smaller services -----------------------------------------------
    // Each needs a room segment after the host, so the marketing home page
    // does not count.
    for host in ["whereby.com/", "app.gather.town/", "meet.jit.si/", "app.around.co/"] {
        if let Some(rest) = title.split_once(host).map(|(_, r)| r) {
            if rest.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
                return Some("a meeting");
            }
        }
    }

    None
}

/// The meeting code inside a window title, if there is one.
pub fn code_in_title(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    let meet = tokens(&lower)
        .find(|t| {
            let g: Vec<&str> = t.split('-').collect();
            g.len() == 3 && [g[0].len(), g[1].len(), g[2].len()] == [3, 4, 3]
        })
        .map(str::to_string);
    if meet.is_some() {
        return meet;
    }
    // Bound before returning: the iterator borrows `lower`, and returning
    // the expression directly keeps that borrow alive past the function.
    let numeric = tokens(&lower)
        .find(|t| (9..=11).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_string);
    numeric
}

pub fn meeting_key(service: &str, window_title: Option<&str>) -> String {
    let title = window_title.unwrap_or("").to_lowercase();
    if let Some(code) = tokens(&title).find(|t| {
        let g: Vec<&str> = t.split('-').collect();
        g.len() == 3 && [g[0].len(), g[1].len(), g[2].len()] == [3, 4, 3]
    }) {
        return format!("{service}:{code}");
    }
    if let Some(id) = tokens(&title)
        .find(|t| (9..=11).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()))
    {
        return format!("{service}:{id}");
    }
    service.to_string()
}


/// Apps whose holding the microphone can mean a call: a browser (a call on
/// a site with no rule) and the apps that place calls. Anything else using
/// the microphone, a recorder or a settings meter, is not a meeting.
pub fn can_hold_a_call(app_name: &str) -> bool {
    const APPS: &[&str] = &[
        "Safari", "Google Chrome", "Arc", "Firefox", "Microsoft Edge", "Brave Browser", "Opera", "Vivaldi", "Chromium", "Orion", "Zen Browser", "Dia",
        "FaceTime", "Skype", "Slack", "Discord", "WhatsApp", "Telegram", "Signal", "Webex", "Cisco Webex Meetings", "Whereby", "Around", "Tuple",
    ];
    APPS.iter().any(|a| a.eq_ignore_ascii_case(app_name.trim()))
}

/// The meeting in front, if any: by its window, else (macOS) by the app in
/// front holding the microphone. Returns the service and whether it was
/// found by the microphone, which only ever offers and never starts.
pub fn call_in_front() -> Option<(String, Option<String>, bool)> {
    let (app, title) = crate::platform::front_window()?;
    if let Some(service) = meeting_on_screen(&app, title.as_deref()) {
        return Some((service.to_string(), title, false));
    }
    if can_hold_a_call(&app) && crate::platform::front_app_holds_microphone() {
        return Some((app, title, true));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meeting_is_recognised_without_hearing_anything() {
        // The en-dash matters: Meet uses one.
        for real in ["Meet \u{2013} qtd-frnw-xkb", "Meet \u{2013} zrn-kwpf-tqm", "meet.google.com/dkv-hbzr-nlt?ijlm=1788883333654&hs=187&adhoc=1"] {
            assert_eq!(meeting_on_screen("Arc", Some(real)), Some("Google Meet"), "missed a real meeting title: {real:?}");
        }
    }

    /// Sharing a screen renames the Meet window and drops the code.
    #[test]
    fn a_shared_screen_is_still_a_meeting() {
        assert_eq!(meeting_on_screen("Arc", Some("meet.google.com is sharing your screen.")), Some("Google Meet"));
        assert_eq!(meeting_on_screen("Google Chrome", Some("zoom.us is sharing your screen.")), Some("Zoom"));
    }

    #[test]
    fn the_share_picker_is_not_proof_of_a_call() {
        assert_eq!(meeting_on_screen("Arc", Some("Choose what to share with meet.google.com")), None);
    }

    #[test]
    fn every_documented_url_shape_is_recognised() {
        for (title, want) in [
            ("meet.google.com/abc-defg-hij", Some("Google Meet")),
            ("us05web.zoom.us/j/83654627214?pwd=abc", Some("Zoom")),
            ("zoom.us/s/9912345678/start", Some("Zoom")),
            ("company.zoom.us/wc/join/9912345678", Some("Zoom")),
            ("zoom.us/my/adalovelace", Some("Zoom")),
            ("zoomgov.com/j/1612345678", Some("Zoom")),
            ("Zoom Meeting", Some("Zoom")),
            ("teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2", Some("Microsoft Teams")),
            ("teams.live.com/meet/9312345678", Some("Microsoft Teams")),
            ("acme.webex.com/acme/j.php?MTID=m1234abcd", Some("Webex")),
            ("acme.webex.com/meet/ada", Some("Webex")),
            ("gotomeet.me/adalovelace", Some("GoTo Meeting")),
            ("global.gotomeeting.com/join/123456789", Some("GoTo Meeting")),
            ("whereby.com/team-standup", Some("a meeting")),
            ("meet.jit.si/DeliberateOtters", Some("a meeting")),
        ] {
            assert_eq!(meeting_on_screen("Arc", Some(title)), want, "{title:?}");
        }
    }

    /// A meeting client sitting open all day is not a call.
    #[test]
    fn a_meeting_client_merely_being_open_is_not_a_call() {
        assert_eq!(meeting_on_screen("zoom.us", None), None);
        assert_eq!(meeting_on_screen("Zoom Workplace", Some("Zoom Workplace")), None);
        assert_eq!(meeting_on_screen("zoom.us", Some("Settings")), None);
        assert_eq!(meeting_on_screen("Microsoft Teams", Some("Chat | Microsoft Teams")), None);
        assert_eq!(meeting_on_screen("zoom.us", Some("Zoom Meeting")), Some("Zoom"));
        assert_eq!(meeting_on_screen("zoom.us", Some("Meeting 836 5462 7214")), Some("Zoom"));
    }

    /// Reading about a meeting is not being in one.
    #[test]
    fn being_near_the_word_zoom_is_not_a_meeting() {
        for (app, title) in [
            ("Google Chrome", Some("Zoom: Video Conferencing - zoom.us")),
            ("Google Chrome", Some("Inbox - Gmail")),
            ("Arc", Some("Please join Zoom meeting in progress - Gmail")),
            ("Arc", Some("Fwd: Zoom meeting tomorrow - Gmail")),
            ("Arc", Some("Google Meet")),
            ("Arc", Some("Meet")),
            ("Arc", Some("meet.google.com/home")),
            ("Arc", Some("Pricing | Microsoft Teams")),
            ("Arc", Some("whereby.com")),
            ("Arc", Some("Video Meetings for Teams | Whereby")),
            ("Calendar", Some("Standup with Ada")),
            ("Slack", Some("general")),
            ("Code", Some("detect.rs")),
        ] {
            assert_eq!(meeting_on_screen(app, title), None, "{app} / {title:?} must not read as a live meeting");
        }
    }

    #[test]
    fn a_calendar_meet_tab_is_a_call_and_names_the_people_on_it() {
        let t = "Meet – Ada Lovelace and Alan Turing – Camera and microphone recording - High memory usage - 979 MB";
        assert_eq!(meeting_on_screen("Google Chrome", Some(t)), Some("Google Meet"));
        assert_eq!(meeting_on_screen("Google Chrome", Some("Meet – Ada Lovelace and Alan Turing")), None, "not live: no recording mark");
        let name = meet_event_name(t).unwrap();
        assert_eq!(name, "Ada Lovelace and Alan Turing");
        assert_eq!(people_in_event_name(&name), vec!["Ada Lovelace".to_string(), "Alan Turing".to_string()]);
        assert!(people_in_event_name("Weekly product sync").is_empty());
        assert!(people_in_event_name("Design review and Q4 planning").is_empty());
        assert_eq!(meet_event_name("Meet - abc-defg-hij"), None);
    }

    #[test]
    fn the_key_identifies_the_call_not_the_service() {
        assert_eq!(meeting_key("Google Meet", Some("Meet \u{2013} hsl-vmtc-pdz")), "Google Meet:hsl-vmtc-pdz");
        assert_ne!(meeting_key("Google Meet", Some("Meet \u{2013} hsl-vmtc-pdz")), meeting_key("Google Meet", Some("Meet \u{2013} qtd-frnw-xkb")));
        assert_eq!(meeting_key("Zoom", Some("us05web.zoom.us/j/83654627214?pwd=x")), "Zoom:83654627214");
        assert_eq!(meeting_key("Zoom", Some("Zoom Meeting")), "Zoom");
        assert_eq!(code_in_title("Meet - abc-defg-hij"), Some("abc-defg-hij".into()));
    }

    #[test]
    fn only_browsers_and_calling_apps_count_as_a_call_by_microphone() {
        assert!(can_hold_a_call("Google Chrome"));
        assert!(can_hold_a_call("FaceTime"));
        assert!(!can_hold_a_call("System Settings"));
        assert!(!can_hold_a_call("Voice Memos"));
    }
}
