//! Chat pop-out: a separate always-on-top, borderless, transparent window. The UI
//! paints its own background with adjustable alpha (text stays fully opaque, so it
//! is readable over a game), and click-through lets the mouse pass to the window
//! underneath. Click-through is toggled with a global hotkey because the window
//! cannot be clicked while it is on.
//!
//! Whole-window alpha (WS_EX_LAYERED) is not used: WebView2 renders blank inside
//! layered top-level windows.

use std::sync::Mutex;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "chat-popout";
pub const HOTKEY: &str = "ctrl+alt+c";

struct State {
    opacity: f64,
    click_through: bool,
}

static STATE: Mutex<State> = Mutex::new(State { opacity: 0.85, click_through: false });

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

pub fn open(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    // Position/size are restored automatically by the window-state plugin when the window is created.
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Streamtubah Chat")
        .inner_size(380.0, 620.0)
        .min_inner_size(220.0, 180.0)
        .always_on_top(true)
        .decorations(false)
        .transparent(true)
        .build()
        .map_err(|e| e.to_string())?;
    STATE.lock().unwrap().click_through = false;
    Ok(())
}

fn notify(app: &AppHandle) {
    let _ = app.emit("popout:state", state());
}

pub fn set_opacity(app: &AppHandle, opacity: f64) {
    STATE.lock().unwrap().opacity = opacity.clamp(0.0, 1.0);
    notify(app);
}

pub fn set_click_through(app: &AppHandle, enabled: bool) {
    STATE.lock().unwrap().click_through = enabled;
    if let Some(w) = window(app) {
        let _ = w.set_ignore_cursor_events(enabled);
    }
    notify(app);
}

pub fn toggle_click_through(app: &AppHandle) {
    if window(app).is_none() {
        return;
    }
    let next = !STATE.lock().unwrap().click_through;
    set_click_through(app, next);
}

pub fn state() -> serde_json::Value {
    let s = STATE.lock().unwrap();
    json!({ "opacity": s.opacity, "click_through": s.click_through, "hotkey": HOTKEY })
}
