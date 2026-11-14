//! Twitch chat via the public IRC-over-WebSocket gateway, anonymous (read-only).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tungstenite::Message;

use super::{ChatMessage, is_timeout, publish, set_status, ws_connect};

pub fn run(target: &str, stop: &AtomicBool) -> Result<(), String> {
    let channel = normalize(target);
    let mut ws = ws_connect("wss://irc-ws.chat.twitch.tv:443")?;
    let nick = format!("justinfan{}", 10000 + super::rand_suffix() % 80000);
    for line in [
        "CAP REQ :twitch.tv/tags twitch.tv/commands".to_string(),
        "PASS SCHMOOPIIE".to_string(),
        format!("NICK {nick}"),
        format!("JOIN #{channel}"),
    ] {
        ws.send(Message::text(line)).map_err(|e| e.to_string())?;
    }
    while !stop.load(Ordering::Relaxed) {
        let text = match ws.read() {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) => return Err("Closed by server".into()),
            Ok(_) => continue,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => return Err(e.to_string()),
        };
        for line in text.split("\r\n").filter(|l| !l.is_empty()) {
            if line.starts_with("PING") {
                ws.send(Message::text(line.replacen("PING", "PONG", 1))).map_err(|e| e.to_string())?;
                continue;
            }
            if line.contains(" ROOMSTATE ") || line.contains(" 366 ") {
                set_status("twitch", "connected", &format!("#{channel}"));
            }
            if line.contains(" ROOMSTATE ") {
                if let Some(room) = tag(line, "room-id") {
                    super::update_context("twitch", |c| c.broadcaster_id = Some(room));
                }
            }
            if line.contains(" RECONNECT") {
                return Err("Server asked to reconnect".into());
            }
            if let Some(msg) = parse(line) {
                publish(msg);
            }
        }
    }
    let _ = ws.close(None);
    Ok(())
}

/// Accepts "name", "#name" or a twitch.tv URL.
fn normalize(target: &str) -> String {
    let t = target.trim().trim_start_matches('#');
    let t = t.rsplit("twitch.tv/").next().unwrap_or(t);
    t.split(['/', '?']).next().unwrap_or(t).to_lowercase()
}

fn tag(line: &str, key: &str) -> Option<String> {
    let tags = line.strip_prefix('@')?.split_once(' ')?.0;
    tags.split(';').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

fn unescape(v: &str) -> String {
    v.replace("\\s", " ").replace("\\:", ";").replace("\\\\", "\\").replace("\\r", "").replace("\\n", "")
}

/// Parses PRIVMSG (chat, cheers) and USERNOTICE (subs, gifts, raids).
fn parse(line: &str) -> Option<ChatMessage> {
    let (tags, rest) = match line.strip_prefix('@') {
        Some(r) => r.split_once(' ')?,
        None => ("", line),
    };
    let tags: HashMap<&str, String> =
        tags.split(';').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k, unescape(v))).collect();
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let (prefix, rest) = rest.split_once(' ')?;
    let (command, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let text = rest.split_once(" :").map(|(_, t)| t.to_string()).unwrap_or_default();
    let login = prefix.split('!').next().unwrap_or("").to_string();
    let author = tags.get("display-name").filter(|s| !s.is_empty()).cloned().unwrap_or(login);

    let mut msg = match command {
        "PRIVMSG" => {
            let mut m = ChatMessage::new("twitch", author, text.trim_start_matches("\u{1}ACTION ").trim_end_matches('\u{1}'));
            if let Some(bits) = tags.get("bits") {
                m.kind = "paid";
                m.amount = Some(format!("{bits} bits"));
            }
            m
        }
        "USERNOTICE" => {
            let system = tags.get("system-msg").cloned().unwrap_or_default();
            let kind = match tags.get("msg-id").map(String::as_str) {
                Some("sub" | "resub" | "subgift" | "submysterygift" | "giftpaidupgrade" | "primepaidupgrade") => "membership",
                _ => "system",
            };
            let mut m = ChatMessage::new("twitch", author, if text.is_empty() { system.clone() } else { format!("{system} — {text}") });
            m.kind = kind;
            m
        }
        _ => return None,
    };
    if let Some(id) = tags.get("id") {
        msg.id = format!("twitch-{id}");
        msg.msg_id = Some(id.clone());
    }
    msg.author_id = tags.get("user-id").filter(|s| !s.is_empty()).cloned();
    msg.color = tags.get("color").filter(|c| !c.is_empty()).cloned();
    msg.badges = tags
        .get("badges")
        .map(|b| b.split(',').filter_map(|x| x.split('/').next()).filter(|x| !x.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_privmsg_with_tags() {
        let line = "@badge-info=;badges=moderator/1,subscriber/12;color=#FF0000;display-name=Alice;id=abc;bits=100 :alice!alice@alice.tmi.twitch.tv PRIVMSG #chan :Cheer100 hello there";
        let m = parse(line).unwrap();
        assert_eq!(m.author, "Alice");
        assert_eq!(m.text, "Cheer100 hello there");
        assert_eq!(m.kind, "paid");
        assert_eq!(m.amount.as_deref(), Some("100 bits"));
        assert_eq!(m.badges, vec!["moderator", "subscriber"]);
        assert_eq!(m.color.as_deref(), Some("#FF0000"));
    }

    #[test]
    fn parses_sub_notice() {
        let line = "@display-name=Bob;msg-id=resub;system-msg=Bob\\ssubscribed\\sfor\\s3\\smonths! :tmi.twitch.tv USERNOTICE #chan :yay";
        let m = parse(line).unwrap();
        assert_eq!(m.kind, "membership");
        assert_eq!(m.text, "Bob subscribed for 3 months! — yay");
    }

    #[test]
    fn normalizes_channel() {
        assert_eq!(normalize("https://www.twitch.tv/SomeOne?x=1"), "someone");
        assert_eq!(normalize("#Foo"), "foo");
    }
}
