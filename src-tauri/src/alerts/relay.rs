//! StreamElements and Streamlabs event sockets. Both relay every channel event (tips, follows,
//! subs, bits, raids, Super Chats, merch…); events that are also read directly from Twitch /
//! Kick / YouTube chat are dropped by the cross-source check in `alerts::emit`.
//! - StreamElements: Astro WebSocket `wss://astro.streamelements.com`, topic `channel.activities`,
//!   channel JWT from the Secrets window.
//! - Streamlabs: socket.io (Engine.IO v3) `sockets.streamlabs.com` with the socket API token.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::Message;

use super::AlertEvent;
use crate::chat::{is_timeout, ws_connect};

pub const SE_TOKEN: &str = "donate.streamelements_token";
pub const SL_TOKEN: &str = "donate.streamlabs_token";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Service {
    StreamElements,
    Streamlabs,
}

impl Service {
    fn token_key(self) -> &'static str {
        match self {
            Service::StreamElements => SE_TOKEN,
            Service::Streamlabs => SL_TOKEN,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Service::StreamElements => "StreamElements",
            Service::Streamlabs => "Streamlabs",
        }
    }
    fn url(self, token: &str) -> String {
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
        match self {
            Service::StreamElements => env("STREAMTUBAH_SE_WS", "wss://astro.streamelements.com"),
            Service::Streamlabs => format!(
                "{}/socket.io/?token={token}&EIO=3&transport=websocket",
                env("STREAMTUBAH_SL_WS", "wss://sockets.streamlabs.com")
            ),
        }
    }
}

/// One running connection: stop flag + human readable state.
pub struct Conn {
    stop: Arc<AtomicBool>,
    pub state: Arc<Mutex<String>>,
}

impl Conn {
    pub fn state(&self) -> String {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Starts a reconnecting connection; `on_state` is called whenever the state text changes.
pub fn start(service: Service, on_state: fn()) -> Conn {
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new("connecting…".to_string()));
    let (flag, st) = (stop.clone(), state.clone());
    let set = move |text: String| {
        *st.lock().unwrap() = text;
        on_state();
    };
    std::thread::spawn(move || {
        let mut delay = 2;
        while !flag.load(Ordering::SeqCst) {
            let Some(token) = crate::vault::get(service.token_key()).filter(|t| !t.trim().is_empty()) else {
                set(format!("add your {} token in Secrets", service.label()));
                sleep_unless(&flag, 5);
                continue;
            };
            set("connecting…".into());
            let started = Instant::now();
            let result = match service {
                Service::StreamElements => run_se(&service.url(&token), token.trim(), &flag, &set, &super::emit),
                Service::Streamlabs => run_sl(&service.url(token.trim()), &flag, &set, &super::emit),
            };
            if flag.load(Ordering::SeqCst) {
                break;
            }
            if started.elapsed() > Duration::from_secs(60) {
                delay = 2;
            }
            set(format!("error: {} (retrying in {delay} s)", result.err().unwrap_or_else(|| "disconnected".into())));
            sleep_unless(&flag, delay);
            delay = (delay * 2).min(60);
        }
    });
    Conn { stop, state }
}

fn sleep_unless(stop: &AtomicBool, secs: u64) {
    for _ in 0..secs * 4 {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

// ---------------------------------------------------------------------------
// StreamElements

fn run_se(url: &str, token: &str, stop: &AtomicBool, set: &dyn Fn(String), sink: &dyn Fn(AlertEvent)) -> Result<(), String> {
    let mut ws = ws_connect(url)?;
    let sub = json!({ "type": "subscribe", "nonce": "streamtubah", "data": { "topic": "channel.activities", "token": token, "token_type": "jwt" } });
    ws.send(Message::text(sub.to_string())).map_err(|e| e.to_string())?;
    while !stop.load(Ordering::SeqCst) {
        let text = match ws.read() {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(f)) => return Err(format!("closed {}", f.map(|f| f.reason.to_string()).unwrap_or_default())),
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
        match msg["type"].as_str() {
            Some("response") if msg["nonce"].as_str() == Some("streamtubah") => {
                if let Some(err) = msg["error"].as_str().filter(|e| !e.is_empty()) {
                    // The detail text is not always meaningful ("internal server error" for a bad JWT).
                    let detail = msg["data"]["message"].as_str().unwrap_or("");
                    return Err(format!("StreamElements refused the token ({err}: {detail})"));
                }
                set("connected".into());
            }
            Some("reconnect") => return Err("server restarting".into()),
            Some("message") => {
                if let Some(ev) = parse_se(&msg) {
                    sink(ev);
                }
            }
            _ => {}
        }
    }
    let _ = ws.close(None);
    Ok(())
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().replace(',', "").parse().ok()))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn money(amount: Option<f64>, currency: &str) -> Option<String> {
    amount.map(|a| format!("{} {currency}", if a.fract() == 0.0 { format!("{a}") } else { format!("{a:.2}") }).trim().to_string())
}

/// `channel.activities` message → alert.
pub fn parse_se(msg: &Value) -> Option<AlertEvent> {
    if msg["topic"].as_str() != Some("channel.activities") {
        return None;
    }
    let a = &msg["data"];
    let d = &a["data"];
    let provider = a["provider"].as_str().unwrap_or("streamelements").to_lowercase();
    let name = [&d["displayName"], &d["username"]].into_iter().filter_map(|v| v.as_str()).find(|s| !s.is_empty()).unwrap_or("Someone");
    let amount = num(&d["amount"]);
    let count = amount.map(|a| a.max(0.0) as u64);
    let mut ev = match a["type"].as_str()? {
        "follow" | "follower" => AlertEvent::new("follow", &provider, name),
        "tip" | "charityCampaignDonation" => {
            let mut e = AlertEvent::new("donation", "streamelements", name);
            e.amount = amount;
            e.amount_text = money(amount, d["currency"].as_str().unwrap_or(""));
            e
        }
        "merch" | "purchase" => {
            let mut e = AlertEvent::new("donation", "streamelements", name);
            e.amount = amount;
            e.amount_text = money(amount, d["currency"].as_str().unwrap_or(""));
            if d["message"].as_str().is_none_or(str::is_empty) {
                let items: Vec<String> = d["items"].as_array().into_iter().flatten().filter_map(|i| i["name"].as_str().map(String::from)).collect();
                if !items.is_empty() {
                    e.message = format!("bought {}", items.join(", "));
                }
            }
            e
        }
        "subscriber" if d["gifted"].as_bool() == Some(true) => {
            let gifter = d["sender"].as_str().filter(|s| !s.is_empty()).unwrap_or("Someone");
            let mut e = AlertEvent::new("gift", &provider, gifter);
            e.count = Some(1);
            e
        }
        "subscriber" => {
            let mut e = AlertEvent::new("sub", &provider, name);
            e.count = count;
            e
        }
        "sponsor" | "sponsorship" => {
            let mut e = AlertEvent::new("member", &provider, name);
            e.count = count;
            e
        }
        "communityGiftPurchase" => {
            let mut e = AlertEvent::new("gift", &provider, d["sender"].as_str().filter(|s| !s.is_empty()).unwrap_or(name));
            e.count = count;
            e
        }
        "cheer" => {
            let mut e = AlertEvent::new("bits", &provider, name);
            e.amount = amount;
            e.amount_text = count.map(|n| format!("{n} bits"));
            e
        }
        "raid" | "host" => {
            let mut e = AlertEvent::new("raid", &provider, name);
            e.count = count;
            e
        }
        "superchat" => {
            let mut e = AlertEvent::new("superchat", &provider, name);
            e.amount = amount;
            e.amount_text = money(amount, d["currency"].as_str().unwrap_or(""));
            e
        }
        _ => return None,
    };
    if ev.message.is_empty() {
        ev.message = text(&d["message"]);
    }
    let id = [&a["_id"], &a["activityId"], &msg["id"]].into_iter().filter_map(|v| v.as_str()).find(|s| !s.is_empty());
    if let Some(id) = id {
        ev.id = format!("se-{id}");
    }
    ev.source = "streamelements".into();
    Some(ev)
}

// ---------------------------------------------------------------------------
// Streamlabs (socket.io over Engine.IO v3: the client pings, the server answers "3")

fn run_sl(url: &str, stop: &AtomicBool, set: &dyn Fn(String), sink: &dyn Fn(AlertEvent)) -> Result<(), String> {
    let mut ws = ws_connect(url)?;
    let mut ping_every = Duration::from_secs(25);
    let mut last_ping = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if last_ping.elapsed() >= ping_every {
            ws.send(Message::text("2")).map_err(|e| e.to_string())?;
            last_ping = Instant::now();
        }
        let frame = match ws.read() {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(f)) => return Err(format!("closed {}", f.map(|f| f.reason.to_string()).unwrap_or_default())),
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => return Err(e.to_string()),
        };
        if let Some(open) = frame.strip_prefix('0') {
            if let Some(ms) = serde_json::from_str::<Value>(open).ok().and_then(|v| v["pingInterval"].as_u64()) {
                ping_every = Duration::from_millis(ms.clamp(1000, 60_000));
            }
        } else if frame == "40" {
            set("connected".into());
        } else if frame == "2" {
            // Engine.IO v4 servers ping instead.
            ws.send(Message::text("3")).map_err(|e| e.to_string())?;
        } else if frame.starts_with("41") {
            return Err("Streamlabs closed the connection".into());
        } else if let Some(err) = frame.strip_prefix("44") {
            return Err(format!("Streamlabs refused the socket token {err}"));
        } else if let Some(payload) = frame.strip_prefix("42") {
            let Ok(Value::Array(args)) = serde_json::from_str::<Value>(payload) else { continue };
            if args.first().and_then(Value::as_str) == Some("event") {
                for ev in args.get(1).map(parse_sl).unwrap_or_default() {
                    sink(ev);
                }
            }
        }
    }
    let _ = ws.close(None);
    Ok(())
}

/// Streamlabs `event` payload → alerts (one per entry in `message`).
pub fn parse_sl(v: &Value) -> Vec<AlertEvent> {
    let kind = v["type"].as_str().unwrap_or("");
    let platform = match v["for"].as_str().unwrap_or("streamlabs") {
        "twitch_account" => "twitch",
        "youtube_account" => "youtube",
        "kick_account" => "kick",
        "facebook_account" => "facebook",
        "trovo_account" => "trovo",
        _ => "streamlabs",
    };
    let items = match &v["message"] {
        Value::Array(a) => a.clone(),
        m @ Value::Object(_) => vec![m.clone()],
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for (i, m) in items.iter().enumerate() {
        let name = [&m["name"], &m["from"], &m["display_name"]].into_iter().filter_map(|v| v.as_str()).find(|s| !s.is_empty()).unwrap_or("Someone");
        let amount = num(&m["amount"]);
        let count = |k: &str| num(&m[k]).map(|n| n.max(0.0) as u64);
        let formatted = m["formatted_amount"].as_str().or(m["formattedAmount"].as_str()).map(String::from);
        let mut ev = match (kind, platform) {
            ("donation" | "merch" | "charity" | "streamlabscharitydonation", _) => {
                let mut e = AlertEvent::new("donation", "streamlabs", name);
                e.amount = amount;
                e.amount_text = formatted.or_else(|| money(amount, m["currency"].as_str().unwrap_or("")));
                if kind == "merch" && m["message"].as_str().is_none_or(str::is_empty) {
                    e.message = m["product"].as_str().map(|p| format!("bought {p}")).unwrap_or_default();
                }
                e
            }
            ("follow", _) => AlertEvent::new("follow", platform, name),
            ("subscription" | "resub", "youtube") => {
                let mut e = AlertEvent::new("member", platform, name);
                e.count = count("months");
                e
            }
            ("subscription" | "resub", _) if m["gifter"].as_str().is_some_and(|g| !g.is_empty()) || m["gifter_display_name"].as_str().is_some() => {
                let gifter = m["gifter_display_name"].as_str().or(m["gifter"].as_str()).unwrap_or("Someone");
                let mut e = AlertEvent::new("gift", platform, gifter);
                e.count = Some(1);
                e
            }
            ("subscription" | "resub", _) => {
                let mut e = AlertEvent::new("sub", platform, name);
                e.count = count("months");
                e
            }
            ("subMysteryGift" | "membershipGift" | "giftedmembership", _) => {
                let mut e = AlertEvent::new("gift", platform, name);
                e.count = count("amount").or_else(|| count("giftMembershipsCount"));
                e
            }
            ("bits", _) => {
                let mut e = AlertEvent::new("bits", platform, name);
                e.amount = amount;
                e.amount_text = amount.map(|n| format!("{n} bits"));
                e
            }
            ("host", _) => {
                let mut e = AlertEvent::new("raid", platform, name);
                e.count = count("viewers");
                e
            }
            ("raid", _) => {
                let mut e = AlertEvent::new("raid", platform, name);
                e.count = count("raiders").or_else(|| count("viewers"));
                e
            }
            ("superchat", _) => {
                let mut e = AlertEvent::new("superchat", platform, name);
                // YouTube amounts arrive in micros.
                e.amount = amount.map(|a| if a >= 10_000.0 { a / 1_000_000.0 } else { a });
                e.amount_text = m["displayString"].as_str().map(String::from).or(formatted);
                e.message = text(&m["comment"]);
                e
            }
            _ => continue,
        };
        if ev.message.is_empty() {
            ev.message = text(&m["message"]);
        }
        let id = [&m["_id"], &m["id"], &m["event_id"]].into_iter().find_map(|v| v.as_str().map(String::from).or(v.as_u64().map(|n| n.to_string())));
        if let Some(id) = id.or_else(|| v["event_id"].as_str().map(|e| format!("{e}-{i}"))) {
            ev.id = format!("sl-{id}");
        }
        ev.source = "streamlabs".into();
        out.push(ev);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn se(kind: &str, provider: &str, data: Value) -> Value {
        json!({ "id": "01J", "type": "message", "topic": "channel.activities",
                "data": { "_id": format!("a-{kind}"), "type": kind, "provider": provider, "channel": "c", "data": data } })
    }

    #[test]
    fn streamelements_activities() {
        let tip = parse_se(&se("tip", "twitch", json!({ "username": "jo", "displayName": "Jo", "amount": 5, "currency": "USD", "message": "hi" }))).unwrap();
        assert_eq!((tip.kind.as_str(), tip.platform.as_str(), tip.name.as_str()), ("donation", "streamelements", "Jo"));
        assert_eq!((tip.amount, tip.amount_text.as_deref(), tip.message.as_str()), (Some(5.0), Some("5 USD"), "hi"));
        assert_eq!((tip.id.as_str(), tip.source.as_str()), ("se-a-tip", "streamelements"));

        let f = parse_se(&se("follow", "twitch", json!({ "username": "jtv", "displayName": "jtv" }))).unwrap();
        assert_eq!((f.kind.as_str(), f.platform.as_str()), ("follow", "twitch"));
        let s = parse_se(&se("subscriber", "twitch", json!({ "displayName": "Sam", "amount": 7, "message": "7 months!" }))).unwrap();
        assert_eq!((s.kind.as_str(), s.count), ("sub", Some(7)));
        let g = parse_se(&se("subscriber", "twitch", json!({ "displayName": "Rec", "gifted": true, "sender": "Gifty", "amount": 1 }))).unwrap();
        assert_eq!((g.kind.as_str(), g.name.as_str(), g.count), ("gift", "Gifty", Some(1)));
        let cg = parse_se(&se("communityGiftPurchase", "twitch", json!({ "displayName": "Gifty", "amount": 5 }))).unwrap();
        assert_eq!((cg.kind.as_str(), cg.count), ("gift", Some(5)));
        let b = parse_se(&se("cheer", "twitch", json!({ "displayName": "B", "amount": 100 }))).unwrap();
        assert_eq!(b.amount_text.as_deref(), Some("100 bits"));
        let r = parse_se(&se("raid", "kick", json!({ "displayName": "R", "amount": 42 }))).unwrap();
        assert_eq!((r.kind.as_str(), r.platform.as_str(), r.count), ("raid", "kick", Some(42)));
        let m = parse_se(&se("sponsor", "youtube", json!({ "displayName": "Y", "amount": 2 }))).unwrap();
        assert_eq!(m.kind, "member");
        let sc = parse_se(&se("superchat", "youtube", json!({ "displayName": "Y", "amount": "2.50", "currency": "EUR" }))).unwrap();
        assert_eq!(sc.amount_text.as_deref(), Some("2.50 EUR"));
        let merch = parse_se(&se("merch", "twitch", json!({ "displayName": "M", "amount": 20, "items": [{ "name": "Hoodie" }] }))).unwrap();
        assert_eq!(merch.message, "bought Hoodie");
        assert!(parse_se(&se("hypetrainStart", "twitch", json!({}))).is_none());
        assert!(parse_se(&json!({ "type": "message", "topic": "other" })).is_none());
    }

    /// Local WebSocket server that runs `script` on the first connection; returns its ws:// URL.
    fn mock_server(script: impl FnOnce(&mut tungstenite::WebSocket<std::net::TcpStream>) + Send + 'static) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(tcp).unwrap();
            script(&mut ws);
            std::thread::sleep(Duration::from_millis(300));
        });
        format!("ws://127.0.0.1:{port}")
    }

    fn read_text(ws: &mut tungstenite::WebSocket<std::net::TcpStream>) -> String {
        loop {
            if let Message::Text(t) = ws.read().unwrap() {
                return t.to_string();
            }
        }
    }

    fn collect(run: impl FnOnce(&AtomicBool, &dyn Fn(String), &dyn Fn(AlertEvent)) -> Result<(), String>) -> (Vec<String>, Vec<AlertEvent>, String) {
        let (states, events) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()));
        let stop = AtomicBool::new(false);
        let err = run(&stop, &|s| states.lock().unwrap().push(s), &|e| events.lock().unwrap().push(e)).err().unwrap_or_default();
        (states.into_inner().unwrap(), events.into_inner().unwrap(), err)
    }

    #[test]
    fn streamelements_socket() {
        let url = mock_server(|ws| {
            let sub: Value = serde_json::from_str(&read_text(ws)).unwrap();
            assert_eq!((sub["type"].as_str(), sub["data"]["topic"].as_str(), sub["data"]["token"].as_str()), (Some("subscribe"), Some("channel.activities"), Some("jwt-1")));
            ws.send(Message::text(json!({ "type": "response", "nonce": "streamtubah", "data": {} }).to_string())).unwrap();
            ws.send(Message::text(se("tip", "twitch", json!({ "displayName": "Jo", "amount": 3, "currency": "USD" })).to_string())).unwrap();
            ws.close(None).unwrap();
        });
        let (states, events, _) = collect(|stop, set, sink| run_se(&url, "jwt-1", stop, set, sink));
        assert_eq!(states, vec!["connected"]);
        assert_eq!(events.iter().map(|e| (e.kind.as_str(), e.name.as_str())).collect::<Vec<_>>(), vec![("donation", "Jo")]);

        let url = mock_server(|ws| {
            read_text(ws);
            ws.send(Message::text(json!({ "type": "response", "nonce": "streamtubah", "error": "err_unauthorized", "data": { "message": "invalid token" } }).to_string())).unwrap();
        });
        let (_, _, err) = collect(|stop, set, sink| run_se(&url, "bad", stop, set, sink));
        assert!(err.contains("invalid token"), "{err}");
    }

    #[test]
    fn streamlabs_socket() {
        let url = mock_server(|ws| {
            ws.send(Message::text(r#"0{"sid":"x","pingInterval":1000,"pingTimeout":5000}"#)).unwrap();
            ws.send(Message::text("40")).unwrap();
            // The client pings on its own (Engine.IO v3).
            assert_eq!(read_text(ws), "2");
            ws.send(Message::text("3")).unwrap();
            let ev = json!(["event", { "type": "donation", "for": "streamlabs", "message": [{ "_id": "d9", "name": "Ann", "amount": 2, "currency": "EUR" }] }]);
            ws.send(Message::text(format!("42{ev}"))).unwrap();
            ws.send(Message::text("41")).unwrap();
        });
        let (states, events, err) = collect(|stop, set, sink| run_sl(&format!("{url}/socket.io/?token=t&EIO=3&transport=websocket"), stop, set, sink));
        assert_eq!(states, vec!["connected"]);
        assert_eq!(events.iter().map(|e| (e.id.as_str(), e.amount_text.as_deref())).collect::<Vec<_>>(), vec![("sl-d9", Some("2 EUR"))]);
        assert!(err.contains("closed"), "{err}");
    }

    #[test]
    fn streamlabs_events() {
        let d = parse_sl(&json!({ "type": "donation", "for": "streamlabs", "event_id": "e1",
            "message": [{ "_id": "d1", "name": "Ann", "amount": "10.5", "formatted_amount": "$10.50", "currency": "USD", "message": "gl" }] }));
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].kind.as_str(), d[0].platform.as_str(), d[0].amount), ("donation", "streamlabs", Some(10.5)));
        assert_eq!((d[0].amount_text.as_deref(), d[0].message.as_str(), d[0].id.as_str()), (Some("$10.50"), "gl", "sl-d1"));

        let f = parse_sl(&json!({ "type": "follow", "for": "twitch_account", "message": [{ "name": "A", "id": "f1" }, { "name": "B", "id": "f2" }] }));
        assert_eq!(f.iter().map(|e| (e.kind.as_str(), e.platform.as_str(), e.name.as_str())).collect::<Vec<_>>(),
            vec![("follow", "twitch", "A"), ("follow", "twitch", "B")]);
        let s = parse_sl(&json!({ "type": "resub", "for": "twitch_account", "message": [{ "name": "S", "months": 3 }] }));
        assert_eq!((s[0].kind.as_str(), s[0].count), ("sub", Some(3)));
        let g = parse_sl(&json!({ "type": "subscription", "for": "twitch_account", "message": [{ "name": "R", "gifter": "Gifty" }] }));
        assert_eq!((g[0].kind.as_str(), g[0].name.as_str()), ("gift", "Gifty"));
        let m = parse_sl(&json!({ "type": "subscription", "for": "youtube_account", "message": [{ "name": "Y", "months": 1 }] }));
        assert_eq!(m[0].kind, "member");
        let sc = parse_sl(&json!({ "type": "superchat", "for": "youtube_account", "message": [{ "name": "Y", "amount": 5000000, "displayString": "$5.00", "comment": "yo" }] }));
        assert_eq!((sc[0].amount, sc[0].amount_text.as_deref(), sc[0].message.as_str()), (Some(5.0), Some("$5.00"), "yo"));
        let r = parse_sl(&json!({ "type": "raid", "for": "twitch_account", "message": [{ "name": "R", "raiders": 12 }] }));
        assert_eq!((r[0].kind.as_str(), r[0].count), ("raid", Some(12)));
        assert!(parse_sl(&json!({ "type": "streamlabels", "message": {} })).is_empty());
    }
}
