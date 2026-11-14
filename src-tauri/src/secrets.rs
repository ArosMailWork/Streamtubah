//! Secrets window: the only place stream keys, client secrets and the API password are shown or
//! edited. The window is excluded from screen capture (Windows display affinity
//! WDA_EXCLUDEFROMCAPTURE): streams, recordings, screenshots and screen sharing see nothing,
//! even if it is opened while live. Values are read only by this window, on demand.

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::settings::{API_PASSWORD, AppSettings, GOOGLE_SECRET, KICK_SECRET};
use crate::vault;

pub const LABEL: &str = "secrets";

#[derive(Serialize)]
pub struct SecretInfo {
    /// "main.key", "target.<id>.key", "platform.google_client_secret", "api.password", …
    pub id: String,
    pub group: String,
    pub label: String,
    pub set: bool,
}

/// Secret fields a service uses: WHIP needs a bearer token, RTMP a stream key, and custom
/// RTMP servers with authentication also a password.
fn service_fields(service_type: &str, settings: &serde_json::Value) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![if service_type == "whip_custom" { ("bearer_token", "Bearer token") } else { ("key", "Stream key") }];
    if service_type == "rtmp_custom" && settings["use_auth"] == json!(true) {
        out.push(("password", "Server password"));
    }
    out
}

/// Vault key behind a secret id.
fn vault_key(s: &AppSettings, id: &str) -> Result<String, String> {
    if id == "api.password" {
        return Ok(API_PASSWORD.into());
    }
    if let Some(field) = id.strip_prefix("main.") {
        return Ok(format!("{}{field}", vault::main_prefix(&s.profile)));
    }
    if let Some(rest) = id.strip_prefix("target.") {
        let (tid, field) = rest.split_once('.').ok_or("Bad secret id")?;
        if !s.stream.targets.iter().any(|t| t.id == tid) {
            return Err("Destination not found".into());
        }
        return Ok(format!("{}{field}", vault::target_prefix(tid)));
    }
    match id {
        "platform.google_client_secret" => Ok(GOOGLE_SECRET.into()),
        "platform.kick_client_secret" => Ok(KICK_SECRET.into()),
        "donate.kofi_token" => Ok(crate::alerts::donate::KOFI_TOKEN.into()),
        "donate.patreon_token" => Ok(crate::alerts::donate::PATREON_TOKEN.into()),
        "donate.streamelements_token" => Ok(crate::alerts::relay::SE_TOKEN.into()),
        "donate.streamlabs_token" => Ok(crate::alerts::relay::SL_TOKEN.into()),
        _ => Err("Unknown secret".into()),
    }
}

pub fn list(s: &AppSettings) -> Vec<SecretInfo> {
    let full = s.with_secrets();
    let mut out = Vec::new();
    let mut push = |id: String, group: String, label: &str, set: bool| out.push(SecretInfo { id, group, label: label.into(), set });
    for (f, label) in service_fields(&s.stream.service_type, &s.stream.service_settings) {
        let set = full.stream.service_settings[f].as_str().is_some_and(|v| !v.is_empty());
        push(format!("main.{f}"), format!("Stream · Main (profile {})", s.profile), label, set);
    }
    for (t, ft) in s.stream.targets.iter().zip(&full.stream.targets) {
        for (f, label) in service_fields(&t.service_type, &t.service_settings) {
            let set = ft.service_settings[f].as_str().is_some_and(|v| !v.is_empty());
            push(format!("target.{}.{f}", t.id), format!("Stream · {}", t.name), label, set);
        }
    }
    push("platform.google_client_secret".into(), "Platform sign-in".into(), "Google client secret", !full.platform_apps.google_client_secret.is_empty());
    push("platform.kick_client_secret".into(), "Platform sign-in".into(), "Kick client secret", !full.platform_apps.kick_client_secret.is_empty());
    push("api.password".into(), "Remote control API".into(), "API password", !full.api.password.is_empty());
    let has = |k: &str| vault::get(k).is_some_and(|v| !v.is_empty());
    push("donate.kofi_token".into(), "Donations".into(), "Ko-fi verification token", has(crate::alerts::donate::KOFI_TOKEN));
    push("donate.patreon_token".into(), "Donations".into(), "Patreon creator access token", has(crate::alerts::donate::PATREON_TOKEN));
    push("donate.streamelements_token".into(), "Donations".into(), "StreamElements JWT token", has(crate::alerts::relay::SE_TOKEN));
    push("donate.streamlabs_token".into(), "Donations".into(), "Streamlabs socket API token", has(crate::alerts::relay::SL_TOKEN));
    out
}

pub fn get(s: &AppSettings, id: &str) -> Result<String, String> {
    let key = vault_key(s, id)?;
    Ok(match id {
        // Build-time defaults count as the current value.
        "platform.google_client_secret" => s.with_secrets().platform_apps.google_client_secret,
        "platform.kick_client_secret" => s.with_secrets().platform_apps.kick_client_secret,
        _ => vault::get(&key).unwrap_or_default(),
    })
}

/// Stores a secret. Returns true when the API password changed (caller reconfigures the API).
pub fn set(s: &AppSettings, id: &str, value: &str) -> Result<bool, String> {
    let key = vault_key(s, id)?;
    if id == "api.password" {
        // An empty API password would leave the API open: generate a new one instead.
        let pw = if value.is_empty() { crate::settings::random_token(16) } else { value.to_string() };
        vault::set(&key, &pw)?;
        return Ok(true);
    }
    vault::set(&key, value)?;
    Ok(false)
}

/// Opens (or focuses) the Secrets window, optionally scrolled to one secret.
pub fn open(app: &AppHandle, focus: Option<String>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(id) = focus {
            let _ = app.emit_to(LABEL, "secrets:focus", id);
        }
    } else {
        // A new page is not listening for events yet: pass the field in the URL instead.
        let url = match &focus {
            Some(id) => format!("index.html?focus={}", id.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_', "")),
            None => "index.html".into(),
        };
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(url.into()))
            .title("Streamtubah Secrets")
            .inner_size(560.0, 640.0)
            .min_inner_size(420.0, 320.0)
            .content_protected(true)
            .build()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
