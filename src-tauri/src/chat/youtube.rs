//! YouTube live chat via the same internal endpoint the chat popout uses
//! (`youtubei/v1/live_chat/get_live_chat`). No API key or login, but it is
//! unofficial: YouTube may change the schema at any time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::{ChatMessage, http, publish, set_status};

pub fn run(target: &str, stop: &AtomicBool) -> Result<(), String> {
    let agent = http();
    let video = resolve_video(&agent, target)?;
    let page = get(&agent, &format!("https://www.youtube.com/live_chat?is_popout=1&v={video}"))?;
    let key = between(&page, "\"INNERTUBE_API_KEY\":\"", "\"").ok_or("Live chat page not found (is the stream live?)")?;
    let version = between(&page, "\"INNERTUBE_CLIENT_VERSION\":\"", "\"").unwrap_or_else(|| "2.20260101.00.00".into());
    let initial = embedded_json(&page, "ytInitialData").ok_or("Live chat is unavailable for this video")?;
    let mut continuation = all_messages_continuation(&initial)
        .or_else(|| next_continuation(&initial["contents"]["liveChatRenderer"]).map(|(c, _)| c))
        .ok_or("Chat is disabled or the stream has ended")?;
    super::update_context("youtube", |c| c.video_id = Some(video.clone()));
    set_status("youtube", "connected", &video);

    let url = format!("https://www.youtube.com/youtubei/v1/live_chat/get_live_chat?key={key}&prettyPrint=false");
    while !stop.load(Ordering::Relaxed) {
        let body = json!({ "context": { "client": { "clientName": "WEB", "clientVersion": version } }, "continuation": continuation });
        let resp: Value = agent
            .post(&url)
            .send_json(&body)
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_json()
            .map_err(|e| e.to_string())?;
        let chat = &resp["continuationContents"]["liveChatContinuation"];
        for action in chat["actions"].as_array().into_iter().flatten() {
            if let Some(m) = parse_item(&action["addChatItemAction"]["item"]) {
                publish(m);
            }
        }
        let (next, wait) = next_continuation(chat).ok_or("Stream ended")?;
        continuation = next;
        let wait = wait.clamp(1000, 5000);
        for _ in 0..wait / 100 {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .header("Accept-Language", "en-US,en;q=0.9")
        // Skip the EU consent interstitial.
        .header("Cookie", "CONSENT=YES+1; SOCS=CAI")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// Accepts a video id, a watch/live/youtu.be URL, a channel URL, or an @handle.
fn resolve_video(agent: &ureq::Agent, target: &str) -> Result<String, String> {
    let t = target.trim();
    let is_id = |s: &str| s.len() == 11 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if is_id(t) {
        return Ok(t.to_string());
    }
    for marker in ["v=", "youtu.be/", "/live/"] {
        if let Some(rest) = t.split(marker).nth(1) {
            let id: String = rest.chars().take(11).collect();
            if is_id(&id) {
                return Ok(id);
            }
        }
    }
    let channel_url = if t.starts_with("http") {
        t.trim_end_matches('/').to_string()
    } else if t.starts_with('@') {
        format!("https://www.youtube.com/{t}")
    } else if t.starts_with("UC") {
        format!("https://www.youtube.com/channel/{t}")
    } else {
        format!("https://www.youtube.com/@{t}")
    };
    let page = get(agent, &format!("{channel_url}/live"))?;
    between(&page, "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=", "\"")
        .filter(|id| is_id(id))
        .ok_or_else(|| "Channel is not live right now".into())
}

fn between(s: &str, start: &str, end: &str) -> Option<String> {
    let from = s.find(start)? + start.len();
    let to = s[from..].find(end)? + from;
    Some(s[from..to].to_string())
}

/// Extracts `var NAME = {...};` / `window["NAME"] = {...};` from a page.
fn embedded_json(page: &str, name: &str) -> Option<Value> {
    let at = page.find(&format!("{name}\"] = ")).or_else(|| page.find(&format!("{name} = ")))?;
    let start = at + page[at..].find('{')?;
    serde_json::Deserializer::from_str(&page[start..]).into_iter::<Value>().next()?.ok()
}

/// The "Live chat" (all messages) view instead of the default "Top chat".
fn all_messages_continuation(initial: &Value) -> Option<String> {
    let items = initial["contents"]["liveChatRenderer"]["header"]["liveChatHeaderRenderer"]["viewSelector"]
        ["sortFilterSubMenuRenderer"]["subMenuItems"]
        .as_array()?;
    items.last()?["continuation"]["reloadContinuationData"]["continuation"].as_str().map(String::from)
}

fn next_continuation(chat: &Value) -> Option<(String, u64)> {
    for c in chat["continuations"].as_array()? {
        for kind in ["invalidationContinuationData", "timedContinuationData", "reloadContinuationData"] {
            if let Some(token) = c[kind]["continuation"].as_str() {
                return Some((token.to_string(), c[kind]["timeoutMs"].as_u64().unwrap_or(2000)));
            }
        }
    }
    None
}

fn runs_text(v: &Value) -> String {
    if let Some(s) = v["simpleText"].as_str() {
        return s.to_string();
    }
    v["runs"]
        .as_array()
        .map(|runs| {
            runs.iter()
                .map(|r| {
                    r["text"].as_str().map(String::from).unwrap_or_else(|| {
                        r["emoji"]["shortcuts"][0].as_str().or(r["emoji"]["emojiId"].as_str()).unwrap_or("").to_string()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_item(item: &Value) -> Option<ChatMessage> {
    let (renderer, kind) = [
        ("liveChatTextMessageRenderer", "message"),
        ("liveChatPaidMessageRenderer", "paid"),
        ("liveChatPaidStickerRenderer", "paid"),
        ("liveChatMembershipItemRenderer", "membership"),
        ("liveChatSponsorshipsGiftPurchaseAnnouncementRenderer", "membership"),
    ]
    .into_iter()
    .find(|(k, _)| item.get(*k).is_some())?;
    let r = &item[renderer];
    let author = runs_text(&r["authorName"]);
    let text = match renderer {
        "liveChatMembershipItemRenderer" => {
            let head = runs_text(&r["headerSubtext"]);
            let body = runs_text(&r["message"]);
            if body.is_empty() { head } else { format!("{head} — {body}") }
        }
        "liveChatSponsorshipsGiftPurchaseAnnouncementRenderer" => {
            runs_text(&r["header"]["liveChatSponsorshipsHeaderRenderer"]["primaryText"])
        }
        _ => runs_text(&r["message"]),
    };
    let author = if author.is_empty() {
        runs_text(&r["header"]["liveChatSponsorshipsHeaderRenderer"]["authorName"])
    } else {
        author
    };
    let mut m = ChatMessage::new("youtube", author, text);
    m.kind = kind;
    if let Some(id) = r["id"].as_str() {
        m.id = format!("youtube-{id}");
        m.msg_id = Some(id.to_string());
    }
    m.author_id = r["authorExternalChannelId"].as_str().map(String::from);
    m.amount = r["purchaseAmountText"]["simpleText"].as_str().map(String::from);
    m.badges = r["authorBadges"]
        .as_array()
        .map(|b| b.iter().filter_map(|x| x["liveChatAuthorBadgeRenderer"]["tooltip"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_super_chat() {
        let item = json!({ "liveChatPaidMessageRenderer": {
            "id": "x1", "authorName": { "simpleText": "Carol" },
            "purchaseAmountText": { "simpleText": "$5.00" },
            "message": { "runs": [{ "text": "hi " }, { "emoji": { "shortcuts": [":wave:"] } }] }
        }});
        let m = parse_item(&item).unwrap();
        assert_eq!((m.author.as_str(), m.text.as_str(), m.kind), ("Carol", "hi :wave:", "paid"));
        assert_eq!(m.amount.as_deref(), Some("$5.00"));
    }

    #[test]
    fn extracts_embedded_json() {
        let page = r#"<script>var ytInitialData = {"a":{"b":1}};</script>"#;
        assert_eq!(embedded_json(page, "ytInitialData").unwrap()["a"]["b"], 1);
    }
}
