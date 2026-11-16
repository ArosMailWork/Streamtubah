//! Persistent application settings (settings.json in the app data dir).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::obs::{runtime::VideoConfig, sys};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct VideoSettings {
    pub base_width: u32,
    pub base_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// "bilinear" | "bicubic" | "lanczos" | "area"
    pub scale_type: String,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            base_width: 1920,
            base_height: 1080,
            output_width: 1280,
            output_height: 720,
            fps_num: 30,
            fps_den: 1,
            scale_type: "bicubic".into(),
        }
    }
}

impl VideoSettings {
    pub fn to_config(&self) -> VideoConfig {
        let scale_type = match self.scale_type.as_str() {
            "bilinear" => sys::obs_scale_type_OBS_SCALE_BILINEAR,
            "lanczos" => sys::obs_scale_type_OBS_SCALE_LANCZOS,
            "area" => sys::obs_scale_type_OBS_SCALE_AREA,
            _ => sys::obs_scale_type_OBS_SCALE_BICUBIC,
        };
        VideoConfig {
            base_width: self.base_width.clamp(16, 7680),
            base_height: self.base_height.clamp(16, 4320),
            output_width: self.output_width.clamp(16, 7680),
            output_height: self.output_height.clamp(16, 4320),
            fps_num: self.fps_num.clamp(1, 240),
            fps_den: self.fps_den.max(1),
            scale_type,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OutputSettings {
    /// Video encoder type id; empty = pick the best available at runtime.
    pub video_encoder: String,
    /// Settings object for the chosen video encoder (edited via its properties form).
    pub video_encoder_settings: Value,
    pub audio_bitrate: u32,
    pub record_path: String,
    /// "mp4" | "fragmented_mp4" | "mkv" | "mov" | "flv"
    pub record_format: String,
    pub replay_seconds: u32,
    /// Audio tracks written by recording and replay buffer (bit 0 = track 1).
    pub record_tracks: u32,
    /// Track sent to the stream (0 = track 1).
    pub stream_track: u32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            video_encoder: String::new(),
            video_encoder_settings: json!({ "rate_control": "CBR", "bitrate": 6000, "keyint_sec": 2 }),
            audio_bitrate: 160,
            record_path: default_video_dir(),
            record_format: "fragmented_mp4".into(),
            replay_seconds: 30,
            record_tracks: 1,
            stream_track: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct StreamSettings {
    /// "rtmp_common" (service list) | "rtmp_custom" (server + key)
    pub service_type: String,
    pub service_settings: Value,
    /// The main destination (configured above) goes live with "Go Live".
    pub main_enabled: bool,
    /// Extra destinations streamed at the same time (multistream).
    pub targets: Vec<StreamTarget>,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            service_type: "rtmp_common".into(),
            service_settings: json!({ "service": "Twitch" }),
            main_enabled: true,
            targets: Vec::new(),
        }
    }
}

/// An extra multistream destination. Shares the main video encoder unless `own_encoder`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct StreamTarget {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub service_type: String,
    pub service_settings: Value,
    /// Separate video encoder at `bitrate_kbps` (costs extra GPU/CPU).
    pub own_encoder: bool,
    pub bitrate_kbps: u32,
}

impl Default for StreamTarget {
    fn default() -> Self {
        Self {
            id: random_token(6),
            name: "Destination".into(),
            enabled: true,
            service_type: "rtmp_common".into(),
            service_settings: json!({ "service": "YouTube - RTMPS" }),
            own_encoder: false,
            bitrate_kbps: 6000,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TransitionSettings {
    pub type_id: String,
    pub duration_ms: u32,
}

impl Default for TransitionSettings {
    fn default() -> Self {
        Self { type_id: "fade_transition".into(), duration_ms: 300 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AudioSettings {
    /// WASAPI endpoint id, "default", or "" (disabled).
    pub desktop_device: String,
    /// WASAPI endpoint id, "default", or "" (disabled). "default" never opens a Bluetooth headset mic.
    pub mic_device: String,
    /// Open the mic only while streaming/recording/replay buffer is active.
    pub mic_live_only: bool,
    /// Where audio monitoring (and voice preview) is played; "default" = system default output.
    pub monitoring_device: String,
    /// Extra global devices (WASAPI endpoint id, "" = disabled): Desktop Audio 2, Mic/Aux 2-4.
    pub desktop2_device: String,
    pub mic2_device: String,
    pub mic3_device: String,
    pub mic4_device: String,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            desktop_device: "default".into(),
            mic_device: "default".into(),
            mic_live_only: false,
            monitoring_device: "default".into(),
            desktop2_device: String::new(),
            mic2_device: String::new(),
            mic3_device: String::new(),
            mic4_device: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ApiSettings {
    pub enabled: bool,
    pub port: u16,
    /// Required by clients (obs-websocket v5 auth). Generated on first run.
    pub password: String,
    /// Listen on all interfaces instead of 127.0.0.1 only.
    pub allow_lan: bool,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self { enabled: true, port: 4455, password: String::new(), allow_lan: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PluginSettings {
    /// Also load plugins installed for OBS Studio (%ProgramData%/obs-studio/plugins).
    pub load_obs_studio_plugins: bool,
    /// Enabled JavaScript plugins → the permissions the user approved (see jsplugins.rs).
    pub js: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self { load_obs_studio_plugins: true, js: Default::default() }
    }
}

/// Developer app credentials for platform login (see docs/platform-apps.md).
/// Release builds bake them in via environment variables at compile time;
/// the fields stay editable so a build without them can still be configured.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PlatformApps {
    pub twitch_client_id: String,
    pub google_client_id: String,
    /// Google treats installed-app secrets as non-confidential.
    pub google_client_secret: String,
    pub kick_client_id: String,
    /// Kick requires a secret even with PKCE.
    pub kick_client_secret: String,
}

impl Default for PlatformApps {
    fn default() -> Self {
        let env = |v: Option<&str>| v.unwrap_or_default().to_string();
        Self {
            twitch_client_id: env(option_env!("STREAMTUBAH_TWITCH_CLIENT_ID")),
            google_client_id: env(option_env!("STREAMTUBAH_GOOGLE_CLIENT_ID")),
            google_client_secret: env(option_env!("STREAMTUBAH_GOOGLE_CLIENT_SECRET")),
            kick_client_id: env(option_env!("STREAMTUBAH_KICK_CLIENT_ID")),
            kick_client_secret: env(option_env!("STREAMTUBAH_KICK_CLIENT_SECRET")),
        }
    }
}

/// Channels to read chat from; empty = disabled.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ChatSettings {
    /// Channel name or twitch.tv URL.
    pub twitch: String,
    /// Channel slug, kick.com URL, or numeric chatroom id.
    pub kick: String,
    /// @handle, channel URL/id, or live video URL/id.
    pub youtube: String,
}

/// Interface options applied when the app starts.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiSettings {
    /// Render the interface without the GPU (saves ~70 MB RAM, slightly less smooth animations).
    pub low_memory: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UpdateSettings {
    /// Look for a new version on GitHub Releases at startup.
    pub auto_check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

/// Auto-Optimize preference and the PC it last ran on.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OptimizeSettings {
    /// 1 = smoothest PC … 5 = best stream.
    pub level: u8,
    /// `optimize::machine_id()` of the PC that was last optimized (or dismissed).
    pub machine: String,
}

impl Default for OptimizeSettings {
    fn default() -> Self {
        Self { level: 3, machine: String::new() }
    }
}

/// Settings switched together by a profile (Settings → Profile).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Profile {
    pub video: VideoSettings,
    pub output: OutputSettings,
    pub stream: StreamSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AppSettings {
    pub video: VideoSettings,
    pub output: OutputSettings,
    pub stream: StreamSettings,
    /// Active scene collection (file `collections/<name>.json`).
    pub collection: String,
    /// Active profile (file `profiles/<name>.json`).
    pub profile: String,
    /// Action id -> accelerator ("Ctrl+Shift+F1"). See `hotkeys::ACTIONS`.
    pub hotkeys: BTreeMap<String, String>,
    pub optimize: OptimizeSettings,
    pub updates: UpdateSettings,
    pub ui: UiSettings,
    pub alerts: crate::alerts::AlertSettings,
    pub donate: crate::alerts::donate::DonateSettings,
    pub transition: TransitionSettings,
    pub audio: AudioSettings,
    pub api: ApiSettings,
    pub plugins: PluginSettings,
    pub chat: ChatSettings,
    pub platform_apps: PlatformApps,
    pub vtuber: crate::vtuber::VtuberSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            video: Default::default(),
            output: Default::default(),
            stream: Default::default(),
            collection: "Scenes".into(),
            profile: "Default".into(),
            hotkeys: BTreeMap::new(),
            optimize: Default::default(),
            updates: Default::default(),
            ui: Default::default(),
            alerts: Default::default(),
            donate: Default::default(),
            transition: Default::default(),
            audio: Default::default(),
            api: Default::default(),
            plugins: Default::default(),
            chat: Default::default(),
            platform_apps: Default::default(),
            vtuber: Default::default(),
        }
    }
}

impl AppSettings {
    pub fn profile(&self) -> Profile {
        Profile { video: self.video.clone(), output: self.output.clone(), stream: self.stream.clone() }
    }

    pub fn load(path: &Path) -> Self {
        // Editors such as Notepad may add a UTF-8 BOM, which serde_json rejects.
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(path, &s)
    }
}

fn default_video_dir() -> String {
    std::env::var_os("USERPROFILE")
        .map(|h| PathBuf::from(h).join("Videos"))
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .into_owned()
}

/// Validates a user-chosen collection/profile name, which is also a file name.
pub fn valid_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() || n.len() > 64 {
        return Err("Name must be 1-64 characters".into());
    }
    if n.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) || n.ends_with('.') {
        return Err("Name contains characters not allowed in file names".into());
    }
    Ok(n.to_string())
}

/// Writes via a temp file so a crash mid-write never leaves a truncated file.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// URL-safe random token (used for the API password).
pub fn random_token(bytes: usize) -> String {
    use base64::Engine;
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Vault keys of platform app secrets (see `vault`).
pub const GOOGLE_SECRET: &str = "platform.google_client_secret";
pub const KICK_SECRET: &str = "platform.kick_client_secret";
pub const API_PASSWORD: &str = "api.password";

impl AppSettings {
    /// Moves every secret (stream keys of main + destinations, platform client secrets) into the
    /// encrypted vault, leaving them out of settings.json. Returns true when anything moved.
    pub fn take_secrets(&mut self) -> Result<bool, String> {
        let mut moved = crate::vault::take_service_secrets(&crate::vault::main_prefix(&self.profile), &mut self.stream.service_settings)?;
        for t in &mut self.stream.targets {
            moved |= crate::vault::take_service_secrets(&crate::vault::target_prefix(&t.id), &mut t.service_settings)?;
        }
        let defaults = PlatformApps::default();
        for (field, key, default) in [
            (&mut self.platform_apps.google_client_secret, GOOGLE_SECRET, defaults.google_client_secret),
            (&mut self.platform_apps.kick_client_secret, KICK_SECRET, defaults.kick_client_secret),
        ] {
            // Values baked into the build are not user secrets; anything else goes to the vault.
            if !field.is_empty() && *field != default {
                crate::vault::set(key, field)?;
                moved = true;
            }
            field.clear();
        }
        if !self.api.password.is_empty() {
            crate::vault::set(API_PASSWORD, &self.api.password)?;
            self.api.password.clear();
            moved = true;
        }
        Ok(moved)
    }

    /// Copy with secrets from the vault filled back in, for code that talks to services.
    pub fn with_secrets(&self) -> AppSettings {
        let mut s = self.clone();
        s.stream.service_settings = crate::vault::with_service_secrets(&crate::vault::main_prefix(&s.profile), &s.stream.service_settings);
        for t in &mut s.stream.targets {
            t.service_settings = crate::vault::with_service_secrets(&crate::vault::target_prefix(&t.id), &t.service_settings);
        }
        let defaults = PlatformApps::default();
        s.platform_apps.google_client_secret = crate::vault::get(GOOGLE_SECRET).unwrap_or(defaults.google_client_secret);
        s.platform_apps.kick_client_secret = crate::vault::get(KICK_SECRET).unwrap_or(defaults.kick_client_secret);
        s.api.password = crate::vault::get(API_PASSWORD).unwrap_or_default();
        s
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loads_settings_with_bom() {
        let p = std::env::temp_dir().join(format!("stb-bom-{}.json", std::process::id()));
        std::fs::write(&p, "\u{feff}{\"ui\": {\"low_memory\": true}, \"profile\": \"Mine\"}").unwrap();
        let s = super::AppSettings::load(&p);
        let _ = std::fs::remove_file(&p);
        assert!(s.ui.low_memory && s.profile == "Mine");
    }
}
