//! Projector: a separate window showing the program output, e.g. fullscreen on a second
//! monitor. A native display covers the whole client area; the (empty) web page underneath
//! keeps keyboard focus for F11 / Esc (see `src/ProjectorApp.tsx`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::commands::AppState;
use crate::preview::Preview;

pub const LABEL: &str = "projector";

pub fn monitors(app: &AppHandle) -> Vec<String> {
    let Some(main) = app.get_webview_window("main") else { return Vec::new() };
    main.available_monitors()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let s = m.size();
            format!("{}: {} ({}x{})", i + 1, m.name().cloned().unwrap_or_else(|| "Display".into()), s.width, s.height)
        })
        .collect()
}

/// Opens (or focuses) the projector. `monitor`: fullscreen on that monitor, else a normal window.
pub fn open(app: &AppHandle, monitor: Option<usize>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Streamtubah Projector")
        .inner_size(960.0, 540.0)
        .build()
        .map_err(|e| e.to_string())?;
    if let Some(m) = monitor.and_then(|i| window.available_monitors().ok()?.into_iter().nth(i)) {
        let _ = window.set_position(*m.position());
        let _ = window.set_fullscreen(true);
    }

    let state = app.state::<AppState>();
    let display = {
        let s = state.0.lock().map_err(|_| "Studio state poisoned")?;
        Preview::create(window.hwnd().map_err(|e| e.to_string())?.0 as isize, app.clone(), s.dirty.clone())?
    };
    display.set_mode(true, false);
    let size = window.inner_size().map_err(|e| e.to_string())?;
    display.set_bounds(0, 0, size.width, size.height);
    state.0.lock().map_err(|_| "Studio state poisoned")?.previews.insert(LABEL.into(), display);

    let handle = app.clone();
    window.on_window_event(move |event| {
        let state = handle.state::<AppState>();
        let Ok(mut s) = state.0.lock() else { return };
        match event {
            WindowEvent::Resized(size) => {
                if let Some(p) = s.previews.get(LABEL) {
                    p.set_bounds(0, 0, size.width, size.height);
                }
            }
            // Drop the display while its parent window still exists.
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
                s.previews.remove(LABEL);
            }
            _ => {}
        }
    });
    Ok(())
}
