//! Twitch chat via the public IRC-over-WebSocket gateway, anonymous (read-only).

use crate::alerts::AlertEvent;
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
                let mut a = AlertEvent::new("bits", "twitch", m.author.clone());
                a.amount = bits.parse().ok();
                a.amount_text = m.amount.clone();
                // Cheermotes ("Cheer100") are not part of the viewer's message.
                a.message = m.text.split_whitespace().filter(|w| !is_cheermote(w)).collect::<Vec<_>>().join(" ");
                m.alert = Some(a);
            }
            m
        }
        "USERNOTICE" => {
            let system = tags.get("system-msg").cloned().unwrap_or_default();
            let kind = match tags.get("msg-id").map(String::as_str) {
                Some("sub" | "resub" | "subgift" | "submysterygift" | "giftpaidupgrade" | "primepaidupgrade") => "membership",
                _ => "system",
            };
            let mut m = ChatMessage::new("twitch", author.clone(), if text.is_empty() { system.clone() } else { format!("{system} — {text}") });
            m.kind = kind;
            m.alert = usernotice_alert(&tags, &author, &text);
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

/// "Cheer100", "Kappa500"…: letters followed by an amount.
fn is_cheermote(word: &str) -> bool {
    let digits = word.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    digits.len() < word.len() && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// Alert for sub / resub / gift / raid notices. Single gifts that belong to a mass gift
/// ("submysterygift") are skipped: the mass gift already raised one alert for all of them.
fn usernotice_alert(tags: &HashMap<&str, String>, author: &str, text: &str) -> Option<AlertEvent> {
    let num = |k: &str| tags.get(k).and_then(|v| v.parse::<u64>().ok());
    let mut a = match tags.get("msg-id").map(String::as_str)? {
        "sub" | "resub" => {
            let mut a = AlertEvent::new("sub", "twitch", author);
            a.count = num("msg-param-cumulative-months");
            a.message = text.to_string();
            a
        }
        "subgift" if tags.contains_key("msg-param-community-gift-id") => return None,
        "subgift" => {
            let mut a = AlertEvent::new("gift", "twitch", author);
            a.count = Some(1);
            a.message = tags.get("msg-param-recipient-display-name").map(|r| format!("to {r}")).unwrap_or_default();
            a
        }
        "submysterygift" => {
            let mut a = AlertEvent::new("gift", "twitch", author);
            a.count = num("msg-param-mass-gift-count");
            a
        }
        "raid" => {
            let name = tags.get("msg-param-displayName").cloned().unwrap_or_else(|| author.to_string());
            let mut a = AlertEvent::new("raid", "twitch", name);
            a.count = num("msg-param-viewerCount");
            a
        }
        _ => return None,
    };
    if let Some(id) = tags.get("id") {
        a.id = format!("twitch-{id}");
    }
    Some(a)
}

#[cfg(test)]
mod alert_tests {
    use super::*;

    #[test]
    fn twitch_alerts() {
        let bits = parse("@display-name=Alice;id=b1;bits=100 :alice!a@a PRIVMSG #c :Cheer100 hello there").unwrap();
        let a = bits.alert.unwrap();
        assert_eq!((a.kind.as_str(), a.amount, a.message.as_str()), ("bits", Some(100.0), "hello there"));
        let resub = parse("@display-name=Bob;id=r1;msg-id=resub;msg-param-cumulative-months=7;system-msg=x :tmi.twitch.tv USERNOTICE #c :yay").unwrap();
        let a = resub.alert.unwrap();
        assert_eq!((a.kind.as_str(), a.count, a.id.as_str()), ("sub", Some(7), "twitch-r1"));
        let mass = parse("@display-name=Gifter;id=g1;msg-id=submysterygift;msg-param-mass-gift-count=5;system-msg=x :tmi.twitch.tv USERNOTICE #c").unwrap();
        assert_eq!(mass.alert.map(|a| (a.kind, a.count)), Some(("gift".into(), Some(5))));
        let part = parse("@display-name=Gifter;id=g2;msg-id=subgift;msg-param-community-gift-id=99;msg-param-recipient-display-name=Zed;system-msg=x :tmi.twitch.tv USERNOTICE #c").unwrap();
        assert!(part.alert.is_none());
        let raid = parse("@display-name=raider;id=x1;msg-id=raid;msg-param-displayName=Raider;msg-param-viewerCount=42;system-msg=x :tmi.twitch.tv USERNOTICE #c").unwrap();
        assert_eq!(raid.alert.map(|a| (a.kind, a.name, a.count)), Some(("raid".into(), "Raider".into(), Some(42))));
        let plain = parse("@display-name=Carol;id=c1 :carol!c@c PRIVMSG #c :Cheer is a word").unwrap();
        assert!(plain.alert.is_none());
    }
}
