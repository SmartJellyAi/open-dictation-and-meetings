import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, said } from "../../lib/api";
import type { MeetingNote, MeetingState, TranscriptUpdate, UpcomingMeeting } from "../../types";
import { speakerLabel } from "../../types";
import { Icon } from "../Icon";
import { TranscriptLines } from "./TranscriptLines";
import { SummaryBlocks } from "./SummaryBlocks";

/// Meeting notes: what is being said right now, and what was said before.
/// The calls on the left (the next one first, a call being written down,
/// then past notes by day) and the one you picked as a document.

function duration(note: MeetingNote): string {
  if (!note.ended_at) return "in progress";
  const mins = Math.max(1, Math.round((note.ended_at - note.started_at) / 60000));
  return mins < 60 ? `${mins} min` : `${Math.floor(mins / 60)}h ${mins % 60}m`;
}

function dayLabel(ts: number): string {
  const d = new Date(ts);
  const today = new Date();
  const yesterday = new Date(Date.now() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  if (Date.now() - ts < 6 * 86_400_000) return d.toLocaleDateString(undefined, { weekday: "long" });
  return d.toLocaleDateString(undefined, { day: "numeric", month: "long" });
}

export default function Meetings({ openNote, onOpened, onOpenSettings }: { openNote: string | null; onOpened: () => void; onOpenSettings: () => void }) {
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [live, setLive] = useState<TranscriptUpdate | null>(null);
  const [state, setState] = useState<MeetingState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [summaryError, setSummaryError] = useState<{ id: string; message: string } | null>(null);
  const [upcoming, setUpcoming] = useState<UpcomingMeeting[]>([]);
  const [exported, setExported] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const tail = useRef<HTMLDivElement | null>(null);

  const refresh = () => {
    api.listMeetingNotes().then(setNotes).catch(() => {});
    api.upcomingMeetings().then(setUpcoming).catch(() => {});
  };

  useEffect(() => {
    if (openNote) {
      setOpen(openNote);
      onOpened();
    }
  }, [openNote, onOpened]);

  useEffect(() => {
    refresh();
    api.getMeetingState().then(setState).catch(() => {});
    api.getLiveTranscript().then(setLive).catch(() => {});
    const subs = [
      listen<string>("meeting-summary-writing", (e) => setBusy(e.payload)),
      listen<{ id: string; message: string }>("meeting-summary-error", (e) => setSummaryError(e.payload)),
      listen("meeting-notes-changed", () => {
        setBusy(null);
        refresh();
      }),
      listen("meetings-upcoming-changed", refresh),
      listen<TranscriptUpdate>("meeting-transcript", (e) => setLive(e.payload)),
      listen<MeetingState>("meeting-state", (e) => {
        setState(e.payload);
        if (e.payload.listening) {
          setError(null);
          setOpen("live");
        } else {
          setTimeout(refresh, 800);
        }
      }),
      listen<string>("meeting-transcribe-error", (e) => setError(e.payload)),
    ];
    return () => subs.forEach((p) => void p.then((un) => un()).catch(() => {}));
  }, []);

  // Follows the words as they arrive.
  useEffect(() => {
    tail.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [live?.partial_me, live?.partial_others, live?.lines?.length]);

  const summarise = (id: string) => {
    setBusy(id);
    setSummaryError(null);
    api
      .summariseMeeting(id)
      .then(() => refresh())
      .catch((e) => setSummaryError({ id, message: said(e) }))
      .finally(() => setBusy(null));
  };

  const start = () => {
    setStarting(true);
    setError(null);
    api
      .startMeetingNotes()
      .then(() => setOpen("live"))
      .catch((e) => setError(said(e)))
      .finally(() => setStarting(false));
  };

  const listening = state?.listening ? state.service ?? "a meeting" : null;
  const selectedId = open ?? (listening ? "live" : notes[0]?.id ?? null);
  const selected = selectedId && selectedId !== "live" ? notes.find((n) => n.id === selectedId) ?? null : null;
  const days: { label: string; notes: MeetingNote[] }[] = [];
  for (const n of notes) {
    const label = dayLabel(n.started_at);
    const last = days[days.length - 1];
    if (last && last.label === label) last.notes.push(n);
    else days.push({ label, notes: [n] });
  }
  const next = upcoming.filter((u) => u.event.starts_at - Date.now() < 16 * 3_600_000).slice(0, 2);

  return (
    <div className="mt-split">
      <aside className="mt-list" aria-label="Meetings">
        {next.map((u) => (
          <div className="mt-next" key={u.event.id}>
            <span className="mt-next-when">
              Next · {new Date(u.event.starts_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}
            </span>
            <span className="mt-next-title">{u.event.title}</span>
            {(u.prep?.agenda || u.event.attendees.length > 0) && <span className="mt-next-sub">{u.prep?.agenda || u.event.attendees.join(", ")}</span>}
          </div>
        ))}
        {listening ? (
          <button type="button" className={`mt-item${selectedId === "live" ? " is-active" : ""}`} onClick={() => setOpen("live")}>
            <span className="mt-item-title"><span className="mt-live-dot" />Taking notes now</span>
            <span className="mt-item-meta">{listening}</span>
          </button>
        ) : (
          <div className="mt-tools">
            <button type="button" className="cg-small-btn is-primary" disabled={starting} onClick={start}>
              {starting ? "Starting…" : "Take notes now"}
            </button>
          </div>
        )}
        {notes.length > 0 && (
          <div className="mt-tools">
            <button
              type="button"
              className="cg-link"
              disabled={exported === "Saving…"}
              onClick={() => {
                setExported("Saving…");
                void api
                  .exportAllMeetings()
                  .then((r) => setExported(`${r.count} saved to Downloads/Meeting notes${r.skipped ? `, ${r.skipped} with no notes yet` : ""}`))
                  .catch(() => setExported("The notes could not be saved."));
              }}
            >
              Export all notes
            </button>
            {exported && <span className="mt-tools-note" role="status">{exported}</span>}
          </div>
        )}
        {days.map((d) => (
          <div className="mt-day" key={d.label}>
            <div className="mt-day-label">{d.label}</div>
            {d.notes.map((n) => (
              <button key={n.id} type="button" className={`mt-item${selectedId === n.id ? " is-active" : ""}`} onClick={() => setOpen(n.id)}>
                <span className="mt-item-title">{n.display_title}</span>
                <span className="mt-item-meta">
                  {new Date(n.started_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })} · {duration(n)}
                </span>
              </button>
            ))}
          </div>
        ))}
        <button type="button" className="cg-link mt-settings" onClick={onOpenSettings}>Meeting settings</button>
      </aside>

      {selectedId !== "live" && selected ? (
        <NoteDocument
          key={selected.id}
          note={selected}
          busy={busy === selected.id}
          error={summaryError?.id === selected.id ? summaryError.message : null}
          onSummarise={() => summarise(selected.id)}
          onDeleted={() => {
            setOpen(null);
            refresh();
          }}
        />
      ) : (
        <article className="mt-doc">
          {selectedId === "live" && listening ? (
            <section className="note-live">
              <header className="note-live-head">
                <span className="note-live-badge">
                  <span className="note-live-dot" />
                  Taking notes
                </span>
                <span className="note-live-service">{listening}</span>
                <button type="button" className="cg-small-btn" onClick={() => void api.stopMeetingNotes().then(() => setTimeout(refresh, 800)).catch(() => {})}>
                  Stop
                </button>
              </header>
              {error && <p className="note-error">{error}</p>}
              {state?.warning && !error && <p className="mt-fine">{state.warning}</p>}
              <div className="note-transcript is-live">
                <TranscriptLines update={live} emptyMessage="Listening. Words appear here as people speak." />
                <div ref={tail} />
              </div>
              <p className="note-caveat">
                Your microphone is <b>You</b>; what your computer plays is <b>Others</b>. With speakers instead of headphones, your microphone hears them too.
              </p>
            </section>
          ) : (
            <div className="notes-empty">
              <Icon name="mic" size={26} />
              <h3>No meeting notes yet</h3>
              <p>When a call starts in a meeting app, the pill at the top of the screen asks whether to take notes. Or press Take notes now.</p>
              {error && <p className="note-error">{error}</p>}
            </div>
          )}
        </article>
      )}
    </div>
  );
}

/// One call as a document: your notes, the written notes around them, the
/// people on it, and the transcript one tap away.
function NoteDocument({ note, busy, error, onSummarise, onDeleted }: { note: MeetingNote; busy: boolean; error: string | null; onSummarise: () => void; onDeleted: () => void }) {
  const [mine, setMine] = useState(note.my_notes);
  const [savedAt, setSavedAt] = useState<number | null>(note.my_notes_at);
  const [editing, setEditing] = useState(note.summary_blocks.length === 0);
  const [transcriptOpen, setTranscriptOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [addingPerson, setAddingPerson] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportNote, setExportNote] = useState<string | null>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const pending = useRef<string | null>(null);

  useEffect(() => {
    if (!exportNote) return;
    const t = window.setTimeout(() => setExportNote(null), 4000);
    return () => window.clearTimeout(t);
  }, [exportNote]);
  const exportVia = (run: () => Promise<unknown>, done: string) => {
    setExportNote(null);
    setExporting(true);
    void run()
      .then(() => setExportNote(done))
      .catch((e) => setExportNote(said(e)))
      .finally(() => setExporting(false));
  };

  // Saved as you type, a moment after you stop, and on leaving the call.
  useEffect(() => {
    if (mine === note.my_notes && pending.current === null) return;
    pending.current = mine;
    const t = window.setTimeout(() => {
      const text = pending.current;
      if (text === null) return;
      void api.saveMeetingMyNotes(note.id, text).then(() => {
        pending.current = null;
        setSavedAt(Date.now());
      });
    }, 700);
    return () => window.clearTimeout(t);
  }, [mine, note.id, note.my_notes]);
  useEffect(
    () => () => {
      if (pending.current !== null) void api.saveMeetingMyNotes(note.id, pending.current);
    },
    [note.id],
  );
  useEffect(() => {
    const el = editor.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.max(el.scrollHeight, 140)}px`;
  }, [mine, editing]);
  useEffect(() => {
    if (note.summary_blocks.length > 0) setEditing(false);
  }, [note.summarised_at, note.summary_blocks.length]);

  const hasNotes = note.summary_blocks.length > 0;
  const hasMine = note.summary_blocks.some((b) => b.kind === "bullets" && b.items.some((i) => i.mine));
  const notesNewer = hasNotes && mine.trim() !== "" && (savedAt ?? 0) > (note.summarised_at ?? 0);
  const setPeople = (people: string[]) => void api.setMeetingPeople(note.id, people);

  return (
    <div className="mg-page">
      <div className="mg-scroll">
        <div className="mg-doc">
          <header className="mg-head">
            <h1>{note.display_title}</h1>
            <div className="mg-menu">
              <button type="button" className="mg-more" aria-label="More" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>⋯</button>
              {menuOpen && (
                <div className="mg-menu-list" role="menu">
                  <button type="button" role="menuitem" disabled={!note.transcript.trim()} onClick={() => { setMenuOpen(false); exportVia(() => writeText(note.transcript), "Copied the transcript"); }}>Copy the transcript</button>
                  <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); exportVia(() => api.meetingMarkdown(note.id).then(writeText), "Copied the notes as Markdown"); }}>Copy the notes as Markdown</button>
                  <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); exportVia(() => api.exportMeeting(note.id, "notes"), "Saved to Downloads/Meeting notes"); }}>Save the notes as a file</button>
                  <button type="button" role="menuitem" disabled={!note.transcript.trim()} onClick={() => { setMenuOpen(false); exportVia(() => api.exportMeeting(note.id, "transcript"), "Saved to Downloads/Meeting notes"); }}>Save the transcript as a file</button>
                  <button type="button" role="menuitem" className="is-danger" onClick={() => { setMenuOpen(false); setConfirmDelete(true); }}>Delete this meeting</button>
                </div>
              )}
            </div>
          </header>
          {confirmDelete && (
            <div className="mg-write">
              <span className="mg-write-text">
                <b>Delete this meeting?</b>
                <span>Its transcript and notes are removed from this computer. This cannot be undone.</span>
              </span>
              <button type="button" className="cg-small-btn" onClick={() => setConfirmDelete(false)}>Keep it</button>
              <button type="button" className="mg-write-btn" onClick={() => void api.deleteMeeting(note.id).then(onDeleted)}>Delete</button>
            </div>
          )}
          <div className="mg-chips">
            <span className="mg-chip">
              <Icon name="clock" size={14} />
              {new Date(note.started_at).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })}
              {" · "}
              {new Date(note.started_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}
              {" · "}
              {duration(note)}
            </span>
            <span className="mg-chip is-quiet">{note.service === "Meeting" ? "Started by hand" : note.service}{note.meeting_code ? ` · ${note.meeting_code}` : ""}</span>
          </div>
          <div className="nc-row vn-people">
            {note.participants.map((p) => (
              <span className="nc-chip" key={p}>
                <span className="nc-avatar" aria-hidden="true">{p[0]?.toUpperCase()}</span>
                {p}
                <button type="button" aria-label={`Remove ${p}`} onClick={() => setPeople(note.participants.filter((x) => x !== p))}>×</button>
              </span>
            ))}
            {addingPerson ? (
              <input
                className="nc-add-person"
                autoFocus
                placeholder="Name"
                aria-label="Add someone who was on the call"
                onBlur={() => setAddingPerson(false)}
                onKeyDown={(e) => {
                  const el = e.currentTarget;
                  if (e.key === "Escape") setAddingPerson(false);
                  if (e.key === "Enter" && el.value.trim()) {
                    setAddingPerson(false);
                    setPeople([...note.participants, el.value.trim()]);
                  }
                }}
              />
            ) : (
              <button type="button" className="nc-add" onClick={() => setAddingPerson(true)}>+ Add someone</button>
            )}
          </div>
          {exportNote && <p className={exportNote.startsWith("Saved") || exportNote.startsWith("Copied") ? "mg-export-note" : "note-error"} role="status">{exportNote}</p>}

          {editing ? (
            <>
              <textarea
                ref={editor}
                className="mg-editor"
                aria-label="Your notes"
                placeholder="Write your notes here, during the call or after. They are kept with the transcript."
                value={mine}
                onChange={(e) => setMine(e.target.value)}
              />
              <div className="mg-write">
                <span className="mg-write-text">
                  <b>{hasNotes ? (notesNewer ? "Your notes changed" : "The notes are written") : "No notes for this call yet"}</b>
                  <span>
                    {hasNotes
                      ? notesNewer
                        ? "Write them again to weave in what you just added."
                        : "Your lines stay word for word; the rest is added around them."
                      : `They can be written from the ${note.transcript_lines.length} lines heard${mine.trim() ? ", with your notes woven in" : ""}.`}
                  </span>
                </span>
                {hasNotes && <button type="button" className="cg-small-btn" onClick={() => setEditing(false)}>Show the notes</button>}
                <button type="button" className="mg-write-btn" disabled={busy || exporting} onClick={onSummarise}>
                  {busy || exporting ? "Writing…" : hasNotes ? "Write them again" : "Write the notes"}
                </button>
              </div>
            </>
          ) : (
            <>
              <div className={`mg-notes${hasMine ? " has-mine" : ""}`}>
                <SummaryBlocks blocks={note.summary_blocks} />
              </div>
              <div className="mg-notes-foot">
                {hasMine && <span>Full ink is what you wrote. Grey is what was added from the transcript.</span>}
                <button type="button" className="cg-link" onClick={() => setEditing(true)}>{mine.trim() ? "Edit your notes" : "Add your own notes"}</button>
                {notesNewer && (
                  <button type="button" className="cg-link" disabled={busy} onClick={onSummarise}>{busy ? "Writing…" : "Your notes changed: write again"}</button>
                )}
              </div>
            </>
          )}
          {error && <p className="note-error">{error}</p>}
          {note.dropped > 0 && <p className="mt-fine">A few seconds of audio dropped during this call.</p>}
        </div>
      </div>

      {transcriptOpen && (
        <div className="mg-pop mg-transcript" role="dialog" aria-label="Transcript">
          <div className="mg-pop-head">
            <span>Transcript</span>
            <button type="button" className="cg-link" onClick={() => void writeText(note.transcript)}>Copy</button>
            <button type="button" className="cg-link" onClick={() => setTranscriptOpen(false)}>Close</button>
          </div>
          {note.transcript_lines.length > 0 ? (
            <div className="mg-lines">
              {note.transcript_lines.map((l, i) => (
                <p key={i} className={`mg-line is-${l.speaker}`}>
                  <b>{speakerLabel(l, note.others_name)}</b> {l.text}
                </p>
              ))}
            </div>
          ) : (
            <p className="mt-fine">Nothing was transcribed.</p>
          )}
        </div>
      )}

      <div className="mg-bar">
        <button type="button" className={`mg-pill${transcriptOpen ? " is-on" : ""}`} aria-expanded={transcriptOpen} onClick={() => setTranscriptOpen((v) => !v)}>
          <Icon name="waveform" size={18} />
          Transcript
          <span className="mg-pill-count">{note.transcript_lines.length} lines</span>
        </button>
      </div>
    </div>
  );
}
