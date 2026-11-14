//! Local WebSocket server.
//!
//! - `/`              control API compatible with a subset of obs-websocket v5, so
//!                    Stream Deck, Touch Portal, Streamer.bot, SAMMI… work unchanged.
//! - `/audio/<name>`  Audio Bridge endpoint (see bridge.rs, docs/audio-bridge.md).
//! - `/audio-source/<name>`  audio into "Plugin Audio" sources (see plugin_audio.rs).
//! - `/plugin?id=&token=`    control API for JavaScript plugins, limited to their approved
//!                    permissions (see jsplugins.rs); `/plugins/<id>/...` serves their files.
//!                    Both are loopback-only.
//!
//! Binds 127.0.0.1 unless LAN access is enabled; password is always required
//! when set (it is generated on first run).

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Listener};
use tungstenite::handshake::server::{Request, Response};
use tungstenite::{Message, WebSocket};

use crate::bridge;
use crate::obs::sys;
use crate::settings::{ApiSettings, random_token};
use crate::studio::Studio;

/// Latest CPU usage percentage (f64 bits), updated by the stats timer.
pub static CPU_USAGE: AtomicU64 = AtomicU64::new(0);

// Event subscription categories (obs-websocket v5).
const SUB_GENERAL: u32 = 1 << 0;
const SUB_SCENES: u32 = 1 << 2;
const SUB_INPUTS: u32 = 1 << 3;
const SUB_OUTPUTS: u32 = 1 << 6;
const SUB_ALL: u32 = (1 << 11) - 1;

struct Client {
    tx: Sender<String>,
    subs: Arc<AtomicU32>,
    identified: Arc<AtomicBool>,
}

struct Server {
    stop: Arc<AtomicBool>,
    /// Listener thread; joined on stop so the port is free before rebinding.
    thread: Option<std::thread::JoinHandle<()>>,
    /// (host, port) the listener is bound to.
    addr: (String, u16),
}

impl Server {
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Ctx {
    app: AppHandle,
    studio: Arc<Mutex<Studio>>,
    clients: Mutex<Vec<Client>>,
    server: Mutex<Option<Server>>,
    password: Mutex<String>,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

/// Starts the server (if enabled) and forwards app events to subscribed clients.
pub fn init(app: &AppHandle, studio: Arc<Mutex<Studio>>, cfg: &ApiSettings) {
    let _ = CTX.set(Ctx {
        app: app.clone(),
        studio,
        clients: Mutex::new(Vec::new()),
        server: Mutex::new(None),
        password: Mutex::new(String::new()),
    });
    for name in ["obs:output", "studio:scene", "studio:scenes", "studio:mute", "studio:volume"] {
        app.listen(name, move |e| {
            let payload: Value = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
            if let Some((sub, ty, data)) = map_event(name, &payload) {
                broadcast(sub, ty, data);
            }
        });
    }
    configure(cfg);
}

/// Applies new API settings, restarting the listener.
pub fn configure(cfg: &ApiSettings) {
    let Some(ctx) = CTX.get() else { return };
    // New connections check the password at handshake, so a password change needs no restart.
    *ctx.password.lock().unwrap() = cfg.password.clone();
    bridge::API_PORT.store(cfg.port, Ordering::Relaxed);
    // Never expose an unauthenticated API beyond this machine.
    let host = if cfg.allow_lan && !cfg.password.is_empty() { "0.0.0.0" } else { "127.0.0.1" };
    let wanted = (host.to_string(), cfg.port);
    let mut server = ctx.server.lock().unwrap();
    if cfg.enabled && server.as_ref().is_some_and(|s| s.addr == wanted) {
        return;
    }
    if let Some(old) = server.take() {
        old.shutdown();
    }
    if !cfg.enabled {
        return;
    }
    let listener = match TcpListener::bind((host, cfg.port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("API: cannot listen on {host}:{}: {e}", cfg.port);
            return;
        }
    };
    let _ = listener.set_nonblocking(true);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    std::thread::spawn(move || handle_connection(stream));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    *server = Some(Server { stop, thread: Some(thread), addr: wanted });
}

/// True while the API listener is running (plugins depend on it).
pub fn is_listening() -> bool {
    CTX.get().is_some_and(|c| c.server.lock().unwrap().is_some())
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
}

/// Audio endpoints accept the API password or a plugin token with the "audio" permission.
fn audio_authorized(query: &str, loopback: bool) -> bool {
    let pw = password();
    if pw.is_empty() || query_param(query, "password") == Some(pw.as_str()) {
        return true;
    }
    let plugin = (query_param(query, "plugin"), query_param(query, "token"));
    loopback
        && matches!(plugin, (Some(id), Some(token))
            if crate::jsplugins::authorize(id, token).is_some_and(|p| p.iter().any(|p| p == "audio")))
}

fn password() -> String {
    CTX.get().map(|c| c.password.lock().unwrap().clone()).unwrap_or_default()
}

const OVERLAY_HTML: &str = include_str!("chat/overlay.html");

/// Plain HTTP GET (not a WebSocket upgrade): serves the chat overlay page.
const ALERTS_HTML: &str = include_str!("alerts/overlay.html");
const WIDGETS_HTML: &str = include_str!("alerts/widgets.html");

/// Alert styles for the overlay (read from the live settings).
fn alerts_config() -> Option<String> {
    let ctx = CTX.get()?;
    let s = ctx.studio.lock().ok()?;
    if crate::EXITING.load(Ordering::SeqCst) {
        return None;
    }
    serde_json::to_string(&s.settings.alerts).ok()
}

fn serve_http(mut stream: TcpStream, head: &str, loopback: bool) {
    use std::io::{Read, Write};
    // Consume the request headers (they were only peeked).
    let mut consumed = [0u8; 2048];
    let _ = stream.read(&mut consumed[..head.len().min(2048)]);
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let route = path.split('?').next().unwrap_or("/");
    let html = "text/html; charset=utf-8";
    let (status, ctype, body): (&str, &str, Vec<u8>) = match route {
        "/overlay/chat" => ("200 OK", html, OVERLAY_HTML.as_bytes().to_vec()),
        "/overlay/alerts" => ("200 OK", html, ALERTS_HTML.as_bytes().to_vec()),
        "/overlay/widgets" => ("200 OK", html, WIDGETS_HTML.as_bytes().to_vec()),
        "/alerts/config" => match alerts_config() {
            Some(json) => ("200 OK", "application/json", json.into_bytes()),
            None => ("503 Service Unavailable", "text/plain", b"unavailable".to_vec()),
        },
        r if r.starts_with("/plugins/") && loopback => {
            let file = r["/plugins/".len()..]
                .split_once('/')
                .and_then(|(id, rel)| crate::jsplugins::read_file(id, &percent_decode(rel)));
            match file {
                Some((bytes, mime)) => ("200 OK", mime, bytes),
                None => ("404 Not Found", "text/plain", b"Not found".to_vec()),
            }
        }
        r => match r.strip_prefix("/alerts/media/").and_then(crate::alerts::read_media) {
            Some((bytes, mime)) => ("200 OK", mime, bytes),
            None => ("404 Not Found", "text/plain", b"Not found".to_vec()),
        },
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    // Close gracefully: an unread request left in the socket makes Windows reset the
    // connection, which can cut off the end of the response.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut sink = [0u8; 1024];
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
}

/// Alert Box overlay stream: {"type": "alert" | "config" | "skip", ...}.
fn alerts_session(mut ws: WebSocket<TcpStream>) {
    let (tx, rx) = mpsc::channel::<String>();
    crate::alerts::subscribe(tx);
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(200)));
    loop {
        while let Ok(msg) = rx.try_recv() {
            if ws.send(Message::text(msg)).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(_) => return,
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn handle_connection(stream: TcpStream) {
    let loopback = stream.peer_addr().is_ok_and(|a| a.ip().is_loopback());
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 2048];
    let n = stream.peek(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]).to_string();
    if !head.to_ascii_lowercase().contains("upgrade: websocket") {
        serve_http(stream, &head, loopback);
        return;
    }
    let _ = stream.set_read_timeout(None);
    let mut path = String::new();
    let ws = tungstenite::accept_hdr(stream, |req: &Request, resp: Response| {
        path = req.uri().to_string();
        Ok(resp)
    });
    let Ok(ws) = ws else { return };
    let (route, query) = path.split_once('?').unwrap_or((&path, ""));
    if let Some(name) = route.strip_prefix("/audio/") {
        audio_session(ws, name.to_string(), query, loopback);
    } else if let Some(name) = route.strip_prefix("/audio-source/") {
        audio_source_session(ws, name.to_string(), query, loopback);
    } else if route == "/plugin" {
        let id = query_param(query, "id").unwrap_or("").to_string();
        match crate::jsplugins::authorize(&id, query_param(query, "token").unwrap_or("")).filter(|_| loopback) {
            Some(perms) => control_session(ws, Some(PluginAuth { id, perms })),
            None => {
                let mut ws = ws;
                let _ = ws.close(Some(close_frame(4009, "Authentication failed")));
            }
        }
    } else if route == "/chat" {
        chat_session(ws);
    } else if route == "/alerts" {
        alerts_session(ws);
    } else {
        control_session(ws, None);
    }
}

fn is_timeout(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

// ---------------------------------------------------------------------------
// Chat overlay stream (read-only public chat; no password so Browser Sources can use it)

fn chat_session(mut ws: WebSocket<TcpStream>) {
    let (tx, rx) = mpsc::channel::<String>();
    for msg in crate::chat::subscribe(tx) {
        if ws.send(Message::text(msg)).is_err() {
            return;
        }
    }
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(200)));
    loop {
        while let Ok(msg) = rx.try_recv() {
            if ws.send(Message::text(msg)).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(_) => return,
        }
    }
}

// ---------------------------------------------------------------------------
// Audio Bridge session

/// Audio pushed into Plugin Audio sources: binary 48 kHz stereo interleaved f32le blocks.
fn audio_source_session(mut ws: WebSocket<TcpStream>, name: String, query: &str, loopback: bool) {
    if !audio_authorized(query, loopback) {
        let _ = ws.close(Some(close_frame(4009, "Authentication failed")));
        return;
    }
    let hello = json!({
        "type": "hello", "channel": name, "sample_rate": bridge::SAMPLE_RATE,
        "channels": bridge::CHANNELS, "format": "f32le", "layout": "interleaved",
    });
    let _ = ws.send(Message::text(hello.to_string()));
    let mut clock = crate::plugin_audio::Clock::new();
    loop {
        match ws.read() {
            Ok(Message::Binary(b)) => {
                crate::plugin_audio::push(&name, &b, &mut clock);
            }
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

fn audio_session(mut ws: WebSocket<TcpStream>, name: String, query: &str, loopback: bool) {
    if !audio_authorized(query, loopback) {
        let _ = ws.close(Some(close_frame(4009, "Authentication failed")));
        return;
    }
    let ch = bridge::channel(&name);
    if ch.connected.load(Ordering::Acquire) {
        let _ = ws.close(Some(close_frame(4010, "Channel already has a client")));
        return;
    }
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(64);
    ch.attach(tx);
    let hello = json!({
        "type": "hello", "channel": name, "sample_rate": bridge::SAMPLE_RATE,
        "channels": bridge::CHANNELS, "format": "f32le", "layout": "interleaved",
    });
    let _ = ws.send(Message::text(hello.to_string()));
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(5)));
    'session: loop {
        while let Ok(block) = rx.try_recv() {
            let bytes: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();
            if ws.send(Message::binary(bytes)).is_err() {
                break 'session;
            }
        }
        match ws.read() {
            Ok(Message::Binary(b)) => {
                let usable = b.len() / 4 / bridge::CHANNELS * bridge::CHANNELS * 4;
                ch.push_from_client(b[..usable].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())));
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(_) => break,
        }
    }
    ch.detach();
}

fn close_frame(code: u16, reason: &str) -> tungstenite::protocol::CloseFrame {
    tungstenite::protocol::CloseFrame { code: code.into(), reason: reason.to_string().into() }
}

// ---------------------------------------------------------------------------
// Control session (obs-websocket v5 subset)

fn sha256_b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(s.as_bytes()))
}

/// A JavaScript plugin connected through `/plugin` (already authenticated by its token).
struct PluginAuth {
    id: String,
    perms: Vec<String>,
}

impl PluginAuth {
    fn has(&self, p: &str) -> bool {
        self.perms.iter().any(|x| x == p)
    }

    /// Event categories the plugin may receive.
    fn event_mask(&self) -> u32 {
        let mut mask = SUB_GENERAL;
        for (perm, sub) in [("scenes", SUB_SCENES), ("inputs", SUB_INPUTS), ("outputs", SUB_OUTPUTS)] {
            if self.has(perm) {
                mask |= sub;
            }
        }
        mask
    }

    /// Plugin-only requests and permission checks. Some(response) = handled here.
    fn intercept(&self, d: &Value) -> Option<Value> {
        let ty = d["requestType"].as_str().unwrap_or("");
        let data = &d["requestData"];
        let mut out = json!({ "requestType": ty, "requestId": d["requestId"], "requestStatus": { "result": true, "code": 100 } });
        match ty {
            "PluginLog" => {
                crate::jsplugins::log(&self.id, data["level"].as_str().unwrap_or("info"), data["text"].as_str().unwrap_or(""));
            }
            "PluginSetWindowVisible" => {
                let (id, visible) = (self.id.clone(), data["visible"].as_bool().unwrap_or(true));
                // Window calls go through the UI thread: never block this socket on them.
                std::thread::spawn(move || {
                    let _ = crate::jsplugins::show(&id, visible);
                });
            }
            "PluginInfo" => out["responseData"] = json!({ "id": self.id, "permissions": self.perms }),
            _ => match crate::jsplugins::permission_for(ty) {
                Some(p) if !self.has(p) => {
                    out["requestStatus"] = json!({
                        "result": false, "code": 403,
                        "comment": format!("This plugin has no '{p}' permission (add it to manifest.json and enable the plugin again)"),
                    });
                }
                _ => return None,
            },
        }
        Some(out)
    }
}

fn control_session(mut ws: WebSocket<TcpStream>, plugin: Option<PluginAuth>) {
    let Some(ctx) = CTX.get() else { return };
    // Plugins already authenticated with their token in the URL.
    let pw = if plugin.is_some() { String::new() } else { password() };
    let (salt, challenge) = (random_token(24), random_token(24));
    let mut hello = json!({ "obsWebSocketVersion": "5.5.0", "rpcVersion": 1 });
    if !pw.is_empty() {
        hello["authentication"] = json!({ "challenge": challenge, "salt": salt });
    }
    if ws.send(Message::text(json!({ "op": 0, "d": hello }).to_string())).is_err() {
        return;
    }

    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    let subs = Arc::new(AtomicU32::new(0));
    let identified = Arc::new(AtomicBool::new(false));
    ctx.clients.lock().unwrap().push(Client { tx, subs: subs.clone(), identified: identified.clone() });
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(50)));

    loop {
        while let Ok(msg) = rx.try_recv() {
            if ws.send(Message::text(msg)).is_err() {
                return;
            }
        }
        let text = match ws.read() {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) => return,
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(_) => return,
        };
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
        let d = &msg["d"];
        let reply = match msg["op"].as_u64() {
            Some(1) => {
                if !pw.is_empty() {
                    let expected = sha256_b64(&(sha256_b64(&(pw.clone() + &salt)) + &challenge));
                    if d["authentication"].as_str() != Some(expected.as_str()) {
                        let _ = ws.close(Some(close_frame(4009, "Authentication failed")));
                        return;
                    }
                }
                let mask = plugin.as_ref().map_or(u32::MAX, PluginAuth::event_mask);
                subs.store(d["eventSubscriptions"].as_u64().map_or(SUB_ALL & !(1 << 16), |v| v as u32) & mask, Ordering::Relaxed);
                identified.store(true, Ordering::Relaxed);
                json!({ "op": 2, "d": { "negotiatedRpcVersion": 1 } })
            }
            Some(3) if identified.load(Ordering::Relaxed) => {
                if let Some(v) = d["eventSubscriptions"].as_u64() {
                    subs.store(v as u32 & plugin.as_ref().map_or(u32::MAX, PluginAuth::event_mask), Ordering::Relaxed);
                }
                json!({ "op": 2, "d": { "negotiatedRpcVersion": 1 } })
            }
            Some(6) if identified.load(Ordering::Relaxed) => {
                let res = plugin.as_ref().and_then(|p| p.intercept(d)).unwrap_or_else(|| run_request(&ctx.studio, d));
                json!({ "op": 7, "d": res })
            }
            Some(8) if identified.load(Ordering::Relaxed) => {
                let halt = d["haltOnFailure"].as_bool().unwrap_or(false);
                let mut results = Vec::new();
                for r in d["requests"].as_array().into_iter().flatten() {
                    let res = plugin.as_ref().and_then(|p| p.intercept(r)).unwrap_or_else(|| run_request(&ctx.studio, r));
                    let failed = res["requestStatus"]["result"] == false;
                    results.push(res);
                    if failed && halt {
                        break;
                    }
                }
                json!({ "op": 9, "d": { "requestId": d["requestId"], "results": results } })
            }
            _ => {
                let _ = ws.close(Some(close_frame(4007, "Not identified")));
                return;
            }
        };
        if ws.send(Message::text(reply.to_string())).is_err() {
            return;
        }
    }
}

fn broadcast(sub: u32, event_type: &str, data: Value) {
    let Some(ctx) = CTX.get() else { return };
    let msg = json!({ "op": 5, "d": { "eventType": event_type, "eventIntent": sub, "eventData": data } }).to_string();
    ctx.clients.lock().unwrap().retain(|c| {
        if !c.identified.load(Ordering::Relaxed) || c.subs.load(Ordering::Relaxed) & sub == 0 {
            return true;
        }
        c.tx.send(msg.clone()).is_ok()
    });
}

/// Translates internal app events into obs-websocket events.
fn map_event(name: &str, p: &Value) -> Option<(u32, &'static str, Value)> {
    match name {
        "studio:scene" => Some((SUB_SCENES, "CurrentProgramSceneChanged", json!({ "sceneName": p["name"], "sceneUuid": "" }))),
        "studio:scenes" => {
            let names = p.as_array().cloned().unwrap_or_default();
            let scenes: Vec<Value> = names.iter().enumerate().rev()
                .map(|(i, n)| json!({ "sceneName": n, "sceneUuid": "", "sceneIndex": names.len() - 1 - i })).collect();
            Some((SUB_SCENES, "SceneListChanged", json!({ "scenes": scenes })))
        }
        "studio:mute" => Some((SUB_INPUTS, "InputMuteStateChanged", json!({ "inputName": p["name"], "inputUuid": "", "inputMuted": p["muted"] }))),
        "studio:volume" => Some((SUB_INPUTS, "InputVolumeChanged", json!({
            "inputName": p["name"], "inputUuid": "", "inputVolumeMul": p["mul"], "inputVolumeDb": p["db"],
        }))),
        "obs:output" => {
            let signal = p["signal"].as_str().unwrap_or("");
            if signal == "saved" {
                return Some((SUB_OUTPUTS, "ReplayBufferSaved", json!({ "savedReplayPath": p["path"] })));
            }
            let (state, active) = match signal {
                "starting" => ("OBS_WEBSOCKET_OUTPUT_STARTING", false),
                "start" => ("OBS_WEBSOCKET_OUTPUT_STARTED", true),
                "stopping" => ("OBS_WEBSOCKET_OUTPUT_STOPPING", true),
                "stop" => ("OBS_WEBSOCKET_OUTPUT_STOPPED", false),
                "reconnect" => ("OBS_WEBSOCKET_OUTPUT_RECONNECTING", true),
                "reconnect_success" => ("OBS_WEBSOCKET_OUTPUT_RECONNECTED", true),
                _ => return None,
            };
            let ty = match p["output"].as_str() {
                // obs-websocket has one stream; extra destinations are not reported separately.
                Some(o) if o.starts_with("stream:") => return None,
                Some("stream") => "StreamStateChanged",
                Some("record") => "RecordStateChanged",
                Some("virtualcam") => "VirtualcamStateChanged",
                _ => "ReplayBufferStateChanged",
            };
            Some((SUB_OUTPUTS, ty, json!({ "outputActive": active, "outputState": state })))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Requests

const REQUESTS: &[&str] = &[
    "GetVersion", "GetStats", "BroadcastCustomEvent", "GetSceneList", "GetCurrentProgramScene",
    "SetCurrentProgramScene", "CreateScene", "RemoveScene", "GetInputList", "GetInputMute", "SetInputMute",
    "ToggleInputMute", "GetInputVolume", "SetInputVolume", "GetInputSettings", "SetInputSettings",
    "CreateInput", "RemoveInput", "GetSceneItemList", "GetSceneItemId", "GetSceneItemEnabled", "SetSceneItemEnabled", "GetSourceFilterList",
    "SetSourceFilterEnabled", "CreateSourceFilter", "RemoveSourceFilter", "GetStreamStatus", "StartStream", "StopStream", "ToggleStream", "GetRecordStatus",
    "StartRecord", "StopRecord", "ToggleRecord", "PauseRecord", "ResumeRecord", "ToggleRecordPause",
    "GetReplayBufferStatus", "StartReplayBuffer", "StopReplayBuffer", "ToggleReplayBuffer", "SaveReplayBuffer",
    "GetVirtualCamStatus", "StartVirtualCam", "StopVirtualCam", "ToggleVirtualCam",
    "GetSceneCollectionList", "SetCurrentSceneCollection", "CreateSceneCollection",
    "GetProfileList", "SetCurrentProfile", "CreateProfile", "RemoveProfile",
];

type ReqResult = std::result::Result<Value, (u16, String)>;

fn field<'a>(d: &'a Value, name: &str) -> std::result::Result<&'a Value, (u16, String)> {
    let v = &d[name];
    if v.is_null() { Err((300, format!("Missing request field '{name}'"))) } else { Ok(v) }
}

fn str_field<'a>(d: &'a Value, name: &str) -> std::result::Result<&'a str, (u16, String)> {
    field(d, name)?.as_str().ok_or((400, format!("Field '{name}' must be a string")))
}

fn failed(e: String) -> (u16, String) {
    if e.contains("not found") { (600, e) } else { (702, e) }
}

fn timecode(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn run_request(studio: &Arc<Mutex<Studio>>, d: &Value) -> Value {
    let ty = d["requestType"].as_str().unwrap_or("").to_string();
    let data = d.get("requestData").cloned().unwrap_or(json!({}));
    let result = match studio.lock() {
        Ok(_) if crate::EXITING.load(Ordering::SeqCst) => Err((702, "Shutting down".into())),
        Ok(mut s) => request(&mut s, &ty, &data),
        Err(_) => Err((702, "Studio unavailable".into())),
    };
    // Requests that change something refresh the app UI (remote scene switch, mute, …).
    // Read-only requests are skipped: controllers poll them several times per second.
    if result.is_ok() && !ty.starts_with("Get") && ty != "BroadcastCustomEvent" {
        if let Some(ctx) = CTX.get() {
            let _ = ctx.app.emit("studio:changed", Value::Null);
        }
    }
    let mut out = json!({ "requestType": ty, "requestId": d["requestId"] });
    match result {
        Ok(v) => {
            out["requestStatus"] = json!({ "result": true, "code": 100 });
            if !v.is_null() {
                out["responseData"] = v;
            }
        }
        Err((code, comment)) => out["requestStatus"] = json!({ "result": false, "code": code, "comment": comment }),
    }
    out
}

fn output_status(s: &Studio, kind: &str) -> Value {
    let d = s.outputs.detail(kind);
    let ms = d["duration_ms"].as_u64().unwrap_or(0);
    json!({
        "outputActive": d["active"], "outputPaused": d["paused"], "outputReconnecting": false,
        "outputTimecode": timecode(ms), "outputDuration": ms, "outputBytes": d["bytes"],
        "outputCongestion": d["congestion"], "outputSkippedFrames": d["dropped"], "outputTotalFrames": d["total"],
    })
}

fn toggle(s: &mut Studio, kind: &str, want: Option<bool>) -> ReqResult {
    let active = s.outputs.detail(kind)["active"] == true;
    let target = want.unwrap_or(!active);
    match (active, target) {
        (true, true) => return Err((500, "Output is already running".into())),
        (false, false) => return Err((501, "Output is not running".into())),
        (false, true) => s.start_output(kind).map_err(failed)?,
        (true, false) => s.outputs.stop(kind),
    }
    Ok(if want.is_none() { json!({ "outputActive": target }) } else { Value::Null })
}

fn request(s: &mut Studio, ty: &str, d: &Value) -> ReqResult {
    Ok(match ty {
        "GetVersion" => json!({
            "obsVersion": "32.1.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
            "availableRequests": REQUESTS, "supportedImageFormats": [],
            "platform": "windows", "platformDescription": "Streamtubah",
        }),
        "GetStats" => unsafe {
            let video = sys::obs_get_video();
            json!({
                "cpuUsage": f64::from_bits(CPU_USAGE.load(Ordering::Relaxed)),
                "memoryUsage": 0, "availableDiskSpace": 0,
                "activeFps": sys::obs_get_active_fps(),
                "averageFrameRenderTime": sys::obs_get_average_frame_time_ns() as f64 / 1_000_000.0,
                "renderSkippedFrames": sys::obs_get_lagged_frames(), "renderTotalFrames": sys::obs_get_total_frames(),
                "outputSkippedFrames": sys::video_output_get_skipped_frames(video),
                "outputTotalFrames": sys::video_output_get_total_frames(video),
                "webSocketSessionIncomingMessages": 0, "webSocketSessionOutgoingMessages": 0,
            })
        },
        "BroadcastCustomEvent" => {
            broadcast(SUB_GENERAL, "CustomEvent", field(d, "eventData")?.clone());
            Value::Null
        }
        "GetStudioModeEnabled" => json!({ "studioModeEnabled": s.studio_mode() }),
        "SetStudioModeEnabled" => {
            s.studio_mode_set(field(d, "studioModeEnabled")?.as_bool().unwrap_or(false));
            Value::Null
        }
        "GetCurrentPreviewScene" => {
            let n = s.preview_scene_name().ok_or((506, "Studio Mode is not active".to_string()))?;
            json!({ "sceneName": n, "sceneUuid": "", "currentPreviewSceneName": n, "currentPreviewSceneUuid": "" })
        }
        "SetCurrentPreviewScene" => {
            if !s.studio_mode() {
                return Err((506, "Studio Mode is not active".into()));
            }
            s.scene_set_current(str_field(d, "sceneName")?).map_err(failed)?;
            Value::Null
        }
        "TriggerStudioModeTransition" => {
            s.studio_transition().map_err(|e| (506, e))?;
            Value::Null
        }
        "GetSceneList" => {
            let names = s.scene_names();
            let scenes: Vec<Value> = names.iter().enumerate().rev()
                .map(|(i, n)| json!({ "sceneName": n, "sceneUuid": "", "sceneIndex": names.len() - 1 - i })).collect();
            json!({
                "currentProgramSceneName": s.current_scene_name(), "currentProgramSceneUuid": "",
                "currentPreviewSceneName": s.preview_scene_name(), "currentPreviewSceneUuid": null, "scenes": scenes,
            })
        }
        "GetCurrentProgramScene" => {
            let n = s.current_scene_name();
            json!({ "sceneName": n, "sceneUuid": "", "currentProgramSceneName": n, "currentProgramSceneUuid": "" })
        }
        "SetCurrentProgramScene" => {
            s.scene_set_program(str_field(d, "sceneName")?).map_err(failed)?;
            Value::Null
        }
        "CreateScene" => {
            s.scene_create(str_field(d, "sceneName")?).map_err(failed)?;
            json!({ "sceneUuid": "" })
        }
        "RemoveScene" => {
            s.scene_remove(str_field(d, "sceneName")?).map_err(failed)?;
            Value::Null
        }
        "GetInputList" => {
            let kind = d["inputKind"].as_str();
            let inputs: Vec<Value> = s.input_list().into_iter()
                .filter(|(_, k)| kind.is_none_or(|w| w == k))
                .map(|(n, k)| json!({ "inputName": n, "inputKind": k, "unversionedInputKind": k, "inputUuid": "" }))
                .collect();
            json!({ "inputs": inputs })
        }
        "GetInputMute" => {
            let src = s.source(str_field(d, "inputName")?).map_err(failed)?;
            json!({ "inputMuted": unsafe { sys::obs_source_muted(src.as_ptr()) } })
        }
        "SetInputMute" => {
            let muted = field(d, "inputMuted")?.as_bool().unwrap_or(false);
            s.audio_set_muted(str_field(d, "inputName")?, muted).map_err(failed)?;
            Value::Null
        }
        "ToggleInputMute" => {
            let name = str_field(d, "inputName")?;
            let src = s.source(name).map_err(failed)?;
            let muted = !unsafe { sys::obs_source_muted(src.as_ptr()) };
            s.audio_set_muted(name, muted).map_err(failed)?;
            json!({ "inputMuted": muted })
        }
        "GetInputVolume" => {
            let src = s.source(str_field(d, "inputName")?).map_err(failed)?;
            let mul = unsafe { sys::obs_source_get_volume(src.as_ptr()) };
            let db = if mul <= 0.0 { -100.0 } else { 20.0 * mul.log10() };
            json!({ "inputVolumeMul": mul, "inputVolumeDb": db })
        }
        "SetInputVolume" => {
            let db = match (d["inputVolumeDb"].as_f64(), d["inputVolumeMul"].as_f64()) {
                (Some(db), _) => db as f32,
                (None, Some(mul)) => if mul <= 0.0 { -100.0 } else { 20.0 * (mul as f32).log10() },
                _ => return Err((300, "Missing inputVolumeDb or inputVolumeMul".into())),
            };
            s.audio_set_volume(str_field(d, "inputName")?, db).map_err(failed)?;
            Value::Null
        }
        "GetInputSettings" => {
            let src = s.source(str_field(d, "inputName")?).map_err(failed)?;
            json!({ "inputSettings": src.settings().to_json(), "inputKind": src.id() })
        }
        "SetInputSettings" => {
            let name = str_field(d, "inputName")?.to_string();
            let patch = field(d, "inputSettings")?.clone();
            s.properties_update(&crate::studio::PropTarget::Source { name }, &patch).map_err(failed)?;
            Value::Null
        }
        "CreateInput" => {
            let id = s
                .input_create(
                    str_field(d, "sceneName")?,
                    str_field(d, "inputKind")?,
                    str_field(d, "inputName")?,
                    d.get("inputSettings").unwrap_or(&Value::Null),
                    d["sceneItemEnabled"].as_bool().unwrap_or(true),
                )
                .map_err(failed)?;
            json!({ "sceneItemId": id, "inputUuid": "" })
        }
        "RemoveInput" => {
            s.input_remove(str_field(d, "inputName")?).map_err(failed)?;
            Value::Null
        }
        "GetSceneItemList" => {
            let items = s.scene_item_list(str_field(d, "sceneName")?).map_err(failed)?;
            let list: Vec<Value> = items.into_iter().enumerate()
                .map(|(i, (id, name, kind, enabled, locked))| json!({
                    "sceneItemId": id, "sceneItemIndex": i, "sourceName": name, "sourceUuid": "",
                    "sceneItemEnabled": enabled, "sceneItemLocked": locked, "inputKind": kind,
                    "isGroup": false, "sourceType": "OBS_SOURCE_TYPE_INPUT",
                }))
                .collect();
            json!({ "sceneItems": list })
        }
        "GetSceneItemId" => {
            let wanted = str_field(d, "sourceName")?;
            let items = s.scene_item_list(str_field(d, "sceneName")?).map_err(failed)?;
            let id = items.iter().find(|i| i.1 == wanted).map(|i| i.0)
                .ok_or((600, format!("Source '{wanted}' not found in scene")))?;
            json!({ "sceneItemId": id })
        }
        "GetSceneItemEnabled" => {
            let id = field(d, "sceneItemId")?.as_i64().unwrap_or(-1);
            let items = s.scene_item_list(str_field(d, "sceneName")?).map_err(failed)?;
            let item = items.iter().find(|i| i.0 == id).ok_or((600, "Scene item not found".to_string()))?;
            json!({ "sceneItemEnabled": item.3 })
        }
        "SetSceneItemEnabled" => {
            let id = field(d, "sceneItemId")?.as_i64().unwrap_or(-1);
            let enabled = field(d, "sceneItemEnabled")?.as_bool().unwrap_or(true);
            s.scene_item_set_enabled(str_field(d, "sceneName")?, id, enabled).map_err(failed)?;
            Value::Null
        }
        "GetSourceFilterList" => {
            let filters = s.filters(str_field(d, "sourceName")?).map_err(failed)?;
            let list: Vec<Value> = filters.into_iter().enumerate()
                .map(|(i, f)| json!({
                    "filterEnabled": f["enabled"], "filterIndex": i, "filterKind": f["type_id"],
                    "filterName": f["name"], "filterSettings": {},
                }))
                .collect();
            json!({ "filters": list })
        }
        "SetSourceFilterEnabled" => {
            let enabled = field(d, "filterEnabled")?.as_bool().unwrap_or(true);
            s.filter_set_enabled(str_field(d, "sourceName")?, str_field(d, "filterName")?, enabled).map_err(failed)?;
            Value::Null
        }
        "CreateSourceFilter" => {
            let source = str_field(d, "sourceName")?.to_string();
            let created = s.filter_add(&source, str_field(d, "filterKind")?, str_field(d, "filterName")?).map_err(failed)?;
            if let Some(settings) = d.get("filterSettings").filter(|v| v.is_object()) {
                let target = crate::studio::PropTarget::Filter { source, filter: created };
                s.properties_update(&target, settings).map_err(failed)?;
            }
            Value::Null
        }
        "RemoveSourceFilter" => {
            s.filter_remove(str_field(d, "sourceName")?, str_field(d, "filterName")?).map_err(failed)?;
            Value::Null
        }
        "GetStreamStatus" => output_status(s, "stream"),
        "GetRecordStatus" => output_status(s, "record"),
        "GetReplayBufferStatus" => json!({ "outputActive": s.outputs.detail("replay")["active"] }),
        "StartStream" => toggle(s, "stream", Some(true))?,
        "StopStream" => toggle(s, "stream", Some(false))?,
        "ToggleStream" => toggle(s, "stream", None)?,
        "StartRecord" => toggle(s, "record", Some(true))?,
        "StopRecord" => {
            toggle(s, "record", Some(false))?;
            json!({ "outputPath": null })
        }
        "ToggleRecord" => toggle(s, "record", None)?,
        "PauseRecord" => {
            s.outputs.set_record_paused(true).map_err(failed)?;
            Value::Null
        }
        "ResumeRecord" => {
            s.outputs.set_record_paused(false).map_err(failed)?;
            Value::Null
        }
        "ToggleRecordPause" => {
            let paused = s.outputs.detail("record")["paused"] == true;
            s.outputs.set_record_paused(!paused).map_err(failed)?;
            Value::Null
        }
        "StartReplayBuffer" => toggle(s, "replay", Some(true))?,
        "StopReplayBuffer" => toggle(s, "replay", Some(false))?,
        "ToggleReplayBuffer" => toggle(s, "replay", None)?,
        "SaveReplayBuffer" => {
            s.outputs.save_replay().map_err(failed)?;
            Value::Null
        }
        "GetVirtualCamStatus" => json!({ "outputActive": s.outputs.is_running("virtualcam") }),
        "StartVirtualCam" | "StopVirtualCam" | "ToggleVirtualCam" => {
            // Registering the camera needs a UAC prompt, which only the app UI may trigger.
            if !s.outputs.is_running("virtualcam") && !crate::virtualcam::installed() {
                return Err((500, "Virtual camera is not installed yet; start it once from the app".into()));
            }
            let want = match ty {
                "StartVirtualCam" => Some(true),
                "StopVirtualCam" => Some(false),
                _ => None,
            };
            toggle(s, "virtualcam", want)?
        }
        "GetSceneCollectionList" => {
            let c = s.collections();
            json!({ "currentSceneCollectionName": c["current"], "sceneCollections": c["list"] })
        }
        "SetCurrentSceneCollection" => {
            s.collection_switch(str_field(d, "sceneCollectionName")?).map_err(failed)?;
            Value::Null
        }
        "CreateSceneCollection" => {
            s.collection_create(str_field(d, "sceneCollectionName")?).map_err(failed)?;
            Value::Null
        }
        "GetProfileList" => {
            let p = s.profiles();
            json!({ "currentProfileName": p["current"], "profiles": p["list"] })
        }
        "SetCurrentProfile" => {
            s.profile_switch(str_field(d, "profileName")?).map_err(failed)?;
            Value::Null
        }
        "CreateProfile" => {
            s.profile_create(str_field(d, "profileName")?).map_err(failed)?;
            Value::Null
        }
        "RemoveProfile" => {
            s.profile_remove(str_field(d, "profileName")?).map_err(failed)?;
            Value::Null
        }
        _ => return Err((204, format!("Unknown request type '{ty}'"))),
    })
}
