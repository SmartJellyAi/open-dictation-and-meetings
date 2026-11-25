import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./ErrorBoundary";

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

function mount() {
  root.render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}

// Waits briefly for the app's bridge before mounting: in dev the page can
// load a moment before it is ready, and the first calls would fail.
function whenTauriReady(done: () => void) {
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown };
  if (w.__TAURI_INTERNALS__) {
    done();
    return;
  }
  const started = Date.now();
  const timer = setInterval(() => {
    if (w.__TAURI_INTERNALS__ || Date.now() - started >= 3000) {
      clearInterval(timer);
      done();
    }
  }, 20);
}

whenTauriReady(mount);
