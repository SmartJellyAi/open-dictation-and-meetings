import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./styles/theme.css";
import "./styles/app.css";
import "./styles/signature.css";
import "./styles/chatgpt.css";
import "./styles/flat.css";
import "./styles/voice-notes.css";
import { Sidebar } from "./components/Sidebar";
import { Titlebar, type ThemeChoice } from "./components/Titlebar";
import Meetings from "./components/views/Meetings";
import Upcoming from "./components/views/Upcoming";
import { Settings } from "./components/views/Settings";
import type { View } from "./types";

const TITLES: Record<View, string> = {
  meetings: "Meetings",
  upcoming: "Upcoming",
  settings: "Settings",
};

function savedTheme(): ThemeChoice {
  try {
    const t = localStorage.getItem("theme");
    if (t === "light" || t === "dark" || t === "system") return t;
  } catch {
    // Private storage: the system's choice.
  }
  return "system";
}

export default function App() {
  const [view, setView] = useState<View>("meetings");
  const [theme, setTheme] = useState<ThemeChoice>(savedTheme);
  /// A meeting to open when Meetings shows, from another page.
  const [openNote, setOpenNote] = useState<string | null>(null);

  useEffect(() => {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
    try {
      localStorage.setItem("theme", theme);
    } catch {
      // Not remembered; the choice still holds for this run.
    }
  }, [theme]);

  // The tray and the pill can ask for a page.
  useEffect(() => {
    const un = listen<string>("navigate", (e) => {
      const v = e.payload as View;
      if (v === "meetings" || v === "upcoming" || v === "settings") setView(v);
    });
    return () => void un.then((f) => f());
  }, []);

  return (
    <div className="app-shell signature-shell">
      <Sidebar view={view} onNavigate={setView} />
      <main className="content">
        <Titlebar title={TITLES[view]} theme={theme} onSetTheme={setTheme} />
        {view === "meetings" && (
          <section className="mt-page">
            <Meetings openNote={openNote} onOpened={() => setOpenNote(null)} onOpenSettings={() => setView("settings")} />
          </section>
        )}
        {view === "upcoming" && (
          <section className="mt-page vn-scroll">
            <Upcoming
              onOpenNote={(id) => {
                setOpenNote(id);
                setView("meetings");
              }}
              onOpenSettings={() => setView("settings")}
            />
          </section>
        )}
        {view === "settings" && <Settings />}
      </main>
    </div>
  );
}
