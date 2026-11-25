import { useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, said } from "../../lib/api";
import type { MeetingNote, MeetingPrep, SettingsView, UpcomingMeeting } from "../../types";

/// Meetings to come, from your calendar or typed in by hand, and the notes
/// prepared before each.

function whenLabel(ts: number): string {
  const d = new Date(ts);
  const today = new Date();
  const tomorrow = new Date(Date.now() + 86_400_000);
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (d.toDateString() === today.toDateString()) return `Today, ${time}`;
  if (d.toDateString() === tomorrow.toDateString()) return `Tomorrow, ${time}`;
  return `${d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "short" })}, ${time}`;
}

function dayOf(ts: number): string {
  return whenLabel(ts).split(",")[0];
}

function markSeen(id: string) {
  try {
    const seen: string[] = JSON.parse(localStorage.getItem("prep-seen") ?? "[]");
    if (!seen.includes(id)) localStorage.setItem("prep-seen", JSON.stringify([...seen.slice(-200), id]));
  } catch {
    // Not remembered: the count stays until the next look.
  }
  void emit("prep-seen");
}

export default function Upcoming({ onOpenNote, onOpenSettings }: { onOpenNote: (id: string) => void; onOpenSettings: () => void }) {
  const [list, setList] = useState<UpcomingMeeting[]>([]);
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [reading, setReading] = useState<string | null>(null);

  const refresh = () => {
    api.upcomingMeetings().then(setList).catch(() => {});
    api.listMeetingNotes().then(setNotes).catch(() => {});
  };
  useEffect(() => {
    refresh();
    void api.getSettings().then(setSettings).catch(() => {});
    const subs = [listen("meeting-prep-changed", refresh), listen("meetings-upcoming-changed", refresh), listen("meeting-notes-changed", refresh)];
    return () => subs.forEach((p) => void p.then((f) => f()));
  }, []);

  const open = openId ? list.find((u) => u.event.id === openId) ?? null : null;
  if (open) {
    return (
      <div className="view notes-view mt-prep-scroll">
        <MeetingPrepPage meeting={open} notes={notes} onBack={() => setOpenId(null)} onOpenNote={onOpenNote} onChanged={refresh} />
      </div>
    );
  }

  const days: { label: string; items: UpcomingMeeting[] }[] = [];
  for (const u of list) {
    const label = dayOf(u.event.starts_at);
    const last = days[days.length - 1];
    if (last && last.label === label) last.items.push(u);
    else days.push({ label, items: [u] });
  }

  return (
    <div className="vn-upcoming">
      <div className="vn-upcoming-head">
        <p className="mt-fine">
          {settings?.calendar_url_set
            ? "From your calendar, read every 15 minutes. Notes are prepared before each meeting."
            : "Add your calendar in Settings, or add a meeting here, and notes are prepared before it."}
        </p>
        <div className="nc-q-buttons">
          {settings?.calendar_url_set ? (
            <button
              type="button"
              className="nc-btn"
              disabled={reading === "Reading…"}
              onClick={() => {
                setReading("Reading…");
                void api
                  .refreshCalendar()
                  .then((n) => setReading(n === 1 ? "1 meeting ahead" : `${n} meetings ahead`))
                  .catch((e) => setReading(said(e)));
              }}
            >
              {reading === "Reading…" ? "Reading…" : "Read the calendar now"}
            </button>
          ) : (
            <button type="button" className="nc-btn" onClick={onOpenSettings}>Add your calendar</button>
          )}
          <button type="button" className="nc-btn is-dark" onClick={() => setAdding(true)}>Add a meeting</button>
        </div>
        {reading && reading !== "Reading…" && <p className="mt-fine" role="status">{reading}</p>}
      </div>

      {adding && <AddMeeting onDone={() => { setAdding(false); refresh(); }} />}

      {list.length === 0 && !adding && (
        <div className="notes-empty">
          <h3>No meetings ahead</h3>
          <p>Meetings from your calendar, and ones you add, appear here with notes prepared before each.</p>
        </div>
      )}

      {days.map((d) => (
        <div className="mt-day" key={d.label}>
          <div className="mt-day-label">{d.label}</div>
          {d.items.map((u) => (
            <button
              key={u.event.id}
              type="button"
              className="mt-item vn-upcoming-item"
              onClick={() => {
                setOpenId(u.event.id);
                markSeen(u.event.id);
              }}
            >
              <span className="mt-item-title">{u.event.title}</span>
              <span className="mt-item-meta">
                {new Date(u.event.starts_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}
                {u.event.attendees.length > 0 ? ` · ${u.event.attendees.slice(0, 3).join(", ")}${u.event.attendees.length > 3 ? ` +${u.event.attendees.length - 3}` : ""}` : ""}
                {u.prep ? (u.prep.thin ? " · little to prepare from" : " · notes ready") : ""}
              </span>
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

/// A meeting typed in by hand.
function AddMeeting({ onDone }: { onDone: () => void }) {
  const soon = new Date(Date.now() + 60 * 60_000);
  soon.setMinutes(soon.getMinutes() < 30 ? 30 : 60, 0, 0);
  const local = new Date(soon.getTime() - soon.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
  const [title, setTitle] = useState("");
  const [when, setWhen] = useState(local);
  const [minutes, setMinutes] = useState(30);
  const [people, setPeople] = useState("");
  const [about, setAbout] = useState("");
  const [link, setLink] = useState("");
  const [error, setError] = useState<string | null>(null);
  const save = () => {
    setError(null);
    const startsAt = new Date(when).getTime();
    if (!title.trim()) return setError("Give the meeting a name.");
    if (Number.isNaN(startsAt)) return setError("Pick when it starts.");
    void api
      .addMeeting({ title, startsAt, minutes, attendees: people.split(",").map((p) => p.trim()).filter(Boolean), description: about, link: link.trim() || null })
      .then(onDone)
      .catch((e) => setError(said(e)));
  };
  return (
    <section className="ncp-panel vn-add">
      <div className="nc-label">Add a meeting</div>
      <label className="nc-faint">
        Name
        <input className="set-input" value={title} autoFocus placeholder="Intro call with Sam" onChange={(e) => setTitle(e.target.value)} />
      </label>
      <div className="vn-add-row">
        <label className="nc-faint">
          Starts
          <input className="set-input" type="datetime-local" value={when} onChange={(e) => setWhen(e.target.value)} />
        </label>
        <label className="nc-faint">
          Minutes
          <input className="set-input" type="number" min={5} step={5} value={minutes} onChange={(e) => setMinutes(Number(e.target.value) || 30)} />
        </label>
      </div>
      <label className="nc-faint">
        Who will be there, separated by commas
        <input className="set-input" value={people} placeholder="Sam Rivera, Lee Park" onChange={(e) => setPeople(e.target.value)} />
      </label>
      <label className="nc-faint">
        What it is about
        <textarea className="set-input vn-textarea" value={about} placeholder="Anything you know: the invite, what they asked for, their company." onChange={(e) => setAbout(e.target.value)} />
      </label>
      <label className="nc-faint">
        Call link
        <input className="set-input" value={link} placeholder="https://meet.google.com/…" onChange={(e) => setLink(e.target.value)} />
      </label>
      {error && <p className="note-error">{error}</p>}
      <div className="nc-q-buttons">
        <button type="button" className="nc-btn is-dark" onClick={save}>Add</button>
        <button type="button" className="nc-btn" onClick={onDone}>Cancel</button>
      </div>
    </section>
  );
}

/// One meeting to come: who and when, the brief, what to be ready for.
function MeetingPrepPage({ meeting: u, notes, onBack, onOpenNote, onChanged }: { meeting: UpcomingMeeting; notes: MeetingNote[]; onBack: () => void; onOpenNote: (id: string) => void; onChanged: () => void }) {
  const e = u.event;
  const prep: MeetingPrep | null = u.prep;
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [addingPerson, setAddingPerson] = useState(false);
  const [wholeInvite, setWholeInvite] = useState(false);
  const people = prep?.participants.length ? prep.participants : e.attendees;
  const first = people[0]?.split(/\s+/)[0] ?? "";
  const lastNote = notes.find((n) => n.participants.some((p) => people.some((q) => q.split(/\s+/)[0].toLowerCase() === p.split(/\s+/)[0].toLowerCase())));

  const run = (what: string, p: Promise<unknown>) => {
    setBusy(what);
    setError(null);
    void p
      .then(onChanged)
      .catch((err) => setError(said(err)))
      .finally(() => setBusy(null));
  };

  return (
    <div className="ncp">
      <nav className="ncp-crumbs" aria-label="Breadcrumb">
        <button type="button" onClick={onBack}>← Upcoming</button>
        <span aria-hidden="true">/</span>
        <b>Before the call</b>
      </nav>

      <header className="ncp-head">
        <div className="ncp-head-main">
          <div className="nc-eyebrow">
            Next conversation <span>· {whenLabel(e.starts_at)}</span>
          </div>
          <h1 className="ncp-title">{e.title}</h1>
          <p className="ncp-agenda">
            {prep?.agenda || (busy === "build" ? "Preparing…" : prep ? "" : "Not prepared yet.")}
          </p>
          <div className="nc-row">
            {people.map((p) => (
              <span className="nc-chip" key={p}>
                <span className="nc-avatar" aria-hidden="true">{p[0]?.toUpperCase()}</span>
                {p}
                <button type="button" aria-label={`Remove ${p}`} disabled={busy !== null} onClick={() => run("people", api.setPrepPeople(e.id, people.filter((x) => x !== p)))}>×</button>
              </span>
            ))}
            {addingPerson ? (
              <input
                className="nc-add-person"
                autoFocus
                placeholder="Name"
                aria-label="Add a participant"
                onBlur={() => setAddingPerson(false)}
                onKeyDown={(ev) => {
                  const el = ev.currentTarget;
                  if (ev.key === "Escape") setAddingPerson(false);
                  if (ev.key === "Enter" && el.value.trim()) {
                    setAddingPerson(false);
                    run("people", api.setPrepPeople(e.id, [...people, el.value.trim()]));
                  }
                }}
              />
            ) : (
              <button type="button" className="nc-add" onClick={() => setAddingPerson(true)}>+ Add someone</button>
            )}
          </div>
          {e.description.trim() && (
            <div className="ncp-source">
              <div className="nc-faint">From the invite</div>
              <p className={wholeInvite ? "" : "is-clamped"}>{e.description.trim()}</p>
              {e.description.length > 180 && (
                <button type="button" className="nc-linkish" onClick={() => setWholeInvite((w) => !w)}>{wholeInvite ? "Show less" : "Show the whole invite"}</button>
              )}
            </div>
          )}
        </div>
        <div className="ncp-head-side">
          {e.link && (
            <button type="button" className="nc-primary" onClick={() => void openUrl(e.link!).catch(() => {})}>Join the call</button>
          )}
          <button type="button" className="nc-btn" disabled={busy !== null} onClick={() => run("build", api.buildMeetingPrep(e.id, !!prep))}>
            {busy === "build" ? "Preparing…" : prep ? "Prepare again" : "Prepare now"}
          </button>
          {lastNote && <button type="button" className="nc-btn" onClick={() => onOpenNote(lastNote.id)}>Read previous notes</button>}
          {e.source === "manual" && (
            <button type="button" className="nc-btn" onClick={() => void api.deleteUpcomingMeeting(e.id).then(() => { onChanged(); onBack(); })}>Remove this meeting</button>
          )}
        </div>
      </header>

      {error && <p className="note-error">{error}</p>}

      {prep?.brief && (
        <section className="ncp-panel ncp-brief">
          <div className="nc-label">
            Before the call
            {prep.sources.length > 0 && <span>Looked up on the web</span>}
          </div>
          <p className="ncp-brief-text">{prep.brief}</p>
          {prep.sources.length > 0 && (
            <div className="nc-row">
              {prep.sources.map(([title, url]) => (
                <button key={url} type="button" className="nc-linkish" title={url} onClick={() => void openUrl(url).catch(() => {})}>{title}</button>
              ))}
            </div>
          )}
        </section>
      )}

      {prep?.thin && <p className="nc-faint">There is little to prepare from yet. Add who will be there and what it is about, then prepare again.</p>}

      {prep && !prep.thin && (
        <div className="ncp-cols">
          <div className="ncp-left">
            {prep.questions.length > 0 && (
              <section className="ncp-panel">
                <div className="nc-label">
                  Questions for you
                  <span>{prep.questions.length} {prep.questions.length === 1 ? "question" : "questions"} · your answers shape the notes</span>
                </div>
                {prep.questions.map((q, i) => {
                  const answered = q.answer !== null && editing !== i;
                  const skipped = q.answer === "";
                  return (
                    <div className={`nc-q${skipped ? " is-skipped" : ""}`} key={i}>
                      <div className="nc-q-text">{q.question}</div>
                      {answered ? (
                        <div className="nc-q-answer">
                          <span className="nc-you">{skipped ? "Skipped" : "You"}</span>
                          {!skipped && <span>{q.answer}</span>}
                          <button type="button" className="nc-linkish" onClick={() => setEditing(i)}>{skipped ? "answer" : "edit"}</button>
                        </div>
                      ) : (
                        <form
                          className="nc-q-form"
                          onSubmit={(ev) => {
                            ev.preventDefault();
                            const el = ev.currentTarget.elements.namedItem("answer") as HTMLInputElement;
                            if (!el.value.trim()) return;
                            setEditing(null);
                            run("q", api.answerPrepQuestion(e.id, i, el.value.trim()));
                          }}
                        >
                          <label className="nc-faint">
                            Your answer
                            <input name="answer" defaultValue={q.answer ?? ""} disabled={busy !== null} />
                          </label>
                          <div className="nc-q-buttons">
                            <button type="submit" className="nc-btn is-dark" disabled={busy !== null}>Save</button>
                            <button type="button" className="nc-btn" disabled={busy !== null} onClick={() => { setEditing(null); run("q", api.answerPrepQuestion(e.id, i, null)); }}>Skip</button>
                          </div>
                        </form>
                      )}
                    </div>
                  );
                })}
              </section>
            )}
            {prep.last_time && (
              <section className="ncp-panel">
                <div className="nc-label">Last time</div>
                <p className="nc-last">{prep.last_time}</p>
              </section>
            )}
          </div>

          <section className="ncp-panel ncp-preds">
            <div className="nc-label is-accent">
              What will probably come up
              <span>{prep.predictions.length} {prep.predictions.length === 1 ? "point" : "points"} for your call{first ? ` with ${first}` : ""}</span>
            </div>
            {prep.predictions.length > 0 && (
              <div className="nc-grid">
                {prep.predictions.map((x, i) => (
                  <article className="nc-pred" key={i}>
                    <div className="nc-pred-head">
                      <h3>{x.point}</h3>
                      <button type="button" className="nc-pred-drop" aria-label="Wrong, drop this point" title="Wrong about this one" disabled={busy !== null} onClick={() => run("drop", api.dropPrepPoint(e.id, i))}>×</button>
                    </div>
                    {x.why && <p className="nc-pred-why">{x.why}</p>}
                    {x.say && (
                      <div className="nc-say">
                        <span>Say</span>
                        {x.say}
                      </div>
                    )}
                    <div className="nc-pred-src" title={x.source}>{x.source === "you" ? "You added this" : x.source}</div>
                  </article>
                ))}
              </div>
            )}
            <div className="nc-foot">
              <span className="nc-faint">
                {busy === "build" ? "Preparing again…" : busy === "q" ? "Working your answer in…" : "Wrong about one? Drop it and it is left out."}
              </span>
              {adding ? (
                <form
                  className="nc-add-point"
                  onSubmit={(ev) => {
                    ev.preventDefault();
                    const el = ev.currentTarget.elements.namedItem("point") as HTMLInputElement;
                    if (!el.value.trim()) return;
                    setAdding(false);
                    run("add", api.addPrepPoint(e.id, el.value.trim()));
                  }}
                >
                  <input name="point" autoFocus placeholder="What else will come up?" aria-label="A point to add" />
                  <button type="submit" className="nc-btn is-dark">Add</button>
                  <button type="button" className="nc-btn" onClick={() => setAdding(false)}>Cancel</button>
                </form>
              ) : (
                <button type="button" className="nc-btn" onClick={() => setAdding(true)} disabled={busy !== null}>Add a point</button>
              )}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
