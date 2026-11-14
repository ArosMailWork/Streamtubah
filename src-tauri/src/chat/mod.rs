//! Unified read-only chat from several platforms (no login required).
//! Each platform runs in its own thread with reconnect/backoff; messages are
//! normalized into `ChatMessage`, kept in a short history, emitted to the UI
//! (`chat:message`) and streamed to overlay clients (`/chat` on the API port).

pub mod actions;
mod kick;
pub(crate) mod raw_http;
mod twitch;
mod youtube;

use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tungstenite::WebSocket;
use tungstenite::stream::MaybeTlsStream;

use crate::settings::ChatSettings;

const HISTORY: usize = 300;
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

#[derive(Serialize, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub platform: &'static str,
    /// Platform's own message id (for delete).
    pub msg_id: Option<String>,
    /// Platform's own user id (for timeout / ban / mod).
    pub author_id: Option<String>,
    pub author: String,
    pub color: Option<String>,
    pub text: String,
    pub badges: Vec<String>,
    /// "message" | "paid" (Super Chat, bits, tips) | "membership" (subs, members) | "system"
    pub kind: &'static str,
    pub amount: Option<String>,
    pub ts: u64,
}

impl ChatMessage {
    pub fn new(platform: &'static str, author: impl Into<String>, text: impl Into<String>) -> Self {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Self {
            id: format!("{platform}-{ts}-{}", rand_suffix()),
            platform,
            msg_id: None,
            author_id: None,
            author: author.into(),
            color: None,
            text: text.into(),
            badges: Vec::new(),
            kind: "message",
            amount: None,
            ts,
        }
    }
}

fn rand_suffix() -> u32 {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    u32::from_le_bytes(b)
}

/// Per-platform connection state shown in the UI.
#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub state: &'static str, // "off" | "connecting" | "connected" | "error"
    pub detail: String,
}

/// Per-platform ids needed for authenticated actions, learned while reading chat.
#[derive(Clone, Debug, Default)]
pub struct ChannelContext {
    /// Twitch room id / Kick broadcaster user id.
    pub broadcaster_id: Option<String>,
    /// YouTube live video id.
    pub video_id: Option<String>,
}

struct Hub {
    app: AppHandle,
    context: Mutex<HashMap<&'static str, ChannelContext>>,
    history: Mutex<VecDeque<ChatMessage>>,
    status: Mutex<HashMap<&'static str, Status>>,
    overlay: Mutex<Vec<Sender<String>>>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

pub fn init(app: &AppHandle, cfg: &ChatSettings) {
    let _ = HUB.set(Hub {
        app: app.clone(),
        context: Mutex::new(HashMap::new()),
        history: Mutex::new(VecDeque::new()),
        status: Mutex::new(HashMap::new()),
        overlay: Mutex::new(Vec::new()),
        stop: Mutex::new(None),
    });
    configure(cfg);
}

/// (Re)starts connectors for the configured channels.
pub fn configure(cfg: &ChatSettings) {
    let Some(hub) = HUB.get() else { return };
    if let Some(old) = hub.stop.lock().unwrap().take() {
        old.store(true, Ordering::Relaxed);
    }
    hub.status.lock().unwrap().clear();
    let stop = Arc::new(AtomicBool::new(false));
    *hub.stop.lock().unwrap() = Some(stop.clone());

    let spawn = |platform: &'static str, target: String, run: fn(&str, &AtomicBool) -> Result<(), String>| {
        if target.trim().is_empty() {
            set_status(platform, "off", "");
            return;
        }
        let stop = stop.clone();
        std::thread::spawn(move || supervise(platform, target.trim(), &stop, run));
    };
    spawn("twitch", cfg.twitch.clone(), twitch::run);
    spawn("kick", cfg.kick.clone(), kick::run);
    spawn("youtube", cfg.youtube.clone(), youtube::run);
}

/// Keeps a connector alive with exponential backoff until stopped.
fn supervise(platform: &'static str, target: &str, stop: &AtomicBool, run: fn(&str, &AtomicBool) -> Result<(), String>) {
    let mut delay = 2;
    while !stop.load(Ordering::Relaxed) {
        set_status(platform, "connecting", target);
        let started = std::time::Instant::now();
        let result = run(target, stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let msg = result.err().unwrap_or_else(|| "Disconnected".into());
        set_status(platform, "error", &format!("{msg} — retrying in {delay}s"));
        if started.elapsed() > Duration::from_secs(60) {
            delay = 2;
        }
        for _ in 0..delay * 10 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        delay = (delay * 2).min(60);
    }
    set_status(platform, "off", "");
}

pub(crate) fn set_status(platform: &'static str, state: &'static str, detail: &str) {
    let Some(hub) = HUB.get() else { return };
    let st = Status { state, detail: detail.to_string() };
    hub.status.lock().unwrap().insert(platform, st.clone());
    let _ = hub.app.emit("chat:status", json!({ "platform": platform, "state": state, "detail": detail }));
}

pub(crate) fn publish(msg: ChatMessage) {
    let Some(hub) = HUB.get() else { return };
    {
        let mut h = hub.history.lock().unwrap();
        h.push_back(msg.clone());
        while h.len() > HISTORY {
            h.pop_front();
        }
    }
    let text = serde_json::to_string(&msg).unwrap_or_default();
    hub.overlay.lock().unwrap().retain(|tx| tx.send(text.clone()).is_ok());
    let _ = hub.app.emit("chat:message", msg);
}

pub(crate) fn update_context(platform: &'static str, f: impl FnOnce(&mut ChannelContext)) {
    if let Some(hub) = HUB.get() {
        f(hub.context.lock().unwrap().entry(platform).or_default());
    }
}

pub fn context(platform: &str) -> ChannelContext {
    HUB.get()
        .and_then(|h| h.context.lock().unwrap().iter().find(|(k, _)| **k == platform).map(|(_, v)| v.clone()))
        .unwrap_or_default()
}

pub fn history() -> Vec<ChatMessage> {
    HUB.get().map(|h| h.history.lock().unwrap().iter().cloned().collect()).unwrap_or_default()
}

pub fn status() -> HashMap<&'static str, Status> {
    HUB.get().map(|h| h.status.lock().unwrap().clone()).unwrap_or_default()
}

/// Registers an overlay subscriber; returns recent history to replay.
pub fn subscribe(tx: Sender<String>) -> Vec<String> {
    let Some(hub) = HUB.get() else { return Vec::new() };
    hub.overlay.lock().unwrap().push(tx);
    hub.history.lock().unwrap().iter().rev().take(50).rev().filter_map(|m| serde_json::to_string(m).ok()).collect()
}

// ---------------------------------------------------------------------------
// Shared helpers for connectors

pub(crate) type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// Opens a (TLS) WebSocket with a short read timeout so loops can poll `stop`.
pub(crate) fn ws_connect(url: &str) -> Result<Ws, String> {
    let uri: tungstenite::http::Uri = url.parse().map_err(|e| format!("{e}"))?;
    let host = uri.host().ok_or("bad url")?.to_string();
    let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("wss") { 443 } else { 80 });
    let addr = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("cannot resolve host")?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    let mut req = tungstenite::client::IntoClientRequest::into_client_request(url).map_err(|e| e.to_string())?;
    req.headers_mut().insert("User-Agent", USER_AGENT.parse().unwrap());
    let (ws, _) = tungstenite::client_tls(req, tcp).map_err(|e| e.to_string())?;
    Ok(ws)
}

pub(crate) fn is_timeout(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

pub(crate) fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

