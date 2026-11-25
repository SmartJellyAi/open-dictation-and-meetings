import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "./Icon";
import { api } from "../lib/api";
import type { View } from "../types";

type IconName = Parameters<typeof Icon>[0]["name"];

const NAV: { key: View; label: string; icon: IconName }[] = [
  { key: "meetings", label: "Meetings", icon: "meetings" },
  { key: "upcoming", label: "Upcoming", icon: "clock" },
];

/// The sidebar as the app draws it: the brand, the places, Settings last.
export function Sidebar({ view, onNavigate }: { view: View; onNavigate: (v: View) => void }) {
  // Upcoming meetings whose notes are ready and not yet opened.
  const [ready, setReady] = useState(0);
  useEffect(() => {
    const read = () =>
      void api
        .upcomingMeetings()
        .then((list) => {
          let seen: string[] = [];
          try {
            seen = JSON.parse(localStorage.getItem("prep-seen") ?? "[]");
          } catch {
            seen = [];
          }
          setReady(list.filter((u) => u.prep && !u.prep.thin && !seen.includes(u.event.id) && u.event.starts_at - Date.now() < 16 * 3_600_000).length);
        })
        .catch(() => {});
    read();
    const subs = [listen("meeting-prep-changed", read), listen("meetings-upcoming-changed", read), listen("prep-seen", read)];
    const t = window.setInterval(read, 60_000);
    return () => {
      window.clearInterval(t);
      subs.forEach((p) => void p.then((f) => f()));
    };
  }, []);

  return (
    <aside className="sidebar cg-sidebar">
      <div className="cg-brand">
        <img src="/jelly-logo.png" alt="" decoding="async" />
        <span className="cg-brand-name">Open Dictation &amp; Meetings</span>
      </div>
      <div className="cg-gap" />
      {NAV.map((item) => (
        <button key={item.key} type="button" className={`cg-nav-item${view === item.key ? " is-active" : ""}`} onClick={() => onNavigate(item.key)}>
          <Icon name={item.icon} size={18} />
          <span className="cg-nav-label">{item.label}</span>
          {item.key === "upcoming" && ready > 0 && <span className="cg-nav-badge">{ready}</span>}
        </button>
      ))}
      <div className="cg-side-scroll" />
      <button type="button" className={`cg-nav-item${view === "settings" ? " is-active" : ""}`} onClick={() => onNavigate("settings")}>
        <Icon name="settings" size={18} />
        <span className="cg-nav-label">Settings</span>
      </button>
    </aside>
  );
}
