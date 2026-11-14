//! Kick chat via its public Pusher channel `chatrooms.<id>.v2` (read-only, no login).
//! Kick rotates the Pusher app key without notice, so the key is negotiated
//! before every connection, with the last known key as fallback.

use crate::alerts::AlertEvent;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tungstenite::Message;

use super::{ChatMessage, is_timeout, publish, set_status, ws_connect};

const FALLBACK_KEY: &str = "34bf7a0ff419a2a775b9";
const FALLBACK_CLUSTER: &str = "us2";

pub fn run(target: &str, stop: &AtomicBool) -> Result<(), String> {
    let (chatroom, broadcaster) = chatroom_id(target)?;
    super::update_context("kick", |c| c.broadcaster_id = broadcaster.map(|b| b.to_string()));
    let (key, cluster) = negotiate().unwrap_or_else(|| (FALLBACK_KEY.into(), FALLBACK_CLUSTER.into()));
    let url = format!("wss://ws-{cluster}.pusher.com/app/{key}?protocol=7&client=js&version=8.4.0-rc2&flash=false");
    let mut ws = ws_connect(&url)?;
    let channel = format!("chatrooms.{chatroom}.v2");

    while !stop.load(Ordering::Relaxed) {
        let text = match ws.read() {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(f)) => return Err(format!("Closed: {}", f.map(|f| f.reason.to_string()).unwrap_or_default())),
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
        let event = frame["event"].as_str().unwrap_or("");
        // Pusher wraps payloads as a JSON string.
        let data: Value = frame["data"].as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(frame["data"].clone());
        match event {
            "pusher:connection_established" => {
                let sub = json!({ "event": "pusher:subscribe", "data": { "auth": "", "channel": channel } });
                ws.send(Message::text(sub.to_string())).map_err(|e| e.to_string())?;
            }
            "pusher_internal:subscription_succeeded" => set_status("kick", "connected", target),
            "pusher:ping" => {
                ws.send(Message::text(r#"{"event":"pusher:pong","data":{}}"#)).map_err(|e| e.to_string())?;
            }
            "pusher:error" => return Err(format!("Pusher error: {}", data["message"].as_str().unwrap_or("unknown"))),
            e if e.ends_with("ChatMessageEvent") || e.ends_with("ChatMessageSentEvent") => {
                if let Some(m) = chat_message(&data) {
                    publish(m);
                }
            }
            e if e.ends_with("SubscriptionEvent") || e.ends_with("GiftedSubscriptionsEvent") || e.ends_with("StreamHostEvent") => {
                if let Some(m) = event_message(e, &data) {
                    publish(m);
                }
            }
            _ => {}
        }
    }
    let _ = ws.close(None);
    Ok(())
}

fn chat_message(d: &Value) -> Option<ChatMessage> {
    let sender = &d["sender"];
    let mut m = ChatMessage::new("kick", sender["username"].as_str()?, strip_emotes(d["content"].as_str().unwrap_or("")));
    if let Some(id) = d["id"].as_str() {
        m.id = format!("kick-{id}");
        m.msg_id = Some(id.to_string());
    }
    m.author_id = sender["id"].as_u64().map(|v| v.to_string());
    m.color = sender["identity"]["color"].as_str().map(String::from);
    m.badges = sender["identity"]["badges"]
        .as_array()
        .map(|b| b.iter().filter_map(|x| x["type"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(m)
}

/// Kick encodes emotes inline as `[emote:<id>:<name>]`; keep just the name.
fn strip_emotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[emote:") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 7..];
        match tail.find(']') {
            Some(end) => {
                let inner = &tail[..end];
                out.push_str(inner.split_once(':').map_or(inner, |(_, name)| name));
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_emotes() {
        assert_eq!(super::strip_emotes("hi [emote:37226:KEKW] lol [emote:1:x]"), "hi KEKW lol x");
        assert_eq!(super::strip_emotes("broken [emote:1"), "broken [emote:1");
    }
}

/// Channel input: a slug, a kick.com URL, or a numeric chatroom id (manual fallback).
/// Returns (chatroom id, broadcaster user id when known).
fn chatroom_id(target: &str) -> Result<(u64, Option<u64>), String> {
    let t = target.trim();
    if let Ok(id) = t.parse::<u64>() {
        return Ok((id, None));
    }
    let slug = t.rsplit("kick.com/").next().unwrap_or(t);
    let slug = slug.split(['/', '?']).next().unwrap_or(slug).to_lowercase().replace('_', "-");
    let resp = super::raw_http::json("kick.com", "GET", &format!("/api/v2/channels/{slug}"), &[], None)
        .map_err(|e| format!("Kick channel lookup failed ({e}). Enter the numeric chatroom id instead."))?;
    let room = resp["chatroom"]["id"].as_u64().ok_or_else(|| format!("Kick channel '{slug}' not found"))?;
    Ok((room, resp["user_id"].as_u64()))
}

/// Asks Kick which Pusher app key/cluster to use right now.
fn negotiate() -> Option<(String, String)> {
    let body = json!({
        "client": { "id": "streamtubah", "type": "web" },
        "capabilities": { "accepted_providers": [{ "provider": "pusher" }] },
    });
    let resp = super::raw_http::json("web.kick.com", "POST", "/api/v1/realtime/connection", &[], Some(&body)).ok()?;
    let key = find_str(&resp, "app_key")?;
    let cluster = find_str(&resp, "cluster").unwrap_or_else(|| FALLBACK_CLUSTER.into());
    Some((key, cluster))
}

/// The response schema is undocumented; search for the field anywhere.
fn find_str(v: &Value, key: &str) -> Option<String> {
    match v {
        Value::Object(o) => o
            .get(key)
            .and_then(|x| x.as_str().map(String::from))
            .or_else(|| o.values().find_map(|x| find_str(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find_str(x, key)),
        _ => None,
    }
}

/// Chat line + alert for subscription, gift and host (raid) events.
fn event_message(event: &str, d: &Value) -> Option<ChatMessage> {
    let s = |k: &str| d[k].as_str().filter(|v| !v.is_empty());
    let (who, text, mut alert) = if event.ends_with("GiftedSubscriptionsEvent") {
        let who = s("gifter_username").unwrap_or("Someone");
        let n = d["gifted_usernames"].as_array().map_or(1, |a| a.len().max(1)) as u64;
        let mut a = AlertEvent::new("gift", "kick", who);
        a.count = Some(n);
        (who, format!("gifted {n} subscription{}", if n == 1 { "" } else { "s" }), a)
    } else if event.ends_with("StreamHostEvent") {
        let who = s("host_username")?;
        let n = d["number_viewers"].as_u64().unwrap_or(0);
        let mut a = AlertEvent::new("raid", "kick", who);
        a.count = Some(n);
        (who, format!("is hosting with {n} viewers"), a)
    } else {
        let who = s("username").unwrap_or("Someone");
        let mut a = AlertEvent::new("sub", "kick", who);
        a.count = d["months"].as_u64();
        (who, "subscribed".to_string(), a)
    };
    if let Some(id) = d["id"].as_str() {
        alert.id = format!("kick-{id}");
    }
    let mut m = ChatMessage::new("kick", who, text);
    m.kind = "membership";
    m.alert = Some(alert);
    Some(m)
}

#[cfg(test)]
mod alert_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kick_alerts() {
        let g = event_message("App\\Events\\GiftedSubscriptionsEvent", &json!({ "gifter_username": "Gifty", "gifted_usernames": ["a", "b", "c"] })).unwrap();
        assert_eq!(g.alert.map(|a| (a.kind, a.name, a.count)), Some(("gift".into(), "Gifty".into(), Some(3))));
        let h = event_message("App\\Events\\StreamHostEvent", &json!({ "host_username": "Hoster", "number_viewers": 12 })).unwrap();
        assert_eq!(h.alert.map(|a| (a.kind, a.count)), Some(("raid".into(), Some(12))));
        let sub = event_message("App\\Events\\SubscriptionEvent", &json!({ "username": "Subby", "months": 3 })).unwrap();
        assert_eq!(sub.alert.map(|a| (a.kind, a.count)), Some(("sub".into(), Some(3))));
    }
}
