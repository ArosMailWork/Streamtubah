//! Self-update from GitHub Releases for the portable build.
//!
//! `latest.json` (a release asset) describes the newest version. The app-only zip holds just
//! `streamtubah.exe`; it must match its SHA-256 and carry an Ed25519 signature made with the
//! project's private key (see `examples/release_tool.rs`), so a tampered download or a hijacked
//! release cannot install code. Installing renames the running exe to `streamtubah.old.exe`
//! (allowed on Windows while it runs), puts the new one in place, and deletes the old one on the
//! next start. Releases that change the bundled OBS runtime are not installed automatically:
//! the user is pointed to the full package instead.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Public half of the release signing key.
const PUBLIC_KEY: &str = "b+rj8Vuj0+s5x9d5UhHpMwb3y2irOKUDndGDBp/CiE4=";
const MANIFEST_URL: &str = "https://github.com/ArosMailWork/Streamtubah/releases/latest/download/latest.json";
/// OBS runtime shipped next to the exe (must match `scripts/setup-obs.sh`).
pub const OBS_RUNTIME: &str = "32.1.0";
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

#[derive(Deserialize, Serialize, Clone)]
pub struct Asset {
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    pub obs_runtime: String,
    pub app: Asset,
    pub full: Asset,
}

#[derive(Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub available: bool,
    pub version: String,
    pub notes: String,
    /// The release changes the OBS runtime: only the full package can update it.
    pub needs_full_package: bool,
    pub full_url: String,
    #[serde(skip)]
    manifest: Option<Manifest>,
}

/// Manifest location; STREAMTUBAH_UPDATE_URL points to a self-hosted or test server.
fn manifest_url() -> String {
    std::env::var("STREAMTUBAH_UPDATE_URL").unwrap_or_else(|_| MANIFEST_URL.into())
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "1.2.10" > "1.2.9"; non-numeric parts count as 0.
fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(candidate) > parse(current)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).build().into()
}

pub fn check() -> Result<UpdateInfo, String> {
    let text = agent()
        .get(&manifest_url())
        .call()
        .map_err(|e| format!("Could not reach the update server: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let m: Manifest = serde_json::from_str(&text).map_err(|e| format!("Invalid update manifest: {e}"))?;
    let current = current_version().to_string();
    Ok(UpdateInfo {
        available: newer(&m.version, &current),
        needs_full_package: m.obs_runtime != OBS_RUNTIME,
        version: m.version.clone(),
        notes: m.notes.clone(),
        full_url: m.full.url.clone(),
        current,
        manifest: Some(m),
    })
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| format!("Download failed: {e}"))?;
    let mut data = Vec::new();
    resp.into_body()
        .into_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("Download failed: {e}"))?;
    if data.len() as u64 > MAX_DOWNLOAD {
        return Err("Update file is unexpectedly large".into());
    }
    Ok(data)
}

/// SHA-256 and Ed25519 signature of a downloaded asset.
fn verify(data: &[u8], asset: &Asset) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let digest = Sha256::digest(data);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(asset.sha256.trim()) {
        return Err("Update rejected: checksum mismatch".into());
    }
    let key_bytes: [u8; 32] = b64.decode(PUBLIC_KEY).ok().and_then(|k| k.try_into().ok()).ok_or("bad public key")?;
    let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| "bad public key")?;
    let sig_bytes: [u8; 64] =
        b64.decode(asset.signature.trim()).ok().and_then(|s| s.try_into().ok()).ok_or("Update rejected: missing signature")?;
    key.verify(data, &Signature::from_bytes(&sig_bytes)).map_err(|_| "Update rejected: invalid signature".to_string())
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_file_name("streamtubah.old.exe")
}

/// Downloads, verifies and swaps in the new exe. The app keeps running; restart to use it.
pub fn install() -> Result<String, String> {
    let info = check()?;
    if !info.available {
        return Err("Already up to date".into());
    }
    if info.needs_full_package {
        return Err(format!("Version {} changes the OBS runtime; download the full package: {}", info.version, info.full_url));
    }
    let m = info.manifest.ok_or("no manifest")?;
    let data = download(&m.app.url)?;
    verify(&data, &m.app)?;

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("Bad update archive: {e}"))?;
    let mut entry = zip.by_name("streamtubah.exe").map_err(|_| "Update archive has no streamtubah.exe")?;
    let mut exe_bytes = Vec::new();
    entry.read_to_end(&mut exe_bytes).map_err(|e| e.to_string())?;

    let exe = exe_path()?;
    let new = exe.with_file_name("streamtubah.new.exe");
    std::fs::write(&new, &exe_bytes).map_err(|e| format!("Cannot write next to the app ({}): {e}", exe.display()))?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Cannot replace the running app: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        // Put the original back so the app still starts.
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Cannot install the update: {e}"));
    }
    Ok(m.version)
}

/// Deletes the previous exe left by an update (it was in use until the restart).
pub fn cleanup() {
    if let Ok(exe) = exe_path() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(exe.with_file_name("streamtubah.new.exe"));
    }
}

/// Starts the (updated) exe and lets the caller exit this instance.
pub fn relaunch() -> Result<(), String> {
    let exe = exe_path()?;
    std::process::Command::new(&exe)
        .args(["--after-update", &std::process::id().to_string()])
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not restart: {e}"))
}

/// Started by `relaunch`: wait (up to 15 s) for the previous instance to exit so it releases
/// the API port and finishes saving before this one loads settings.
pub fn wait_for_previous_instance() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    let args: Vec<String> = std::env::args().collect();
    let Some(pid) = args.iter().position(|a| a == "--after-update").and_then(|i| args.get(i + 1)?.parse::<u32>().ok()) else {
        return;
    };
    if let Ok(h) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        unsafe {
            WaitForSingleObject(h, 15_000);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn rejects_bad_checksum_and_signature() {
        let data = b"not an update";
        let mut a = Asset { url: String::new(), sha256: "00".into(), signature: String::new(), size: 0 };
        assert!(verify(data, &a).unwrap_err().contains("checksum"));
        a.sha256 = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify(data, &a).unwrap_err().contains("signature"));
        a.signature = base64::engine::general_purpose::STANDARD.encode([7u8; 64]);
        assert!(verify(data, &a).unwrap_err().contains("invalid signature"));
    }
}
