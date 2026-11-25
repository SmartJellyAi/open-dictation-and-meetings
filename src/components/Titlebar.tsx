import { useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "./Icon";
import { api } from "../lib/api";
import type { MeetingState } from "../types";

export type ThemeChoice = "system" | "light" | "dark";

const CHOICES: { value: ThemeChoice; label: string; icon: "display" | "sun" | "moon" }[] = [
  { value: "system", label: "Match system", icon: "display" },
  { value: "light", label: "Light", icon: "sun" },
  { value: "dark", label: "Dark", icon: "moon" },
];

/// The page's title, the one action every page shares (dictate), and the
/// appearance switch.
export function Titlebar({ title, theme, onSetTheme }: { title: ReactNode; theme: ThemeChoice; onSetTheme: (t: ThemeChoice) => void }) {
  const [key, setKey] = useState("");
  const [meeting, setMeeting] = useState<MeetingState | null>(null);
  useEffect(() => {
    const read = () => void api.getSettings().then((s) => setKey(s.dictation_on ? prettyKey(s.dictation_key) : "")).catch(() => {});
    read();
    void api.getMeetingState().then(setMeeting).catch(() => {});
    const subs = [listen("settings-changed", read), listen<MeetingState>("meeting-state", (e) => setMeeting(e.payload))];
    return () => subs.forEach((p) => void p.then((f) => f()));
  }, []);
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-title" data-tauri-drag-region>{title}</div>
      <div className="titlebar-right">
        {meeting?.listening && (
          <button type="button" className="cg-talk vn-live" onClick={() => void api.stopMeetingNotes()} title="Stop taking notes">
            <span className="mt-live-dot" />
            Stop notes
          </button>
        )}
        <button type="button" className="cg-talk" onClick={() => void api.toggleDictation()} title={key ? `Dictate, or press ${key} in any app` : "Dictate"}>
          <Icon name="mic" size={15} />
          Dictate {key ? <span>{key}</span> : null}
        </button>
        <div className="theme-seg" role="radiogroup" aria-label="Appearance">
          {CHOICES.map((c) => (
            <button
              key={c.value}
              type="button"
              role="radio"
              aria-checked={theme === c.value}
              className={`theme-seg-option${theme === c.value ? " is-selected" : ""}`}
              title={c.label}
              onClick={() => onSetTheme(c.value)}
            >
              <Icon name={c.icon} size={15} />
            </button>
          ))}
        </div>
      </div>
    </header>
  );
}

const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);

/// "Alt+Space" as the keyboard prints it: "⌥ Space" on a Mac.
export function prettyKey(key: string): string {
  const mac: Record<string, string> = { alt: "⌥", option: "⌥", ctrl: "⌃", control: "⌃", shift: "⇧", super: "⌘", cmd: "⌘", command: "⌘", commandorcontrol: "⌘" };
  const other: Record<string, string> = { alt: "Alt", option: "Alt", ctrl: "Ctrl", control: "Ctrl", shift: "Shift", super: "Win", cmd: "Ctrl", command: "Ctrl", commandorcontrol: "Ctrl" };
  const parts = key.split("+").map((p) => p.trim()).filter(Boolean);
  const names = IS_MAC ? mac : other;
  const out = parts.map((p) => names[p.toLowerCase()] ?? (p.length === 1 ? p.toUpperCase() : p));
  return IS_MAC ? out.join("") .replace(/([⌥⌃⇧⌘])(?=[A-Z0-9][a-z]*$)/, "$1 ") : out.join("+");
}
