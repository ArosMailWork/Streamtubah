//! Donations, all handled on the streamer's PC (no relay server):
//! - Ko-fi: webhooks to a small local listener (`127.0.0.1:<kofi_port>/kofi`, separate from the
//!   control API so the API is never exposed), published through a temporary Cloudflare quick
//!   tunnel (`cloudflared`, downloaded on first use). Requests must carry the Ko-fi verification
//!   token stored in the Secrets window.
//! - Patreon: polls the creator's members every minute with the creator access token (Secrets);
//!   the first poll only remembers existing patrons, later new active patrons raise alerts.
//! - StreamElements / Streamlabs: their event sockets (see `relay.rs`).

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use super::AlertEvent;
use super::relay::{self, Conn, Service};

pub const KOFI_TOKEN: &str = "donate.kofi_token";
pub const PATREON_TOKEN: &str = "donate.patreon_token";
const CLOUDFLARED_URL: &str = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";
const POLL: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DonateSettings {
    pub kofi_enabled: bool,
    /// Local port of the Ko-fi webhook listener (the tunnel points here).
    pub kofi_port: u16,
    pub patreon_enabled: bool,
    pub streamelements_enabled: bool,
    pub streamlabs_enabled: bool,
}

impl Default for DonateSettings {
    fn default() -> Self {
        Self { kofi_enabled: false, kofi_port: 4460, patreon_enabled: false, streamelements_enabled: false, streamlabs_enabled: false }
    }
}

#[derive(Default)]
struct State {
    cfg: DonateSettings,
    kofi_stop: Option<Arc<AtomicBool>>,
    tunnel: Option<Child>,
    public_url: Option<String>,
    kofi_state: String,
    patreon_stop: Option<Arc<AtomicBool>>,
    patreon_state: String,
    se: Option<Conn>,
    sl: Option<Conn>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static APP: OnceLock<(AppHandle, PathBuf)> = OnceLock::new();

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

pub fn status() -> Value {
    with(|s| {
        json!({
            "kofi": {
                "enabled": s.cfg.kofi_enabled,
                "state": s.kofi_state,
                "public_url": s.public_url,
                "webhook_url": s.public_url.as_ref().map(|u| format!("{u}/kofi")),
            },
            "patreon": { "enabled": s.cfg.patreon_enabled, "state": s.patreon_state },
            "streamelements": { "enabled": s.cfg.streamelements_enabled, "state": s.se.as_ref().map_or("off".into(), Conn::state) },
            "streamlabs": { "enabled": s.cfg.streamlabs_enabled, "state": s.sl.as_ref().map_or("off".into(), Conn::state) },
        })
    })
}

fn notify() {
    if let Some((app, _)) = APP.get() {
        let _ = app.emit("donate:status", status());
    }
}

fn set_kofi_state(text: impl Into<String>) {
    with(|s| s.kofi_state = text.into());
    notify();
}

fn set_patreon_state(text: impl Into<String>) {
    with(|s| s.patreon_state = text.into());
    notify();
}

pub fn init(app: &AppHandle, data_dir: PathBuf, cfg: &DonateSettings) {
    let _ = APP.set((app.clone(), data_dir));
    configure(cfg);
}

/// (Re)starts or stops Ko-fi and Patreon according to the settings.
pub fn configure(cfg: &DonateSettings) {
    let prev = with(|s| std::mem::replace(&mut s.cfg, cfg.clone()));
    if prev.kofi_enabled != cfg.kofi_enabled || prev.kofi_port != cfg.kofi_port || with(|s| s.kofi_stop.is_none()) == cfg.kofi_enabled {
        stop_kofi();
        if cfg.kofi_enabled {
            start_kofi(cfg.kofi_port);
        } else {
            set_kofi_state("off");
        }
    }
    if prev.patreon_enabled != cfg.patreon_enabled || with(|s| s.patreon_stop.is_none()) == cfg.patreon_enabled {
        if let Some(stop) = with(|s| s.patreon_stop.take()) {
            stop.store(true, Ordering::SeqCst);
        }
        if cfg.patreon_enabled {
            start_patreon();
        } else {
            set_patreon_state("off");
        }
    }
    configure_relays(cfg);
}

/// Opens or closes the StreamElements / Streamlabs sockets. A changed token is picked up on the
/// next reconnect; `restart_relay` forces it.
fn configure_relays(cfg: &DonateSettings) {
    let old = with(|s| {
        let se = if cfg.streamelements_enabled != s.se.is_some() { Some(s.se.take()) } else { None };
        let sl = if cfg.streamlabs_enabled != s.sl.is_some() { Some(s.sl.take()) } else { None };
        (se, sl)
    });
    drop(old);
    if cfg.streamelements_enabled && with(|s| s.se.is_none()) {
        let c = relay::start(Service::StreamElements, notify);
        with(|s| s.se = Some(c));
    }
    if cfg.streamlabs_enabled && with(|s| s.sl.is_none()) {
        let c = relay::start(Service::Streamlabs, notify);
        with(|s| s.sl = Some(c));
    }
    notify();
}

/// Reconnects a relay after its token changed in the Secrets window.
pub fn restart_relay(secret_key: &str) {
    let cfg = with(|s| {
        match secret_key {
            relay::SE_TOKEN => drop(s.se.take()),
            relay::SL_TOKEN => drop(s.sl.take()),
            _ => {}
        }
        s.cfg.clone()
    });
    configure_relays(&cfg);
}

/// Stops the tunnel process (called at app exit).
pub fn shutdown() {
    with(|s| (s.se.take(), s.sl.take()));
    stop_kofi();
    if let Some(stop) = with(|s| s.patreon_stop.take()) {
        stop.store(true, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Ko-fi

fn stop_kofi() {
    let (stop, tunnel) = with(|s| {
        s.public_url = None;
        (s.kofi_stop.take(), s.tunnel.take())
    });
    if let Some(stop) = stop {
        stop.store(true, Ordering::SeqCst);
    }
    if let Some(mut child) = tunnel {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn start_kofi(port: u16) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => return set_kofi_state(format!("error: port {port} is busy ({e})")),
    };
    let _ = listener.set_nonblocking(true);
    let stop = Arc::new(AtomicBool::new(false));
    with(|s| s.kofi_stop = Some(stop.clone()));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    std::thread::spawn(move || handle_kofi(stream));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    set_kofi_state("starting tunnel…");
    std::thread::spawn(move || {
        if let Err(e) = run_tunnel(port, &stop) {
            if !stop.load(Ordering::SeqCst) {
                set_kofi_state(format!("error: {e}"));
            }
        }
    });
}

fn cloudflared_path() -> Result<PathBuf, String> {
    let (_, data) = APP.get().ok_or("not initialised")?;
    let dir = data.join("tools");
    let exe = dir.join("cloudflared.exe");
    if exe.exists() {
        return Ok(exe);
    }
    set_kofi_state("downloading cloudflared (one time, ~60 MB)…");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(600))).build().into();
    let resp = agent.get(CLOUDFLARED_URL).call().map_err(|e| format!("cloudflared download failed: {e}"))?;
    let mut data = Vec::new();
    resp.into_body().into_reader().take(200 * 1024 * 1024).read_to_end(&mut data).map_err(|e| e.to_string())?;
    if data.len() < 1_000_000 || &data[..2] != b"MZ" {
        return Err("cloudflared download is not a Windows program".into());
    }
    let tmp = exe.with_extension("download");
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &exe).map_err(|e| e.to_string())?;
    Ok(exe)
}

/// Runs `cloudflared tunnel --url` and publishes the trycloudflare.com address it prints.
fn run_tunnel(port: u16, stop: &AtomicBool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = cloudflared_path()?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut child = Command::new(exe)
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{port}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("cannot start cloudflared: {e}"))?;
    let stderr = child.stderr.take().ok_or("no cloudflared output")?;
    with(|s| s.tunnel = Some(child));
    if stop.load(Ordering::SeqCst) {
        stop_kofi();
        return Ok(());
    }
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        if let Some(url) = find_tunnel_url(&line) {
            with(|s| s.public_url = Some(url));
            set_kofi_state("ready");
        }
    }
    // stderr closed: cloudflared exited.
    Err("the tunnel stopped".into())
}

fn find_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-')).collect();
    url.trim_end_matches('/').ends_with(".trycloudflare.com").then(|| url.trim_end_matches('/').to_string())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Ko-fi webhook body (`data=<url-encoded JSON>`) → alert, if the verification token matches.
pub fn parse_kofi(body: &str, token: &str) -> Result<AlertEvent, &'static str> {
    let data = body.split('&').find_map(|kv| kv.strip_prefix("data=")).ok_or("no data field")?;
    let d: Value = serde_json::from_str(&percent_decode(data)).map_err(|_| "bad JSON")?;
    if token.is_empty() || d["verification_token"].as_str() != Some(token) {
        return Err("bad verification token");
    }
    let kind = if d["type"].as_str() == Some("Subscription") { "patron" } else { "donation" };
    let mut ev = AlertEvent::new(kind, "kofi", d["from_name"].as_str().filter(|s| !s.is_empty()).unwrap_or("Someone"));
    if let Some(id) = d["kofi_transaction_id"].as_str() {
        ev.id = format!("kofi-{id}");
    }
    let amount = d["amount"].as_str().unwrap_or("");
    ev.amount = amount.parse().ok();
    ev.amount_text = Some(format!("{amount} {}", d["currency"].as_str().unwrap_or("")).trim().to_string());
    if d["is_public"].as_bool().unwrap_or(true) {
        ev.message = d["message"].as_str().unwrap_or("").to_string();
    }
    Ok(ev)
}

fn handle_kofi(mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line);
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let respond = |stream: &mut TcpStream, status: &str| {
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    };
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("POST") || parts.next().map(|p| p.split('?').next().unwrap_or("")) != Some("/kofi") || length > 65536 {
        return respond(&mut stream, "404 Not Found");
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return respond(&mut stream, "400 Bad Request");
    }
    let token = crate::vault::get(KOFI_TOKEN).unwrap_or_default();
    match parse_kofi(&String::from_utf8_lossy(&body), &token) {
        Ok(ev) => {
            super::emit(ev);
            respond(&mut stream, "200 OK");
        }
        Err(e) => {
            eprintln!("Ko-fi webhook rejected: {e}");
            respond(&mut stream, "403 Forbidden");
        }
    }
}

// ---------------------------------------------------------------------------
// Patreon

fn patreon_base() -> String {
    std::env::var("STREAMTUBAH_PATREON_API").unwrap_or_else(|_| "https://www.patreon.com".into())
}

fn patreon_get(path_or_url: &str, token: &str) -> Result<Value, String> {
    let url = if path_or_url.starts_with("http") { path_or_url.to_string() } else { format!("{}{path_or_url}", patreon_base()) };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let text = agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("Patreon request failed: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Active patrons: (member id, name, monthly amount in cents).
fn active_patrons(token: &str) -> Result<Vec<(String, String, u64)>, String> {
    let campaigns = patreon_get("/api/oauth2/v2/campaigns", token)?;
    let id = campaigns["data"][0]["id"].as_str().ok_or("No Patreon campaign on this account")?.to_string();
    let mut url = format!(
        "/api/oauth2/v2/campaigns/{id}/members?fields%5Bmember%5D=full_name,patron_status,currently_entitled_amount_cents&page%5Bcount%5D=500"
    );
    let mut out = Vec::new();
    for _ in 0..20 {
        let page = patreon_get(&url, token)?;
        for m in page["data"].as_array().into_iter().flatten() {
            let a = &m["attributes"];
            if a["patron_status"].as_str() == Some("active_patron") {
                out.push((
                    m["id"].as_str().unwrap_or("").to_string(),
                    a["full_name"].as_str().unwrap_or("Someone").to_string(),
                    a["currently_entitled_amount_cents"].as_u64().unwrap_or(0),
                ));
            }
        }
        match page["links"]["next"].as_str() {
            Some(next) => url = next.to_string(),
            None => break,
        }
    }
    Ok(out)
}

fn start_patreon() {
    let stop = Arc::new(AtomicBool::new(false));
    with(|s| s.patreon_stop = Some(stop.clone()));
    std::thread::spawn(move || {
        let mut seen: Option<HashSet<String>> = None;
        while !stop.load(Ordering::SeqCst) {
            match crate::vault::get(PATREON_TOKEN) {
                None => set_patreon_state("add your Patreon creator access token in Secrets"),
                Some(token) => match active_patrons(&token) {
                    Ok(list) => {
                        if let Some(known) = &mut seen {
                            for (id, name, cents) in &list {
                                if known.insert(id.clone()) {
                                    let mut ev = AlertEvent::new("patron", "patreon", name.clone());
                                    ev.id = format!("patreon-{id}");
                                    ev.amount = Some(*cents as f64 / 100.0);
                                    ev.amount_text = Some(format!("${:.2}/month", *cents as f64 / 100.0));
                                    super::emit(ev);
                                }
                            }
                        } else {
                            seen = Some(list.iter().map(|(id, _, _)| id.clone()).collect());
                        }
                        set_patreon_state(format!("watching · {} active patrons", list.len()));
                    }
                    Err(e) => set_patreon_state(format!("error: {e}")),
                },
            }
            for _ in 0..POLL.as_secs() {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kofi_webhook_parsing() {
        let json = r#"{"verification_token":"tok-123","message_id":"m","kofi_transaction_id":"abc","type":"Donation","is_public":true,"from_name":"Jo Example","message":"Good luck & have fun!","amount":"3.00","currency":"USD"}"#;
        let body = format!("data={}", json.replace('%', "%25").replace(' ', "+").replace('&', "%26").replace('"', "%22"));
        let ev = parse_kofi(&body, "tok-123").unwrap();
        assert_eq!((ev.kind.as_str(), ev.name.as_str(), ev.amount), ("donation", "Jo Example", Some(3.0)));
        assert_eq!(ev.amount_text.as_deref(), Some("3.00 USD"));
        assert_eq!(ev.message, "Good luck & have fun!");
        assert_eq!(ev.id, "kofi-abc");
        assert!(parse_kofi(&body, "wrong").is_err());
        assert!(parse_kofi(&body, "").is_err());
        let private = body.replace("true", "false");
        assert_eq!(parse_kofi(&private, "tok-123").unwrap().message, "");
    }

    #[test]
    fn tunnel_url_detection() {
        let l = "2026-11-14T10:00:00Z INF |  https://quiet-river-1234.trycloudflare.com                                     |";
        assert_eq!(find_tunnel_url(l).as_deref(), Some("https://quiet-river-1234.trycloudflare.com"));
        assert_eq!(find_tunnel_url("INF Visit https://www.cloudflare.com/website-terms/"), None);
    }
}
