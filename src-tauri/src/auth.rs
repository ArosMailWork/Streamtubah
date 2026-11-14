//! Platform login (OAuth) and token storage.
//!
//! - Twitch: Device Code Grant with a public client (no secret in the app).
//! - Google/YouTube: installed-app flow, loopback redirect + PKCE.
//! - Kick: authorization code + PKCE on a fixed loopback port (Kick also requires the client secret).
//!
//! Tokens are stored in `accounts.json` in the app data folder (like OBS stores its
//! service tokens) and refreshed automatically.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::chat::raw_http;
use crate::settings::{PlatformApps, random_token, write_atomic};

pub const KICK_REDIRECT_PORT: u16 = 17563;

const TWITCH_SCOPES: &str = "user:read:chat user:write:chat moderator:manage:banned_users moderator:manage:chat_messages \
channel:manage:moderators channel:manage:vips moderator:read:followers channel:read:subscriptions bits:read";
const GOOGLE_SCOPES: &str = "https://www.googleapis.com/auth/youtube.force-ssl";
const KICK_SCOPES: &str = "user:read channel:read chat:write moderation:ban moderation:chat_message:manage events:subscribe";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Account {
    pub platform: String,
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds; 0 = unknown.
    pub expires_at: u64,
}

/// What the UI may see (no tokens).
#[derive(Serialize, Clone)]
pub struct AccountInfo {
    pub platform: String,
    pub login: String,
    pub display_name: String,
}

static PATH: OnceLock<PathBuf> = OnceLock::new();
static ACCOUNTS: LazyLock<Mutex<HashMap<String, Account>>> = LazyLock::new(Default::default);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn init(path: PathBuf) {
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, Account>>(&text) {
            *ACCOUNTS.lock().unwrap() = map;
        }
    }
    let _ = PATH.set(path);
}

fn save() {
    let Some(path) = PATH.get() else { return };
    if let Ok(text) = serde_json::to_string_pretty(&*ACCOUNTS.lock().unwrap()) {
        let _ = write_atomic(path, &text);
    }
}

pub fn list() -> Vec<AccountInfo> {
    let mut v: Vec<_> = ACCOUNTS
        .lock()
        .unwrap()
        .values()
        .map(|a| AccountInfo { platform: a.platform.clone(), login: a.login.clone(), display_name: a.display_name.clone() })
        .collect();
    v.sort_by(|a, b| a.platform.cmp(&b.platform));
    v
}

pub fn logout(platform: &str) {
    ACCOUNTS.lock().unwrap().remove(platform);
    save();
}

fn store(a: Account) {
    ACCOUNTS.lock().unwrap().insert(a.platform.clone(), a);
    save();
}

/// A valid account for the platform, refreshing the token if it is about to expire.
pub fn account(platform: &str, apps: &PlatformApps) -> Result<Account, String> {
    let acc = ACCOUNTS.lock().unwrap().get(platform).cloned().ok_or_else(|| format!("Sign in to {platform} first"))?;
    if acc.expires_at == 0 || acc.expires_at > now() + 120 {
        return Ok(acc);
    }
    let refreshed = refresh(&acc, apps)?;
    store(refreshed.clone());
    Ok(refreshed)
}

fn emit(app: &AppHandle, platform: &str, state: &str, extra: Value) {
    let mut payload = json!({ "platform": platform, "state": state });
    if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    let _ = app.emit("auth:status", payload);
}

/// Starts a login in the background. Progress is reported via `auth:status` events.
pub fn login(app: AppHandle, platform: String, apps: PlatformApps) -> Result<(), String> {
    match platform.as_str() {
        "twitch" if apps.twitch_client_id.is_empty() => return Err("Twitch client ID is not configured".into()),
        "youtube" if apps.google_client_id.is_empty() => return Err("Google client ID is not configured".into()),
        "kick" if apps.kick_client_id.is_empty() || apps.kick_client_secret.is_empty() => {
            return Err("Kick client ID / secret are not configured".into());
        }
        "twitch" | "youtube" | "kick" => {}
        _ => return Err(format!("Unknown platform '{platform}'")),
    }
    std::thread::spawn(move || {
        let result = match platform.as_str() {
            "twitch" => twitch_login(&app, &apps),
            "youtube" => google_login(&app, &apps),
            _ => kick_login(&app, &apps),
        };
        match result {
            Ok(acc) => {
                emit(&app, &platform, "done", json!({ "login": acc.display_name }));
                store(acc);
            }
            Err(e) => emit(&app, &platform, "error", json!({ "message": e })),
        }
    });
    Ok(())
}

fn open_browser(url: &str) {
    let _ = tauri_plugin_opener::open_url(url, None::<&str>);
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", k, urlencode(v))).collect::<Vec<_>>().join("&")
}

pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<Value, String> {
    let resp = ureq::post(url)
        .config()
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form(pairs))
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body: Value = resp.into_body().read_json().unwrap_or(Value::Null);
    if status >= 400 {
        let msg = body["error_description"].as_str().or(body["message"].as_str()).or(body["error"].as_str()).unwrap_or("");
        return Err(format!("{status} {msg}"));
    }
    Ok(body)
}

fn pkce() -> (String, String) {
    let verifier = random_token(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Waits for the browser redirect on a loopback listener; returns the `code` query value.
fn wait_for_code(listener: TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(false).map_err(|e| e.to_string())?;
    let deadline = SystemTime::now() + Duration::from_secs(300);
    loop {
        if SystemTime::now() > deadline {
            return Err("Login timed out".into());
        }
        let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
        let path = line.split_whitespace().nth(1).unwrap_or("");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let param = |k: &str| {
            query.split('&').find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| urldecode(v)))
        };
        if param("code").is_none() && param("error").is_none() {
            // favicon or unrelated request
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        let ok = param("state").as_deref() == Some(state) && param("code").is_some();
        let page = if ok {
            "<h2>Signed in. You can close this tab and return to Streamtubah.</h2>"
        } else {
            "<h2>Sign-in failed. Return to Streamtubah and try again.</h2>"
        };
        let _ = stream.write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{page}", page.len())
                .as_bytes(),
        );
        if !ok {
            return Err(param("error_description").or(param("error")).unwrap_or_else(|| "state mismatch".into()));
        }
        return Ok(param("code").unwrap());
    }
}

fn urldecode(s: &str) -> String {
    let bytes = s.replace('+', " ").into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn expires(body: &Value) -> u64 {
    body["expires_in"].as_u64().map_or(0, |s| now() + s)
}

// ---------------------------------------------------------------------------
// Twitch (device code)

fn twitch_login(app: &AppHandle, apps: &PlatformApps) -> Result<Account, String> {
    let id = apps.twitch_client_id.as_str();
    let dev = post_form("https://id.twitch.tv/oauth2/device", &[("client_id", id), ("scopes", TWITCH_SCOPES)])?;
    let device_code = dev["device_code"].as_str().ok_or("no device code")?.to_string();
    let uri = dev["verification_uri"].as_str().unwrap_or("https://www.twitch.tv/activate").to_string();
    let interval = dev["interval"].as_u64().unwrap_or(5);
    emit(app, "twitch", "pending", json!({ "user_code": dev["user_code"], "verification_uri": uri }));
    open_browser(&uri);
    let deadline = now() + dev["expires_in"].as_u64().unwrap_or(1800);
    let token = loop {
        std::thread::sleep(Duration::from_secs(interval));
        if now() > deadline {
            return Err("Login timed out".into());
        }
        match post_form(
            "https://id.twitch.tv/oauth2/token",
            &[
                ("client_id", id),
                ("scopes", TWITCH_SCOPES),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        ) {
            Ok(t) => break t,
            Err(e) if e.contains("authorization_pending") || e.contains("slow_down") => continue,
            Err(e) => return Err(e),
        }
    };
    let access = token["access_token"].as_str().ok_or("no access token")?.to_string();
    let user = twitch_get(id, &access, "/helix/users")?;
    let u = &user["data"][0];
    Ok(Account {
        platform: "twitch".into(),
        user_id: u["id"].as_str().unwrap_or_default().into(),
        login: u["login"].as_str().unwrap_or_default().into(),
        display_name: u["display_name"].as_str().unwrap_or_default().into(),
        access_token: access,
        refresh_token: token["refresh_token"].as_str().unwrap_or_default().into(),
        expires_at: expires(&token),
    })
}

pub fn twitch_get(client_id: &str, token: &str, path: &str) -> Result<Value, String> {
    ureq::get(&format!("https://api.twitch.tv{path}"))
        .header("Client-Id", client_id)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Google / YouTube (loopback + PKCE)

fn google_login(app: &AppHandle, apps: &PlatformApps) -> Result<Account, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let (verifier, challenge) = pkce();
    let state = random_token(16);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent&state={state}",
        urlencode(&apps.google_client_id),
        urlencode(&redirect),
        urlencode(GOOGLE_SCOPES),
    );
    emit(app, "youtube", "pending", json!({ "verification_uri": url }));
    open_browser(&url);
    let code = wait_for_code(listener, &state)?;
    let token = post_form(
        "https://oauth2.googleapis.com/token",
        &[
            ("client_id", &apps.google_client_id),
            ("client_secret", &apps.google_client_secret),
            ("code", &code),
            ("code_verifier", &verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &redirect),
        ],
    )?;
    let access = token["access_token"].as_str().ok_or("no access token")?.to_string();
    let me: Value = ureq::get("https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true")
        .header("Authorization", &format!("Bearer {access}"))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let ch = &me["items"][0];
    let title = ch["snippet"]["title"].as_str().unwrap_or("YouTube").to_string();
    Ok(Account {
        platform: "youtube".into(),
        user_id: ch["id"].as_str().unwrap_or_default().into(),
        login: ch["snippet"]["customUrl"].as_str().unwrap_or(&title).into(),
        display_name: title,
        access_token: access,
        refresh_token: token["refresh_token"].as_str().unwrap_or_default().into(),
        expires_at: expires(&token),
    })
}

// ---------------------------------------------------------------------------
// Kick (fixed loopback + PKCE + secret)

fn kick_login(app: &AppHandle, apps: &PlatformApps) -> Result<Account, String> {
    let listener = TcpListener::bind(("127.0.0.1", KICK_REDIRECT_PORT))
        .map_err(|e| format!("Port {KICK_REDIRECT_PORT} is busy: {e}"))?;
    let redirect = format!("http://localhost:{KICK_REDIRECT_PORT}/callback");
    let (verifier, challenge) = pkce();
    let state = random_token(16);
    let url = format!(
        "https://id.kick.com/oauth/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        urlencode(&apps.kick_client_id),
        urlencode(&redirect),
        urlencode(KICK_SCOPES),
    );
    emit(app, "kick", "pending", json!({ "verification_uri": url }));
    open_browser(&url);
    let code = wait_for_code(listener, &state)?;
    let token = kick_token(&[
        ("code", &code),
        ("client_id", &apps.kick_client_id),
        ("client_secret", &apps.kick_client_secret),
        ("redirect_uri", &redirect),
        ("grant_type", "authorization_code"),
        ("code_verifier", &verifier),
    ])?;
    let access = token["access_token"].as_str().ok_or("no access token")?.to_string();
    let auth = format!("Bearer {access}");
    let me = raw_http::json("api.kick.com", "GET", "/public/v1/users", &[("Authorization", &auth)], None)?;
    let u = &me["data"][0];
    let name = u["name"].as_str().unwrap_or("kick").to_string();
    Ok(Account {
        platform: "kick".into(),
        user_id: u["user_id"].as_u64().map(|v| v.to_string()).unwrap_or_default(),
        login: name.clone(),
        display_name: name,
        access_token: access,
        refresh_token: token["refresh_token"].as_str().unwrap_or_default().into(),
        expires_at: expires(&token),
    })
}

fn kick_token(pairs: &[(&str, &str)]) -> Result<Value, String> {
    let body = raw_http::Body { content_type: "application/x-www-form-urlencoded", data: form(pairs) };
    let (status, text) = raw_http::request("id.kick.com", "POST", "/oauth/token", &[], Some(body))?;
    if status >= 400 {
        return Err(format!("{status} {}", text.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Refresh

fn refresh(acc: &Account, apps: &PlatformApps) -> Result<Account, String> {
    if acc.refresh_token.is_empty() {
        return Err(format!("{} session expired; sign in again", acc.platform));
    }
    let rt = acc.refresh_token.as_str();
    let token = match acc.platform.as_str() {
        "twitch" => post_form(
            "https://id.twitch.tv/oauth2/token",
            &[("grant_type", "refresh_token"), ("refresh_token", rt), ("client_id", &apps.twitch_client_id)],
        )?,
        "youtube" => post_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", rt),
                ("client_id", &apps.google_client_id),
                ("client_secret", &apps.google_client_secret),
            ],
        )?,
        _ => kick_token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", rt),
            ("client_id", &apps.kick_client_id),
            ("client_secret", &apps.kick_client_secret),
        ])?,
    };
    let mut next = acc.clone();
    next.access_token = token["access_token"].as_str().ok_or("refresh failed")?.to_string();
    if let Some(r) = token["refresh_token"].as_str() {
        next.refresh_token = r.to_string();
    }
    next.expires_at = expires(&token);
    Ok(next)
}

#[cfg(test)]
mod tests {
    #[test]
    fn url_codec_round_trip() {
        let s = "a b&c=d/é";
        assert_eq!(super::urldecode(&super::urlencode(s)), s);
    }
}
