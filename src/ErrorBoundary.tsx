import React from "react";

/// A last-resort net so a render/effect crash shows a legible error instead of
/// a blank window. Before this, one unguarded `invoke`/`listen` at mount (e.g.
/// the Tauri IPC internals not yet injected when the dev-server webview loads)
/// took the whole app down to a black/white screen with no way back. Now the
/// error is shown with a Reload button -- and by the time the user reloads, the
/// runtime is ready, so it recovers.
interface State {
  error: Error | null;
}

export class ErrorBoundary extends React.Component<{ children: React.ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // Surfaced to the terminal too (Tauri forwards webview console to stdout in
    // dev), so it's captured in logs, not only on screen.
    console.error("[app] uncaught error:", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            padding: 24,
            fontFamily: "-apple-system, system-ui, sans-serif",
            color: "#e6e6e6",
            background: "#1a1a1e",
            height: "100vh",
            boxSizing: "border-box",
            overflow: "auto",
          }}
        >
          <h2 style={{ margin: "0 0 8px", fontSize: 16 }}>Something went wrong loading this window</h2>
          <p style={{ margin: "0 0 16px", fontSize: 13, color: "#a0a0a8" }}>
            Reload to try again.
          </p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              fontSize: 13,
              padding: "6px 14px",
              borderRadius: 8,
              border: "none",
              background: "#3b82f6",
              color: "#fff",
              cursor: "pointer",
            }}
          >
            Reload
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
