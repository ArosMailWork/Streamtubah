//! Authenticated chat actions: send, timeout, ban, unban, delete, mod, VIP.
//! Platform limits: Kick has no moderator/VIP API; YouTube cannot unban without
//! the ban id and has no VIP concept.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{context, raw_http};
use crate::auth::{self, Account, urlencode};
use crate::settings::PlatformApps;

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Send { text: String },
    Timeout { user_id: String, seconds: u64 },
    Ban { user_id: String },
    Unban { user_id: String },
    Delete { message_id: String },
    Mod { user_id: String },
    Vip { user_id: String },
}

pub fn perform(platform: &str, action: &Action, apps: &PlatformApps) -> Result<(), String> {
    let acc = auth::account(platform, apps)?;
    match platform {
        "twitch" => twitch(&acc, action, apps),
        "youtube" => youtube(&acc, action),
        "kick" => kick(&acc, action),
        _ => Err(format!("Unknown platform '{platform}'")),
    }
}

/// Sends to every platform that is both signed in and connected; returns per-platform errors.
pub fn send_all(text: &str, apps: &PlatformApps) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    for a in auth::list() {
        let connected = super::status().get(a.platform.as_str()).is_some_and(|s| s.state == "connected");
        if !connected {
            continue;
        }
        if let Err(e) = perform(&a.platform, &Action::Send { text: text.to_string() }, apps) {
            errors.push((a.platform, e));
        }
    }
    errors
}

fn unsupported(platform: &str, what: &str) -> Result<(), String> {
    Err(format!("{platform} does not support {what} through its API"))
}

// ---------------------------------------------------------------------------
// Twitch (Helix)

fn twitch(acc: &Account, action: &Action, apps: &PlatformApps) -> Result<(), String> {
    let room = context("twitch").broadcaster_id.ok_or("Twitch chat is not connected")?;
    let me = &acc.user_id;
    let (method, path, body) = match action {
        Action::Send { text } => {
            ("POST", "/helix/chat/messages".to_string(), json!({ "broadcaster_id": room, "sender_id": me, "message": text }))
        }
        Action::Timeout { user_id, seconds } => (
            "POST",
            format!("/helix/moderation/bans?broadcaster_id={room}&moderator_id={me}"),
            json!({ "data": { "user_id": user_id, "duration": seconds.clamp(&1, &1_209_600) } }),
        ),
        Action::Ban { user_id } => (
            "POST",
            format!("/helix/moderation/bans?broadcaster_id={room}&moderator_id={me}"),
            json!({ "data": { "user_id": user_id } }),
        ),
        Action::Unban { user_id } => {
            ("DELETE", format!("/helix/moderation/bans?broadcaster_id={room}&moderator_id={me}&user_id={user_id}"), Value::Null)
        }
        Action::Delete { message_id } => (
            "DELETE",
            format!("/helix/moderation/chat?broadcaster_id={room}&moderator_id={me}&message_id={message_id}"),
            Value::Null,
        ),
        Action::Mod { user_id } => {
            ("POST", format!("/helix/moderation/moderators?broadcaster_id={room}&user_id={user_id}"), Value::Null)
        }
        Action::Vip { user_id } => ("POST", format!("/helix/channels/vips?broadcaster_id={room}&user_id={user_id}"), Value::Null),
    };
    let url = format!("https://api.twitch.tv{path}");
    let auth = format!("Bearer {}", acc.access_token);
    let req = |m: &str| {
        ureq::http::Request::builder()
            .method(m)
            .uri(&url)
            .header("Client-Id", &apps.twitch_client_id)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
    };
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let resp = if body.is_null() {
        agent.run(req(method).body(()).map_err(|e| e.to_string())?)
    } else {
        agent.run(req(method).body(body.to_string()).map_err(|e| e.to_string())?)
    }
    .map_err(|e| e.to_string())?;
    check(resp)
}

fn check(resp: ureq::http::Response<ureq::Body>) -> Result<(), String> {
    let status = resp.status().as_u16();
    if status < 300 {
        return Ok(());
    }
    let body: Value = resp.into_body().read_json().unwrap_or(Value::Null);
    let msg = body["message"].as_str().or(body["error"]["message"].as_str()).unwrap_or("request failed");
    Err(format!("{status}: {msg}"))
}

// ---------------------------------------------------------------------------
// YouTube (Data API v3)

fn youtube(acc: &Account, action: &Action) -> Result<(), String> {
    let auth = format!("Bearer {}", acc.access_token);
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let api = "https://www.googleapis.com/youtube/v3";
    let live_chat_id = || -> Result<String, String> {
        let video = context("youtube").video_id.ok_or("YouTube chat is not connected")?;
        let v: Value = agent
            .get(&format!("{api}/videos?part=liveStreamingDetails&id={video}"))
            .header("Authorization", &auth)
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_json()
            .map_err(|e| e.to_string())?;
        v["items"][0]["liveStreamingDetails"]["activeLiveChatId"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "This stream has no active live chat".into())
    };
    let post = |path: &str, body: Value| {
        check(
            agent
                .post(&format!("{api}/{path}"))
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .send(body.to_string())
                .map_err(|e| e.to_string())?,
        )
    };
    match action {
        Action::Send { text } => post(
            "liveChat/messages?part=snippet",
            json!({ "snippet": { "liveChatId": live_chat_id()?, "type": "textMessageEvent", "textMessageDetails": { "messageText": text } } }),
        ),
        Action::Timeout { user_id, seconds } => post(
            "liveChat/bans?part=snippet",
            json!({ "snippet": { "liveChatId": live_chat_id()?, "type": "temporary", "banDurationSeconds": seconds,
                "bannedUserDetails": { "channelId": user_id } } }),
        ),
        Action::Ban { user_id } => post(
            "liveChat/bans?part=snippet",
            json!({ "snippet": { "liveChatId": live_chat_id()?, "type": "permanent", "bannedUserDetails": { "channelId": user_id } } }),
        ),
        Action::Mod { user_id } => post(
            "liveChat/moderators?part=snippet",
            json!({ "snippet": { "liveChatId": live_chat_id()?, "moderatorDetails": { "channelId": user_id } } }),
        ),
        Action::Delete { message_id } => check(
            agent
                .delete(&format!("{api}/liveChat/messages?id={}", urlencode(message_id)))
                .header("Authorization", &auth)
                .call()
                .map_err(|e| e.to_string())?,
        ),
        Action::Unban { .. } => unsupported("YouTube", "unban from chat (use YouTube Studio)"),
        Action::Vip { .. } => unsupported("YouTube", "VIP"),
    }
}

// ---------------------------------------------------------------------------
// Kick (public API v1)

fn kick(acc: &Account, action: &Action) -> Result<(), String> {
    let broadcaster: u64 = context("kick")
        .broadcaster_id
        .and_then(|b| b.parse().ok())
        .ok_or("Kick chat is not connected (enter the channel name, not the chatroom id)")?;
    let auth = format!("Bearer {}", acc.access_token);
    let h = [("Authorization", auth.as_str())];
    let uid = |s: &str| s.parse::<u64>().map_err(|_| "invalid Kick user id".to_string());
    let call = |method: &str, path: &str, body: Option<Value>| {
        raw_http::json("api.kick.com", method, path, &h, body.as_ref()).map(|_| ())
    };
    match action {
        Action::Send { text } => {
            call("POST", "/public/v1/chat", Some(json!({ "content": text, "type": "user", "broadcaster_user_id": broadcaster })))
        }
        Action::Timeout { user_id, seconds } => call(
            "POST",
            "/public/v1/moderation/bans",
            Some(json!({ "broadcaster_user_id": broadcaster, "user_id": uid(user_id)?,
                "duration": (seconds.div_ceil(60)).clamp(1, 10080) })),
        ),
        Action::Ban { user_id } => call(
            "POST",
            "/public/v1/moderation/bans",
            Some(json!({ "broadcaster_user_id": broadcaster, "user_id": uid(user_id)? })),
        ),
        Action::Unban { user_id } => call(
            "DELETE",
            "/public/v1/moderation/bans",
            Some(json!({ "broadcaster_user_id": broadcaster, "user_id": uid(user_id)? })),
        ),
        Action::Delete { message_id } => call("DELETE", &format!("/public/v1/chat/{message_id}"), None),
        Action::Mod { .. } => unsupported("Kick", "adding moderators"),
        Action::Vip { .. } => unsupported("Kick", "VIP"),
    }
}
