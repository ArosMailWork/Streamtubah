//! JavaScript plugins written by the user.
//!
//! A plugin is a folder `<app data>/plugins-js/<id>/` with `manifest.json` and a `main` page
//! (`.html`, or `.js` wrapped in a generated page). Each enabled plugin runs in its own hidden
//! WebView window (label `plugin-<id>`), loaded from the API port (`/plugins/<id>/…`), so a
//! busy or crashed plugin does not freeze the app UI. Plugin pages get no Tauri IPC (no
//! capability lists them); they talk to Streamtubah only through the local API with a
//! per-run token that allows just the permissions the user approved:
//!
//! | permission  | allows |
//! |-------------|--------|
//! | scenes      | scene list / switch / create / remove, scene items |
//! | inputs      | inputs (sources), volume, mute, settings, filters |
//! | outputs     | stream, record, replay buffer, virtual camera |
//! | collections | scene collections and profiles |
//! | audio       | Audio Bridge (`/audio/<ch>`) and Plugin Audio sources (`/audio-source/<ch>`) |
//! | mic         | microphone access in the plugin page (getUserMedia) |
//!
//! The SDK (`jsplugins_sdk.js`) is injected as `window.streamtubah` before the page's scripts.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const PERMISSIONS: [&str; 6] = ["scenes", "inputs", "outputs", "collections", "audio", "mic"];
const SDK: &str = include_str!("jsplugins_sdk.js");
const LOG_LINES: usize = 300;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    /// Entry page or script, relative to the plugin folder.
    pub main: String,
    pub permissions: Vec<String>,
    pub window: WindowSpec,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct WindowSpec {
    pub title: String,
    pub width: f64,
    pub height: f64,
}

impl Default for WindowSpec {
    fn default() -> Self {
        Self { title: String::new(), width: 480.0, height: 600.0 }
    }
}

struct Running {
    token: String,
    permissions: Vec<String>,
}

#[derive(Default)]
struct State {
    dir: PathBuf,
    approved: BTreeMap<String, Vec<String>>,
    running: HashMap<String, Running>,
    errors: HashMap<String, String>,
    logs: HashMap<String, VecDeque<Value>>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn label(id: &str) -> String {
    format!("plugin-{id}")
}

fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).map_err(|_| "manifest.json is missing".to_string())?;
    let m: Manifest = serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    if m.main.is_empty() || safe_relative(&m.main).is_none() {
        return Err("manifest.json: \"main\" must be a file inside the plugin folder".into());
    }
    if let Some(p) = m.permissions.iter().find(|p| !PERMISSIONS.contains(&p.as_str())) {
        return Err(format!("manifest.json: unknown permission '{p}' (known: {})", PERMISSIONS.join(", ")));
    }
    Ok(m)
}

/// Relative path without `..`, roots or drive prefixes.
fn safe_relative(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    if p.contains('\\') || p.contains(':') {
        return None;
    }
    path.components().all(|c| matches!(c, Component::Normal(_))).then(|| path.to_path_buf())
}

/// Plugins found on disk: (id, manifest or error).
fn discover(dir: &Path) -> Vec<(String, Result<Manifest, String>)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            valid_id(&id).then(|| (id, read_manifest(&e.path())))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn folder() -> PathBuf {
    with(|s| s.dir.clone())
}

pub fn init(app: &AppHandle, data_dir: &Path, approved: &BTreeMap<String, Vec<String>>) {
    let _ = APP.set(app.clone());
    let dir = data_dir.join("plugins-js");
    let _ = std::fs::create_dir_all(&dir);
    with(|s| s.dir = dir);
    configure(approved);
}

/// Starts approved plugins and stops the others. `approved` = enabled id → permissions the user accepted.
pub fn configure(approved: &BTreeMap<String, Vec<String>>) {
    with(|s| s.approved = approved.clone());
    let dir = folder();
    let found = discover(&dir);
    let running: Vec<String> = with(|s| s.running.keys().cloned().collect());
    for id in running {
        let keep = found.iter().any(|(fid, m)| *fid == id && m.as_ref().is_ok_and(|m| allowed_to_run(approved, &id, m).is_ok()));
        if !keep {
            stop(&id);
        }
    }
    for (id, manifest) in found {
        if !approved.contains_key(&id) || with(|s| s.running.contains_key(&id)) {
            continue;
        }
        let result = manifest.and_then(|m| {
            allowed_to_run(approved, &id, &m)?;
            start(&id, &m)
        });
        with(|s| match result {
            Ok(()) => drop(s.errors.remove(&id)),
            Err(e) => drop(s.errors.insert(id.clone(), e)),
        });
    }
    notify();
}

fn allowed_to_run(approved: &BTreeMap<String, Vec<String>>, id: &str, m: &Manifest) -> Result<(), String> {
    let ok = approved.get(id).ok_or("not enabled")?;
    match m.permissions.iter().find(|p| !ok.contains(p)) {
        Some(p) => Err(format!("asks for a new permission '{p}': enable it again to approve")),
        None => Ok(()),
    }
}

fn start(id: &str, m: &Manifest) -> Result<(), String> {
    let app = APP.get().ok_or("not initialised")?;
    if !crate::api::is_listening() {
        return Err("the remote control API is off (Settings → Remote API); plugins use it to talk to Streamtubah".into());
    }
    let port = crate::bridge::API_PORT.load(std::sync::atomic::Ordering::Relaxed);
    let token = crate::settings::random_token(32);
    let page = if m.main.ends_with(".js") { "__main.html".to_string() } else { m.main.clone() };
    let url: tauri::Url = format!("http://127.0.0.1:{port}/plugins/{id}/{page}").parse().map_err(|e| format!("{e}"))?;
    let config = json!({ "id": id, "name": m.name, "port": port, "token": token, "permissions": m.permissions });
    let script = format!("window.__STREAMTUBAH__ = {config};\n{SDK}");
    let title = if m.window.title.is_empty() { format!("{} — Streamtubah plugin", if m.name.is_empty() { id } else { &m.name }) } else { m.window.title.clone() };
    with(|s| s.running.insert(id.to_string(), Running { token, permissions: m.permissions.clone() }));
    let window = WebviewWindowBuilder::new(app, label(id), WebviewUrl::External(url))
        .title(title)
        .inner_size(m.window.width.clamp(200.0, 4000.0), m.window.height.clamp(150.0, 4000.0))
        .visible(false)
        .initialization_script(&script)
        .build()
        .map_err(|e| {
            with(|s| s.running.remove(id));
            e.to_string()
        })?;
    // The close button hides the plugin's window; the plugin keeps running.
    let w = window.clone();
    window.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            let _ = w.hide();
        }
    });
    configure_webview(&window, m.permissions.iter().any(|p| p == "mic"));
    log(id, "info", "started");
    Ok(())
}

/// Microphone prompts cannot be shown in a hidden window: answer them from the approved
/// permissions (everything else is denied). The page also keeps running at full speed while
/// hidden (no background timer throttling), like a background service.
fn configure_webview(window: &tauri::WebviewWindow, allow_mic: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let _ = window.with_webview(move |wv| unsafe {
        let controller = wv.controller();
        let _ = controller.SetIsVisible(true);
        let Ok(core) = controller.CoreWebView2() else { return };
        let handler = webview2_com::PermissionRequestedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let allow = allow_mic && kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE;
                args.SetState(if allow { COREWEBVIEW2_PERMISSION_STATE_ALLOW } else { COREWEBVIEW2_PERMISSION_STATE_DENY })?;
            }
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_PermissionRequested(&handler, &mut token);
    });
}

fn stop(id: &str) {
    with(|s| s.running.remove(id));
    if let Some(w) = APP.get().and_then(|a| a.get_webview_window(&label(id))) {
        let _ = w.destroy();
    }
    log(id, "info", "stopped");
}

pub fn reload(id: &str) -> Result<(), String> {
    stop(id);
    let approved = with(|s| s.approved.clone());
    configure(&approved);
    with(|s| s.errors.get(id).cloned()).map_or(Ok(()), Err)
}

pub fn show(id: &str, visible: bool) -> Result<(), String> {
    let w = APP.get().and_then(|a| a.get_webview_window(&label(id))).ok_or("Plugin is not running")?;
    if visible {
        let _ = w.unminimize();
        w.show().map_err(|e| e.to_string())?;
        let _ = w.set_focus();
    } else {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Restarts every enabled plugin (the API port or state changed).
pub fn restart_all() {
    shutdown();
    let approved = with(|s| s.approved.clone());
    configure(&approved);
}

/// Stops every plugin.
pub fn shutdown() {
    let ids: Vec<String> = with(|s| s.running.keys().cloned().collect());
    for id in ids {
        stop(&id);
    }
}

/// Permissions of a running plugin whose token matches.
pub fn authorize(id: &str, token: &str) -> Option<Vec<String>> {
    with(|s| s.running.get(id).filter(|r| !token.is_empty() && r.token == token).map(|r| r.permissions.clone()))
}

pub fn log(id: &str, level: &str, text: &str) {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let line = json!({ "id": id, "level": level, "text": text.chars().take(2000).collect::<String>(), "ts": ts });
    with(|s| {
        let q = s.logs.entry(id.to_string()).or_default();
        q.push_back(line.clone());
        while q.len() > LOG_LINES {
            q.pop_front();
        }
    });
    if let Some(app) = APP.get() {
        let _ = app.emit("jsplugins:log", line);
    }
}

pub fn logs(id: &str) -> Vec<Value> {
    with(|s| s.logs.get(id).map(|q| q.iter().cloned().collect()).unwrap_or_default())
}

fn notify() {
    if let Some(app) = APP.get() {
        let _ = app.emit("jsplugins:changed", ());
    }
}

/// Plugins on disk with their state, for Settings → Plugins.
pub fn list() -> Vec<Value> {
    let dir = folder();
    discover(&dir)
        .into_iter()
        .map(|(id, m)| {
            let (running, error, approved) =
                with(|s| (s.running.contains_key(&id), s.errors.get(&id).cloned(), s.approved.get(&id).cloned()));
            match m {
                Ok(m) => json!({
                    "id": id, "name": if m.name.is_empty() { id.clone() } else { m.name }, "version": m.version,
                    "description": m.description, "author": m.author, "permissions": m.permissions,
                    "enabled": approved.is_some(), "running": running, "error": error, "manifest_error": null,
                }),
                Err(e) => json!({
                    "id": id, "name": id, "version": "", "description": "", "author": "", "permissions": [],
                    "enabled": approved.is_some(), "running": false, "error": null, "manifest_error": e,
                }),
            }
        })
        .collect()
}

/// Manifest permissions of a plugin on disk (what enabling it approves).
pub fn manifest_permissions(id: &str) -> Result<Vec<String>, String> {
    if !valid_id(id) {
        return Err("Bad plugin id".into());
    }
    Ok(read_manifest(&folder().join(id))?.permissions)
}

/// A file of a running plugin, for the API's HTTP server. Paths cannot leave the plugin folder.
pub fn read_file(id: &str, rel: &str) -> Option<(Vec<u8>, &'static str)> {
    if !valid_id(id) || !with(|s| s.running.contains_key(id)) {
        return None;
    }
    let dir = folder().join(id);
    if rel == "__main.html" {
        let m = read_manifest(&dir).ok()?;
        let page = format!("<!doctype html><meta charset=\"utf-8\"><title>{}</title><body><script src=\"{}\"></script></body>", html_escape(&m.name), html_escape(&m.main));
        return Some((page.into_bytes(), "text/html; charset=utf-8"));
    }
    let rel = safe_relative(rel)?;
    let path = dir.join(rel);
    let mime = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    Some((std::fs::read(path).ok()?, mime))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Permission needed for a control API request; None = always allowed.
pub fn permission_for(request_type: &str) -> Option<&'static str> {
    Some(match request_type {
        "GetVersion" | "GetStats" | "BroadcastCustomEvent" => return None,
        t if t.contains("Scene") && !t.contains("SceneCollection") => "scenes",
        t if t.contains("Input") || t.contains("SourceFilter") => "inputs",
        t if t.contains("Stream") || t.contains("Record") || t.contains("ReplayBuffer") || t.contains("VirtualCam") => "outputs",
        t if t.contains("SceneCollection") || t.contains("Profile") => "collections",
        // Unknown requests need every permission to be on the safe side.
        _ => "all",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        assert!(safe_relative("index.html").is_some());
        assert!(safe_relative("ui/app.js").is_some());
        for bad in ["../x", "a/../../x", "/etc/passwd", "C:/x", "a\\..\\x", ""] {
            assert!(safe_relative(bad).is_none_or(|p| p.as_os_str().is_empty()), "{bad}");
        }
        assert!(valid_id("my-voice_2") && !valid_id("../x") && !valid_id(""));
    }

    #[test]
    fn request_permissions() {
        assert_eq!(permission_for("GetSceneList"), Some("scenes"));
        assert_eq!(permission_for("SetSceneItemEnabled"), Some("scenes"));
        assert_eq!(permission_for("SetCurrentSceneCollection"), Some("collections"));
        assert_eq!(permission_for("SetInputVolume"), Some("inputs"));
        assert_eq!(permission_for("CreateSourceFilter"), Some("inputs"));
        assert_eq!(permission_for("ToggleRecord"), Some("outputs"));
        assert_eq!(permission_for("StartVirtualCam"), Some("outputs"));
        assert_eq!(permission_for("CreateProfile"), Some("collections"));
        assert_eq!(permission_for("GetVersion"), None);
        assert_eq!(permission_for("Shutdown"), Some("all"));
    }

    #[test]
    fn manifest_checks() {
        let dir = std::env::temp_dir().join(format!("st-plugin-{}", crate::settings::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{"name":"X","main":"main.js","permissions":["audio","mic"]}"#).unwrap();
        assert_eq!(read_manifest(&dir).unwrap().permissions, vec!["audio", "mic"]);
        std::fs::write(dir.join("manifest.json"), r#"{"main":"main.js","permissions":["root"]}"#).unwrap();
        assert!(read_manifest(&dir).unwrap_err().contains("unknown permission"));
        std::fs::write(dir.join("manifest.json"), r#"{"main":"../evil.js"}"#).unwrap();
        assert!(read_manifest(&dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
        let approved = BTreeMap::from([("p".to_string(), vec!["audio".to_string()])]);
        let m = Manifest { permissions: vec!["audio".into(), "mic".into()], ..Default::default() };
        assert!(allowed_to_run(&approved, "p", &m).unwrap_err().contains("mic"));
    }
}
