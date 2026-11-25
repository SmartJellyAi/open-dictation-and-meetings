import { useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { api, said } from "../../lib/api";
import type { SettingsView } from "../../types";
import { prettyKey } from "../Titlebar";
import { RowButton, SettingCard, SettingRow, StatusPill, Toggle } from "./SettingsRows";

type Tab = "general" | "dictation" | "meetings" | "prep";

const TABS: { key: Tab; label: string }[] = [
  { key: "general", label: "General" },
  { key: "dictation", label: "Dictation" },
  { key: "meetings", label: "Meeting notes" },
  { key: "prep", label: "Pre-meeting notes" },
];

/// Settings fills the page, with its parts as tabs.
export function Settings() {
  const [tab, setTab] = useState<Tab>("general");
  const [s, setS] = useState<SettingsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const read = () => void api.getSettings().then(setS).catch(() => {});
  useEffect(read, []);

  /// A change saved, then every page that shows it reads it again.
  const apply = (p: Promise<SettingsView>) => {
    setError(null);
    void p
      .then((v) => {
        setS(v);
        void emit("settings-changed");
      })
      .catch((e) => setError(said(e)));
  };
  const set = (key: string, value: boolean | string) => apply(api.setSetting(key, value));

  return (
    <div className="sp settings-page stg-page">
      <div className="stg-tabs" role="tablist" aria-label="Settings">
        {TABS.map((t) => (
          <button key={t.key} type="button" role="tab" aria-selected={tab === t.key} className={tab === t.key ? "is-active" : ""} onClick={() => setTab(t.key)}>
            {t.label}
          </button>
        ))}
      </div>
      <main className="sp-main">
        <div className="sp-page">
          <div className="settings-body">
            {error && <p className="note-error">{error}</p>}
            {s && tab === "general" && <General s={s} apply={apply} set={set} />}
            {s && tab === "dictation" && <Dictation s={s} apply={apply} set={set} />}
            {s && tab === "meetings" && <MeetingNotes s={s} set={set} />}
            {s && tab === "prep" && <PreMeeting s={s} set={set} onCalendar={read} />}
          </div>
        </div>
      </main>
    </div>
  );
}

type Part = { s: SettingsView; apply: (p: Promise<SettingsView>) => void; set: (key: string, value: boolean | string) => void };

/// One provider's key: saved here, checked against the provider, never
/// shown again in full.
function KeyRow({ provider, label, desc, hint, apply }: { provider: "openai" | "deepgram"; label: string; desc: string; hint: string | null; apply: Part["apply"] }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [check, setCheck] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const save = () => {
    setBusy(true);
    setCheck(null);
    void api
      .setApiKey(provider, draft)
      .then(async (v) => {
        apply(Promise.resolve(v));
        setEditing(false);
        setDraft("");
        try {
          await api.checkApiKey(provider);
          setCheck({ ok: true, text: "Saved and checked." });
        } catch (e) {
          setCheck({ ok: false, text: said(e) });
        }
      })
      .catch((e) => setCheck({ ok: false, text: said(e) }))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <SettingRow
        label={label}
        desc={
          <>
            {desc} {hint ? <code>{hint}</code> : "Not set."}
            {check && <span className={check.ok ? "vn-ok" : "own-error"}> {check.text}</span>}
          </>
        }
        control={
          <span className="own-actions">
            <RowButton onClick={() => { setEditing(true); setDraft(""); setCheck(null); }}>{hint ? "Replace" : "Add key"}</RowButton>
            {hint && <RowButton onClick={() => { setCheck(null); apply(api.setApiKey(provider, "")); }}>Remove</RowButton>}
          </span>
        }
      />
      {editing && (
        <div className="set-row set-row-inline">
          <input
            type="password"
            className="set-input"
            placeholder={provider === "openai" ? "sk-…" : "Deepgram API key"}
            value={draft}
            autoFocus
            autoComplete="off"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && draft.trim()) save();
            }}
          />
          <RowButton onClick={() => draft.trim() && save()}>{busy ? "Checking…" : "Save and check"}</RowButton>
          <RowButton onClick={() => setEditing(false)}>Cancel</RowButton>
        </div>
      )}
    </>
  );
}

function General({ s, apply, set }: Part) {
  const [mics, setMics] = useState<string[]>([]);
  const [name, setName] = useState(s.your_name);
  useEffect(() => {
    void api.listMicrophones().then(setMics).catch(() => {});
  }, []);
  const mac = s.platform.os === "macos";
  return (
    <>
      <SettingCard title="Your keys">
        <p className="set-card-sub">This app talks to the providers directly with your own keys. They are kept in this app's database on this computer and sent only to their provider.</p>
        <KeyRow provider="deepgram" label="Deepgram" desc="Live words while you dictate and during calls." hint={s.deepgram_key_hint} apply={apply} />
        <KeyRow provider="openai" label="OpenAI" desc="Meeting notes, pre-meeting notes and tidying dictation. Also writes calls down when there is no Deepgram key." hint={s.openai_key_hint} apply={apply} />
      </SettingCard>
      <SettingCard title="You">
        <SettingRow
          label="Your name"
          desc="So notes say what you agreed to, and you are never listed among the people you meet."
          control={<input type="text" className="set-input" value={name} placeholder="Your name" onChange={(e) => setName(e.target.value)} onBlur={() => name !== s.your_name && set("your_name", name.trim())} />}
        />
      </SettingCard>
      <SettingCard title="Microphone">
        <SettingRow
          label="Microphone"
          desc="Used for dictation and for your side of a call."
          control={
            <select className="set-select" aria-label="Microphone" value={s.mic_device} onChange={(e) => set("mic_device", e.target.value)}>
              <option value="">System default</option>
              {mics.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
              {s.mic_device && !mics.includes(s.mic_device) && <option value={s.mic_device}>{s.mic_device} (not connected)</option>}
            </select>
          }
        />
      </SettingCard>
      {(mac || s.platform.os === "windows") && (
        <SettingCard title="Permissions">
          <SettingRow
            label="Microphone"
            desc="The system asks the first time you dictate or take notes. If you said no, allow it in the system's settings."
            control={<RowButton onClick={() => void api.openPrivacySettings("microphone")}>Open</RowButton>}
          />
          {mac && (
            <>
              <SettingRow
                label="Accessibility"
                desc="Lets dictation type into other apps, and lets the app see which meeting window is in front."
                control={
                  s.platform.needs_accessibility ? (
                    <RowButton onClick={() => void api.requestAccessibility().then(() => api.openPrivacySettings("accessibility"))}>Allow</RowButton>
                  ) : (
                    <StatusPill tone="good">Allowed</StatusPill>
                  )
                }
              />
              <SettingRow
                label="System audio recording"
                desc="Lets meeting notes hear the other people on a call. macOS asks the first time notes start."
                control={<RowButton onClick={() => void api.openPrivacySettings("audio")}>Open</RowButton>}
              />
            </>
          )}
        </SettingCard>
      )}
    </>
  );
}


/// A key pressed on the keyboard, as a shortcut ("Alt+Shift+D"). Null
/// while only modifiers are down. A key without a modifier must be a
/// function key, or ordinary typing would start dictation.
function shortcutFrom(e: KeyboardEvent): string | null {
  const code = e.code;
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) key = code;
  else if (code === "Space") key = "Space";
  if (!key) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (mods.length === 0 && !/^F\d+$/.test(key)) return null;
  return [...mods, key].join("+");
}

function Dictation({ s, apply, set }: Part) {
  const [recording, setRecording] = useState(false);
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const k = shortcutFrom(e);
      if (!k) return;
      setRecording(false);
      apply(api.setDictationKey(k));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [recording, apply]);
  return (
    <SettingCard title="Dictation">
      <SettingRow label="Dictation" desc="Talk, and the words are typed into the app you are in." control={<Toggle on={s.dictation_on} label="Dictation" onChange={(v) => set("dictation_on", v)} />} />
      {s.dictation_on && (
        <>
          <SettingRow
            label="Dictation key"
            desc={recording ? "Press the keys you want, with at least one of Ctrl, Alt, Shift or Command. Esc cancels." : "Works in any app."}
            control={
              <span className="own-actions">
                <span className="vn-key">{prettyKey(s.dictation_key)}</span>
                <RowButton onClick={() => setRecording((r) => !r)}>{recording ? "Press the keys…" : "Change"}</RowButton>
              </span>
            }
          />
          <SettingRow
            label="How the key works"
            desc={s.dictation_mode === "hold" ? "Hold the key while you talk; the words are typed when you let go." : "Press to start, press again to stop."}
            control={
              <div className="set-seg" role="radiogroup" aria-label="How the key works">
                {(
                  [
                    ["toggle", "Press to start"],
                    ["hold", "Hold to talk"],
                  ] as const
                ).map(([v, label]) => (
                  <button key={v} type="button" role="radio" aria-checked={s.dictation_mode === v} className={`set-seg-item${s.dictation_mode === v ? " is-on" : ""}`} onClick={() => set("dictation_mode", v)}>
                    {label}
                  </button>
                ))}
              </div>
            }
          />
          <SettingRow
            label="Tidy before typing"
            desc="Fixes misheard words, drops fillers and false starts, and follows spoken commands like new line. Needs an OpenAI key."
            control={<Toggle on={s.dictation_cleanup} label="Tidy before typing" onChange={(v) => set("dictation_cleanup", v)} />}
          />
          {!s.platform.typing && <p className="own-refused">Typing into other apps is not available here. Install xdotool (X11) or wtype (Wayland); until then the words stay in the pill to copy.</p>}
          {s.platform.needs_accessibility && <p className="own-refused">Allow Accessibility in General, Permissions, so the words can be typed into other apps.</p>}
        </>
      )}
    </SettingCard>
  );
}

function MeetingNotes({ s, set }: { s: SettingsView; set: Part["set"] }) {
  return (
    <SettingCard title="Meeting notes">
      <SettingRow label="Meeting notetaker" desc="A live transcript of your calls, and notes written when each call ends." control={<Toggle on={s.notetaker_on} label="Meeting notetaker" onChange={(v) => set("notetaker_on", v)} />} />
      {s.notetaker_on && (
        <SettingRow
          label="Notice calls"
          desc={
            s.platform.meeting_detection
              ? "When a call starts in Zoom, Google Meet, Teams or another meeting app, the pill asks whether to take notes."
              : "Noticing calls is not available on this system yet. Start notes from the Meetings page or the tray."
          }
          control={<Toggle on={s.meeting_detection} label="Notice calls" disabled={!s.platform.meeting_detection} onChange={(v) => set("meeting_detection", v)} />}
        />
      )}
      {!s.platform.system_audio && <p className="own-refused">On this system only your microphone is heard. With speakers on, it picks up the others too, less clearly.</p>}
    </SettingCard>
  );
}

function PreMeeting({ s, set, onCalendar }: { s: SettingsView; set: Part["set"]; onCalendar: () => void }) {
  const [url, setUrl] = useState("");
  const [about, setAbout] = useState(s.about_you);
  const [calendar, setCalendar] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const saveCalendar = (value: string) => {
    setBusy(true);
    setCalendar(null);
    void api
      .setCalendarUrl(value)
      .then((n) => {
        setUrl("");
        setCalendar({ ok: true, text: value.trim() ? (n === 1 ? "Saved. 1 meeting ahead." : `Saved. ${n} meetings ahead.`) : "Removed." });
        onCalendar();
      })
      .catch((e) => setCalendar({ ok: false, text: said(e) }))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <SettingCard title="Pre-meeting notes">
        <SettingRow label="Pre-meeting notes" desc="Before each meeting: who you are meeting, what will probably come up, and what to have ready." control={<Toggle on={s.pre_meeting_notes} label="Pre-meeting notes" onChange={(v) => set("pre_meeting_notes", v)} />} />
        {s.pre_meeting_notes && (
          <SettingRow label="Look people up on the web" desc="Who they are and what their company does, with the pages it came from." control={<Toggle on={s.web_research} label="Look people up on the web" onChange={(v) => set("web_research", v)} />} />
        )}
      </SettingCard>
      <SettingCard title="Calendar">
        <p className="set-card-sub">
          Paste your calendar's private address in iCal format. In Google Calendar it is in the calendar's settings, under Integrate calendar; Outlook and iCloud offer one when you share or publish a calendar. Or add meetings by hand on the Upcoming page.
        </p>
        <SettingRow
          label="Calendar address"
          desc={
            <>
              {s.calendar_url_set ? "A calendar is connected." : "No calendar yet."}
              {calendar && <span className={calendar.ok ? "vn-ok" : "own-error"}> {calendar.text}</span>}
            </>
          }
          control={s.calendar_url_set ? <RowButton onClick={() => saveCalendar("")}>Remove</RowButton> : <span />}
        />
        <div className="set-row set-row-inline">
          <input type="password" className="set-input" placeholder="https://… .ics" value={url} autoComplete="off" onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => e.key === "Enter" && url.trim() && saveCalendar(url)} />
          <RowButton onClick={() => url.trim() && saveCalendar(url)}>{busy ? "Reading…" : s.calendar_url_set ? "Replace" : "Save"}</RowButton>
        </div>
      </SettingCard>
      <SettingCard title="About your work">
        <p className="set-card-sub">A few lines on what you do and what you are working on, so the notes before a meeting can say why it matters to you.</p>
        <textarea className="set-input vn-textarea" value={about} placeholder="I run a small design studio. This quarter we are looking for two new clients in retail." onChange={(e) => setAbout(e.target.value)} onBlur={() => about !== s.about_you && set("about_you", about)} />
      </SettingCard>
    </>
  );
}
