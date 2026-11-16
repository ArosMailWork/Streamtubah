//! Alerts: one event bus for follows, subs, gifts, bits, raids, Super Chats, memberships,
//! donations and patrons from every source (chat connectors, Ko-fi, Patreon, tests).
//!
//! Events go to the Alert Box overlay (`/overlay/alerts` + WebSocket `/alerts` on the API port,
//! added to scenes as a Browser Source) and to the app UI (`alert:event`). Styles live in
//! `settings.alerts`; custom images/sounds are copied into `<app data>/alert-media/`.

pub mod donate;
pub mod relay;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};

/// (kind, label) in display order.
pub const KINDS: [(&str, &str); 9] = [
    ("follow", "Follow"),
    ("sub", "Subscription"),
    ("gift", "Gifted subs"),
    ("bits", "Bits / Cheer"),
    ("raid", "Raid / Host"),
    ("superchat", "Super Chat / Sticker"),
    ("member", "Membership"),
    ("donation", "Donation"),
    ("patron", "New patron"),
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AlertEvent {
    pub id: String,
    pub kind: String,
    pub platform: String,
    pub name: String,
    /// Numeric amount (bits, money) for minimum-amount filters.
    pub amount: Option<f64>,
    /// Amount as shown ("$5.00", "100 bits", "50000 VND").
    pub amount_text: Option<String>,
    /// Gift count, raid viewers, sub months.
    pub count: Option<u64>,
    pub message: String,
    pub ts: u64,
    /// Fired from the "Test" buttons.
    pub test: bool,
    /// Relay service that delivered it ("streamelements", "streamlabs"); empty = read directly.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

impl AlertEvent {
    pub fn new(kind: &str, platform: &str, name: impl Into<String>) -> Self {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Self {
            id: format!("{platform}-{kind}-{ts}-{}", crate::settings::random_token(4)),
            kind: kind.into(),
            platform: platform.into(),
            name: name.into(),
            amount: None,
            amount_text: None,
            count: None,
            message: String::new(),
            ts,
            test: false,
            source: String::new(),
        }
    }
}

/// Look of one alert type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AlertStyle {
    pub enabled: bool,
    /// Text with {name} {amount} {count} {message} {platform}.
    pub template: String,
    /// File in alert-media ("" = built-in animated icon).
    pub image: String,
    /// File in alert-media, "" = built-in chime, "none" = silent.
    pub sound: String,
    pub volume: f32,
    pub duration_ms: u32,
    /// "fade" | "slide" | "zoom" | "bounce"
    pub animation: String,
    pub text_color: String,
    pub accent_color: String,
    pub font_size: u32,
    /// Ignore smaller amounts (bits, money). 0 = all.
    pub min_amount: f64,
    /// Show the viewer's message under the title.
    pub show_message: bool,
}

impl Default for AlertStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            template: "{name}".into(),
            image: String::new(),
            sound: String::new(),
            volume: 0.7,
            duration_ms: 6000,
            animation: "bounce".into(),
            text_color: "#ffffff".into(),
            accent_color: "#31c3a2".into(),
            font_size: 44,
            min_amount: 0.0,
            show_message: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AlertSettings {
    pub styles: BTreeMap<String, AlertStyle>,
    /// Pause between two alerts in the queue.
    pub gap_ms: u32,
}

impl Default for AlertSettings {
    fn default() -> Self {
        let templates = [
            ("follow", "{name} just followed!", "#9b7bff"),
            ("sub", "{name} subscribed!", "#31c3a2"),
            ("gift", "{name} gifted {count} subs!", "#f5c542"),
            ("bits", "{name} cheered {amount}!", "#7dd3fc"),
            ("raid", "{name} is raiding with {count} viewers!", "#fb923c"),
            ("superchat", "{name} sent {amount}!", "#f5c542"),
            ("member", "{name} became a member!", "#31c3a2"),
            ("donation", "{name} donated {amount}!", "#4ade80"),
            ("patron", "{name} became a patron!", "#f472b6"),
        ];
        let styles = templates
            .into_iter()
            .map(|(k, t, c)| (k.to_string(), AlertStyle { template: t.into(), accent_color: c.into(), ..Default::default() }))
            .collect();
        Self { styles, gap_ms: 800 }
    }
}

struct Hub {
    app: AppHandle,
    media: PathBuf,
    overlay: Mutex<Vec<Sender<String>>>,
    recent: Mutex<VecDeque<AlertEvent>>,
}

static HUB: OnceLock<Hub> = OnceLock::new();
const RECENT: usize = 50;

pub fn init(app: &AppHandle, data_dir: &Path) {
    let media = data_dir.join("alert-media");
    let _ = std::fs::create_dir_all(&media);
    let _ = HUB.set(Hub { app: app.clone(), media, overlay: Mutex::new(Vec::new()), recent: Mutex::new(VecDeque::new()) });
}

fn broadcast(text: String) {
    if let Some(hub) = HUB.get() {
        hub.overlay.lock().unwrap().retain(|tx| tx.send(text.clone()).is_ok());
    }
}

/// Publishes an alert (duplicates by id are dropped). Filtering by style happens in the overlay,
/// so the app keeps a full event history.
pub fn emit(ev: AlertEvent) {
    let Some(hub) = HUB.get() else { return };
    {
        let mut recent = hub.recent.lock().unwrap();
        if recent.iter().any(|e| e.id == ev.id || is_relay_duplicate(e, &ev)) {
            return;
        }
        recent.push_back(ev.clone());
        while recent.len() > RECENT {
            recent.pop_front();
        }
    }
    broadcast(json!({ "type": "alert", "event": ev }).to_string());
    let _ = hub.app.emit("alert:event", ev);
}

/// Window in which the same event from two sources counts as one.
const DEDUPE_MS: u64 = 30_000;

/// True when `new` repeats `old` through another source (e.g. a Twitch sub read from chat and
/// again from StreamElements), or is a single gift relayed after the mass gift it belongs to.
fn is_relay_duplicate(old: &AlertEvent, new: &AlertEvent) -> bool {
    if old.test || new.test || old.platform != new.platform || !old.name.eq_ignore_ascii_case(&new.name) || new.ts.abs_diff(old.ts) > DEDUPE_MS {
        return false;
    }
    let mass_gift_part = !new.source.is_empty() && new.kind == "gift" && old.kind == "gift" && new.count == Some(1) && old.count.unwrap_or(0) > 1;
    mass_gift_part || (old.source != new.source && old.kind == new.kind && old.count == new.count)
}

/// Tells open overlays to reload their styles.
pub fn config_changed() {
    broadcast(json!({ "type": "config" }).to_string());
}

/// Skips the alert currently shown in every overlay.
pub fn skip() {
    broadcast(json!({ "type": "skip" }).to_string());
}

pub fn subscribe(tx: Sender<String>) {
    if let Some(hub) = HUB.get() {
        hub.overlay.lock().unwrap().push(tx);
    }
}

pub fn recent() -> Vec<AlertEvent> {
    HUB.get().map(|h| h.recent.lock().unwrap().iter().cloned().collect()).unwrap_or_default()
}

/// Replays a past event (by id) as a new alert.
pub fn replay(id: &str) -> Result<(), String> {
    let ev = recent().into_iter().find(|e| e.id == id).ok_or("Event not found")?;
    broadcast(json!({ "type": "alert", "event": ev }).to_string());
    Ok(())
}

/// Sample event for the "Test" buttons.
pub fn test(kind: &str) -> Result<(), String> {
    if !KINDS.iter().any(|(k, _)| *k == kind) {
        return Err(format!("Unknown alert type '{kind}'"));
    }
    let mut ev = AlertEvent::new(kind, "test", "Streamtubah");
    ev.test = true;
    match kind {
        "sub" => ev.count = Some(6),
        "gift" => ev.count = Some(5),
        "bits" => (ev.amount, ev.amount_text) = (Some(500.0), Some("500 bits".into())),
        "raid" => ev.count = Some(42),
        "superchat" => (ev.amount, ev.amount_text) = (Some(5.0), Some("$5.00".into())),
        "donation" | "patron" => (ev.amount, ev.amount_text) = (Some(50000.0), Some("50.000 ₫".into())),
        _ => {}
    }
    if matches!(kind, "bits" | "superchat" | "donation" | "sub") {
        ev.message = "This is a test alert. Thanks for watching!".into();
    }
    emit(ev);
    Ok(())
}

// ---------------------------------------------------------------------------
// Custom media (images/GIFs/sounds) for alert styles

const MEDIA_EXT: [&str; 11] = ["png", "gif", "jpg", "jpeg", "webp", "apng", "mp3", "wav", "ogg", "webm", "mp4"];

/// Copies a user-chosen file into alert-media and returns its new name.
pub fn import_media(src: &str) -> Result<String, String> {
    let hub = HUB.get().ok_or("alerts not ready")?;
    let path = Path::new(src);
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !MEDIA_EXT.contains(&ext.as_str()) {
        return Err(format!("Unsupported file type '.{ext}' (use PNG, GIF, JPG, WebP, MP3, WAV, OGG, WebM or MP4)"));
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 50 * 1024 * 1024 {
        return Err("File is larger than 50 MB".into());
    }
    let name = format!("{}.{ext}", crate::settings::random_token(9));
    std::fs::copy(path, hub.media.join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

fn valid_media_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Path of an imported media file (validated name, file must exist).
pub fn media_path(name: &str) -> Option<PathBuf> {
    let hub = HUB.get()?;
    let p = hub.media.join(name);
    (valid_media_name(name) && p.is_file()).then_some(p)
}

/// Bytes + MIME type of an imported media file. Names are validated (no paths).
pub fn read_media(name: &str) -> Option<(Vec<u8>, &'static str)> {
    let hub = HUB.get()?;
    if !valid_media_name(name) {
        return None;
    }
    let ext = name.rsplit('.').next()?.to_lowercase();
    let mime = match ext.as_str() {
        "png" | "apng" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        _ => return None,
    };
    Some((std::fs::read(hub.media.join(name)).ok()?, mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_duplicates() {
        let mut direct = AlertEvent::new("sub", "twitch", "Sam");
        direct.count = Some(3);
        let mut relayed = AlertEvent::new("sub", "twitch", "sam");
        relayed.count = Some(3);
        relayed.source = "streamelements".into();
        assert!(is_relay_duplicate(&direct, &relayed));
        let mut other = relayed.clone();
        other.source = "streamlabs".into();
        assert!(is_relay_duplicate(&relayed, &other));
        // Same source twice = two real events.
        assert!(!is_relay_duplicate(&relayed, &relayed.clone()));
        let mut later = relayed.clone();
        later.ts += DEDUPE_MS + 1;
        assert!(!is_relay_duplicate(&direct, &later));
        let mut yt = relayed.clone();
        yt.platform = "youtube".into();
        assert!(!is_relay_duplicate(&direct, &yt));
        let mut mass = AlertEvent::new("gift", "twitch", "Gifty");
        mass.count = Some(5);
        mass.source = "streamelements".into();
        let mut single = AlertEvent::new("gift", "twitch", "Gifty");
        single.count = Some(1);
        single.source = "streamelements".into();
        assert!(is_relay_duplicate(&mass, &single));
    }

    #[test]
    fn defaults_cover_every_kind() {
        let s = AlertSettings::default();
        for (k, _) in KINDS {
            assert!(s.styles.contains_key(k), "{k}");
        }
    }
}
