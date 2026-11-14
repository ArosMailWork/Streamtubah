//! User-assigned global hotkeys (Settings → Hotkeys). Registered system-wide so they work
//! while a game has focus; a registered combination is consumed and not passed to other apps.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Mutex;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::commands::AppState;
use crate::studio::{DESKTOP_AUDIO, MIC_AUDIO};

/// Fixed actions; scene switches use `scene:<scene name>`.
pub const ACTIONS: &[&str] = &[
    "stream.toggle",
    "record.toggle",
    "record.pause",
    "replay.toggle",
    "replay.save",
    "virtualcam.toggle",
    "studio.transition",
    "mute.desktop",
    "mute.mic",
];

/// Registered shortcut id -> action.
static REGISTERED: Mutex<Option<HashMap<u32, (Shortcut, String)>>> = Mutex::new(None);

/// Replaces all user hotkeys. Returns one message per combination that could not be registered.
pub fn apply(app: &AppHandle, map: &BTreeMap<String, String>) -> Vec<String> {
    let gs = app.global_shortcut();
    let mut reg = REGISTERED.lock().unwrap();
    for (shortcut, _) in reg.take().unwrap_or_default().values() {
        let _ = gs.unregister(*shortcut);
    }
    let reserved = Shortcut::from_str(crate::popout::HOTKEY).ok().map(|s| s.id());
    let mut next = HashMap::new();
    let mut errors = Vec::new();
    for (action, accel) in map.iter().filter(|(_, a)| !a.trim().is_empty()) {
        let Ok(shortcut) = Shortcut::from_str(accel) else {
            errors.push(format!("{accel}: not a valid key combination"));
            continue;
        };
        if Some(shortcut.id()) == reserved || next.contains_key(&shortcut.id()) {
            errors.push(format!("{accel}: already used by another action"));
            continue;
        }
        match gs.register(shortcut) {
            Ok(()) => {
                next.insert(shortcut.id(), (shortcut, action.clone()));
            }
            Err(_) => errors.push(format!("{accel}: taken by another application")),
        }
    }
    *reg = Some(next);
    errors
}

/// Action bound to a pressed shortcut, if any.
pub fn action_for(shortcut: &Shortcut) -> Option<String> {
    REGISTERED.lock().unwrap().as_ref()?.get(&shortcut.id()).map(|(_, a)| a.clone())
}

pub fn dispatch(app: &AppHandle, action: &str) {
    if action == "virtualcam.toggle" {
        crate::virtualcam::toggle(app.clone());
        return;
    }
    let state = app.state::<AppState>();
    let Ok(mut s) = state.0.lock() else { return };
    let toggle = |s: &mut crate::studio::Studio, kind: &str| {
        if s.outputs.is_running(kind) {
            s.outputs.stop(kind);
            Ok(())
        } else {
            s.start_output(kind)
        }
    };
    let result = match action {
        "stream.toggle" => toggle(&mut s, "stream"),
        "record.toggle" => toggle(&mut s, "record"),
        "replay.toggle" => toggle(&mut s, "replay"),
        "record.pause" => {
            let paused = s.outputs.record_paused();
            s.outputs.set_record_paused(!paused)
        }
        "replay.save" => s.outputs.save_replay(),
        "studio.transition" => s.studio_transition(),
        "mute.desktop" => s.audio_toggle_muted(DESKTOP_AUDIO),
        "mute.mic" => s.audio_toggle_muted(MIC_AUDIO),
        a => match a.strip_prefix("scene:") {
            Some(scene) => s.scene_set_current(scene),
            None => Err(format!("Unknown hotkey action '{a}'")),
        },
    };
    drop(s);
    if let Err(e) = result {
        let _ = app.emit("app:error", json!(e));
    }
    let _ = app.emit("studio:changed", Value::Null);
}
