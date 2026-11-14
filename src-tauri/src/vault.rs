//! Encrypted secret store: stream keys and platform client secrets.
//!
//! Stored in `secrets.bin` encrypted with Windows DPAPI for the current Windows user, so the
//! file is useless when copied to another PC or read by another user. Moving the portable app
//! to another PC therefore loses the secrets (they must be entered again); the unreadable file
//! is kept as `secrets.bin.unreadable`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};

use crate::settings::write_atomic;

/// Secret fields of libobs streaming services (stream key, WHIP token, RTMP auth password).
pub const SERVICE_SECRET_FIELDS: [&str; 3] = ["key", "bearer_token", "password"];
/// Ties the ciphertext to this app (DPAPI "optional entropy").
const ENTROPY: &[u8] = b"Streamtubah secret vault v1";

struct Vault {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

static VAULT: Mutex<Option<Vault>> = Mutex::new(None);
/// True when an existing vault could not be decrypted (other PC or Windows user).
static LOST: Mutex<bool> = Mutex::new(false);

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

fn take_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    // SAFETY: DPAPI allocated `out` with LocalAlloc; copied, then freed exactly once.
    unsafe {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        v
    }
}

fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let (input, entropy) = (blob(plain), blob(ENTROPY));
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe { CryptProtectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
        .map_err(|e| format!("Encrypting secrets failed: {e}"))?;
    Ok(take_blob(out))
}

fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    let (input, entropy) = (blob(cipher), blob(ENTROPY));
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe { CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
        .map_err(|e| format!("Decrypting secrets failed: {e}"))?;
    Ok(take_blob(out))
}

pub fn init(path: PathBuf) {
    let mut map = BTreeMap::new();
    if let Ok(raw) = std::fs::read(&path) {
        match unprotect(&decode_file(&raw)).and_then(|p| serde_json::from_slice(&p).map_err(|e| e.to_string())) {
            Ok(m) => map = m,
            Err(e) => {
                eprintln!("secret vault unreadable ({e}); starting empty");
                let _ = std::fs::rename(&path, path.with_extension("bin.unreadable"));
                *LOST.lock().unwrap() = true;
            }
        }
    }
    *VAULT.lock().unwrap() = Some(Vault { path, map });
}

/// Secrets saved on another PC/user could not be read and must be entered again.
pub fn lost() -> bool {
    *LOST.lock().unwrap()
}

fn with<T>(f: impl FnOnce(&mut Vault) -> T) -> T {
    let mut g = VAULT.lock().unwrap();
    f(g.as_mut().expect("vault::init not called"))
}

fn persist(v: &Vault) -> Result<(), String> {
    let plain = serde_json::to_vec(&v.map).map_err(|e| e.to_string())?;
    let cipher = protect(&plain)?;
    // write_atomic takes text; store the ciphertext as base64.
    use base64::Engine;
    write_atomic(&v.path, &base64::engine::general_purpose::STANDARD.encode(cipher))
}

/// Reads back what `persist` wrote (base64 of the DPAPI blob).
fn decode_file(raw: &[u8]) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(raw.trim_ascii()).unwrap_or_default()
}

pub fn get(key: &str) -> Option<String> {
    with(|v| v.map.get(key).cloned())
}

/// Stores a secret; an empty value deletes it.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    with(|v| {
        if value.is_empty() {
            v.map.remove(key);
        } else {
            v.map.insert(key.to_string(), value.to_string());
        }
        persist(v)
    })
}

/// Copies every secret under `from` (a key prefix) to the same suffix under `to`.
pub fn copy_prefix(from: &str, to: &str) -> Result<(), String> {
    with(|v| {
        let found: Vec<(String, String)> =
            v.map.iter().filter_map(|(k, val)| k.strip_prefix(from).map(|rest| (format!("{to}{rest}"), val.clone()))).collect();
        v.map.extend(found);
        persist(v)
    })
}

pub fn remove_prefix(prefix: &str) -> Result<(), String> {
    with(|v| {
        v.map.retain(|k, _| !k.starts_with(prefix));
        persist(v)
    })
}

/// Vault prefix of the main destination of a profile, e.g. `stream@Default.`.
pub fn main_prefix(profile: &str) -> String {
    format!("stream@{profile}.")
}

/// Vault prefix of an extra destination (ids are unique across profiles).
pub fn target_prefix(id: &str) -> String {
    format!("target.{id}.")
}

/// Moves secret fields out of a service settings object into the vault. Returns true when
/// something was moved (the caller should save the stripped settings).
pub fn take_service_secrets(prefix: &str, settings: &mut Value) -> Result<bool, String> {
    let Some(obj) = settings.as_object_mut() else { return Ok(false) };
    let mut moved = false;
    for field in SERVICE_SECRET_FIELDS {
        if let Some(val) = obj.remove(field) {
            if let Some(s) = val.as_str().filter(|s| !s.is_empty()) {
                set(&format!("{prefix}{field}"), s)?;
            }
            moved = true;
        }
    }
    Ok(moved)
}

/// Service settings with the vault's secrets put back (for creating the libobs service).
pub fn with_service_secrets(prefix: &str, settings: &Value) -> Value {
    let mut s = if settings.is_object() { settings.clone() } else { Value::Object(Default::default()) };
    for field in SERVICE_SECRET_FIELDS {
        if let Some(val) = get(&format!("{prefix}{field}")) {
            s[field] = Value::String(val);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip() {
        let c = protect(b"live_123_secret").unwrap();
        assert!(!c.windows(6).any(|w| w == b"secret"), "ciphertext must not contain the plaintext");
        assert_eq!(unprotect(&c).unwrap(), b"live_123_secret");
    }

    #[test]
    fn vault_file_is_encrypted_and_reloads() {
        let dir = std::env::temp_dir().join(format!("stb-vault-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.bin");
        init(path.clone());
        set("stream@Default.key", "live_abc_SECRETKEY").unwrap();
        let mut svc = serde_json::json!({ "service": "Twitch", "key": "live_xyz_OTHER", "server": "auto" });
        assert!(take_service_secrets("target.t1.", &mut svc).unwrap());
        assert!(svc.get("key").is_none() && svc["server"] == "auto");
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("SECRETKEY"));
        assert!(!String::from_utf8_lossy(&raw).contains("OTHER"));
        // Reload from disk.
        init(path.clone());
        assert_eq!(get("stream@Default.key").as_deref(), Some("live_abc_SECRETKEY"));
        assert_eq!(with_service_secrets("target.t1.", &svc)["key"], "live_xyz_OTHER");
        copy_prefix("stream@Default.", "stream@Copy.").unwrap();
        assert_eq!(get("stream@Copy.key").as_deref(), Some("live_abc_SECRETKEY"));
        remove_prefix("stream@Copy.").unwrap();
        assert!(get("stream@Copy.key").is_none());
        set("stream@Default.key", "").unwrap();
        assert!(get("stream@Default.key").is_none());
        // A corrupted/foreign file is set aside, not fatal.
        std::fs::write(&path, "bm90IGEgdmFsaWQgYmxvYg==").unwrap();
        init(path.clone());
        assert!(lost() && get("stream@Default.key").is_none());
        assert!(dir.join("secrets.bin.unreadable").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
