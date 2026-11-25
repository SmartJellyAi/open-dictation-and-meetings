//! The pill: a small window at the top of the screen that shows what is
//! happening (dictating, a call noticed, notes running) and never takes
//! the keyboard from the app you are typing in.
//!
//! The page inside decides when it has something to show and asks to be
//! shown or hidden; this places it at the top centre of the screen.

use tauri::{AppHandle, Emitter, Manager};

pub const LABEL: &str = "pill";

/// Tells the pill something changed, so it reads the state again.
pub fn refresh(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "pill-refresh", ());
}

fn place(window: &tauri::WebviewWindow) {
    let monitor = window.current_monitor().ok().flatten().or_else(|| window.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let size = m.size().to_logical::<f64>(scale);
    let origin = m.position().to_logical::<f64>(scale);
    let width = window.outer_size().map(|s| s.to_logical::<f64>(scale).width).unwrap_or(460.0);
    // Below the menu bar on macOS; near the top edge elsewhere.
    let top = if cfg!(target_os = "macos") { 34.0 } else { 12.0 };
    let _ = window.set_position(tauri::LogicalPosition::new(origin.x + (size.width - width) / 2.0, origin.y + top));
}

#[tauri::command]
pub fn pill_visible(app: AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    if visible {
        place(&window);
        let _ = window.set_focusable(false);
        let _ = window.set_always_on_top(true);
        let _ = window.show();
    } else {
        let _ = window.hide();
    }
}
