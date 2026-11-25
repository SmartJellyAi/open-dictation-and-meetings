import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, said } from "../lib/api";
import type { DictationState, MeetingOffer, MeetingState } from "../types";

/// How much of the words heard so far fits on the pill's one line.
const TAIL = 64;

/// The pill at the top of the screen: dictation as it happens, a call
/// noticed, notes running. It never takes the keyboard, and it is hidden
/// whenever there is nothing to say.
export function Pill() {
  const [dictation, setDictation] = useState<DictationState | null>(null);
  const [offer, setOffer] = useState<MeetingOffer | null>(null);
  const [meeting, setMeeting] = useState<MeetingState | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const shown = useRef(false);

  useEffect(() => {
    void api.getMeetingState().then(setMeeting).catch(() => {});
    void api.getMeetingOffer().then(setOffer).catch(() => {});
    const subs = [
      listen<DictationState>("dictation-state", (e) => {
        setCopied(false);
        setDictation(e.payload.active || e.payload.error ? e.payload : null);
      }),
      listen<MeetingOffer | null>("meeting-offer", (e) => setOffer(e.payload)),
      listen<MeetingState>("meeting-state", (e) => setMeeting(e.payload)),
      listen<string>("meeting-transcribe-error", (e) => setProblem(e.payload)),
      listen("pill-refresh", () => {
        void api.getMeetingState().then(setMeeting).catch(() => {});
        void api.getMeetingOffer().then(setOffer).catch(() => {});
      }),
    ];
    return () => subs.forEach((p) => void p.then((f) => f()));
  }, []);

  // A dictation that could not be typed stays a while to be copied.
  useEffect(() => {
    if (!dictation?.error) return;
    const t = window.setTimeout(() => setDictation(null), dictation.text ? 20_000 : 6_000);
    return () => window.clearTimeout(t);
  }, [dictation]);
  useEffect(() => {
    if (!problem) return;
    const t = window.setTimeout(() => setProblem(null), 8_000);
    return () => window.clearTimeout(t);
  }, [problem]);

  const visible = !!dictation || !!offer || !!meeting?.listening || !!problem;
  useEffect(() => {
    if (visible === shown.current) return;
    shown.current = visible;
    void api.pillVisible(visible).catch(() => {});
  }, [visible]);

  if (dictation) {
    if (dictation.error) {
      return (
        <div className="pill is-wide" role="status">
          <p className="pill-words">{dictation.error}</p>
          {dictation.text && (
            <button type="button" className="pill-btn is-primary" onClick={() => void writeText(dictation.text!).then(() => setCopied(true))}>
              {copied ? "Copied" : "Copy"}
            </button>
          )}
          <button type="button" className="pill-btn" onClick={() => setDictation(null)}>Close</button>
        </div>
      );
    }
    const words = dictation.partial.trim();
    const tail = words.length > TAIL ? `…${words.slice(-TAIL).replace(/^\S*\s/, "")}` : words;
    return (
      <div className="pill" role="status" aria-live="polite">
        <span className={`pill-mic${dictation.finishing ? " is-done" : ""}`} aria-hidden="true">
          <span className="pill-mic-pulse" />
        </span>
        <p className={`pill-words${tail ? "" : " is-waiting"}`}>
          {dictation.finishing ? (tail ? tail : "Typing…") : tail || (dictation.held ? "Listening. Let go to type." : "Listening…")}
        </p>
        {!dictation.finishing && !dictation.held && (
          <button type="button" className="pill-stop" aria-label="Stop dictating" title="Stop" onClick={() => void api.toggleDictation()}>
            <span />
          </button>
        )}
      </div>
    );
  }

  if (problem) {
    return (
      <div className="pill is-wide" role="status">
        <p className="pill-words">{problem}</p>
        <button type="button" className="pill-btn" onClick={() => void api.showMain("settings")}>Settings</button>
        <button type="button" className="pill-btn" onClick={() => setProblem(null)}>Close</button>
      </div>
    );
  }

  if (offer && !meeting?.listening) {
    const where = offer.service === "a meeting" ? "A call" : `A call in ${offer.service}`;
    return (
      <div className="pill is-wide" role="status">
        <p className="pill-words">{where}. Take notes?</p>
        <button
          type="button"
          className="pill-btn is-primary"
          onClick={() => void api.acceptMeetingOffer().catch((e) => setProblem(said(e)))}
        >
          Take notes
        </button>
        <button type="button" className="pill-btn" onClick={() => void api.declineMeetingOffer()}>Not now</button>
      </div>
    );
  }

  if (meeting?.listening) {
    return (
      <div className="pill" role="status">
        <span className="pill-rec" aria-hidden="true">
          <span className="pill-mic-pulse" />
        </span>
        <button type="button" className="pill-words pill-link" onClick={() => void api.showMain("meetings")}>
          Taking notes{meeting.service && meeting.service !== "Meeting" ? ` · ${meeting.service}` : ""}
        </button>
        <button type="button" className="pill-btn" onClick={() => void api.stopMeetingNotes()}>Stop</button>
      </div>
    );
  }

  return null;
}
