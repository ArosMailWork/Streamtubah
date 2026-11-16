//! Browser component (CEF + obs-browser) downloaded on first use.
//!
//! The portable package ships without Chromium (~230 MB of the 334 MB). Browser Sources (Alert Box,
//! chat overlay, widgets, any web page) need it: the first time one is added, the app offers to
//! download `Streamtubah-cef-<OBS>-win64.zip` from the GitHub release `runtime-cef-<OBS>`, checks its
//! SHA-256 and Ed25519 signature (same key as app updates), extracts it to
//! `<app data>/components/cef-<OBS>/obs-browser/{bin/64bit,data}` and asks for a restart (libobs
//! loads plugins only at startup). That folder is an extra OBS plugin root. Builds that still carry
//! obs-browser next to the exe (dev builds, older packages) use their own copy.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::updater::OBS_RUNTIME;

/// Error text the UI recognises to open the download dialog.
pub const MISSING: &str = "BROWSER_COMPONENT_MISSING: Browser Sources need the browser component (Chromium). Download it from the dialog.";
const MAX_DOWNLOAD: u64 = 400 * 1024 * 1024;

fn base_url() -> String {
    std::env::var("STREAMTUBAH_CEF_URL")
        .unwrap_or_else(|_| format!("https://github.com/ArosMailWork/Streamtubah/releases/download/runtime-cef-{OBS_RUNTIME}"))
}

pub fn manifest_name() -> String {
    format!("cef-{OBS_RUNTIME}.json")
}

#[derive(Deserialize)]
struct Manifest {
    obs_runtime: String,
    #[serde(flatten)]
    asset: crate::updater::Asset,
}

struct Ctx {
    app: AppHandle,
    data_dir: PathBuf,
    bundled: bool,
}

static CTX: OnceLock<Ctx> = OnceLock::new();
static STATE: Mutex<String> = Mutex::new(String::new());
static BUSY: AtomicBool = AtomicBool::new(false);
/// Installed during this run (a restart loads it).
static PENDING_RESTART: AtomicBool = AtomicBool::new(false);

fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("components").join(format!("cef-{OBS_RUNTIME}"))
}

fn bundled_in(bin_dir: &Path) -> bool {
    bin_dir.join("obs-plugins/64bit/obs-browser.dll").is_file()
}

fn installed_in(data_dir: &Path) -> bool {
    root(data_dir).join("obs-browser/bin/64bit/.complete").is_file()
}

/// Extra plugin root for libobs, if the downloaded component is present and needed.
/// Also removes components of other OBS versions.
pub fn plugin_root(data_dir: &Path, bin_dir: &Path) -> Option<PathBuf> {
    let keep = root(data_dir);
    if let Ok(entries) = std::fs::read_dir(data_dir.join("components")) {
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p != keep && name.starts_with("cef-") {
                let _ = std::fs::remove_dir_all(p);
            }
        }
    }
    (!bundled_in(bin_dir) && installed_in(data_dir)).then_some(keep)
}

pub fn init(app: &AppHandle, data_dir: PathBuf) {
    let bundled = std::env::current_exe().ok().and_then(|e| e.parent().map(bundled_in)).unwrap_or(false);
    let _ = CTX.set(Ctx { app: app.clone(), data_dir, bundled });
}

/// Browser Source type registered (CEF loaded at startup).
pub fn loaded() -> bool {
    unsafe { crate::obs::sys::obs_get_source_output_flags(c"browser_source".as_ptr()) != 0 }
}

/// Refuses Browser Sources while the component is not loaded (libobs would create a dead
/// placeholder source instead of failing).
pub fn require(type_id: &str) -> Result<(), String> {
    if type_id == "browser_source" && !loaded() { Err(MISSING.into()) } else { Ok(()) }
}

pub fn status() -> Value {
    let (installed, bundled) = CTX.get().map_or((false, false), |c| (installed_in(&c.data_dir), c.bundled));
    json!({
        "loaded": loaded(),
        "bundled": bundled,
        "installed": installed,
        "busy": BUSY.load(Ordering::SeqCst),
        "restart_needed": PENDING_RESTART.load(Ordering::SeqCst) && !loaded(),
        "state": STATE.lock().unwrap().clone(),
        "obs_runtime": OBS_RUNTIME,
    })
}

fn set_state(text: impl Into<String>) {
    *STATE.lock().unwrap() = text.into();
    if let Some(c) = CTX.get() {
        let _ = c.app.emit("cef:status", status());
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(15))).build().into()
}

fn fetch_manifest(local_zip: Option<&Path>) -> Result<Manifest, String> {
    // Offline install: the manifest published next to the zip.
    let local = local_zip.and_then(|z| z.parent()).map(|d| d.join(manifest_name())).filter(|p| p.is_file());
    let text = match local {
        Some(p) => std::fs::read_to_string(p).map_err(|e| e.to_string())?,
        None => agent()
            .get(&format!("{}/{}", base_url(), manifest_name()))
            .call()
            .map_err(|e| format!("Cannot reach the download page: {e}"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?,
    };
    let m: Manifest = serde_json::from_str(&text).map_err(|e| format!("Bad component manifest: {e}"))?;
    if m.obs_runtime != OBS_RUNTIME {
        return Err(format!("This browser component is for OBS {} (this app needs {OBS_RUNTIME})", m.obs_runtime));
    }
    Ok(m)
}

fn download(m: &Manifest) -> Result<Vec<u8>, String> {
    let resp = agent().get(&m.asset.url).call().map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = resp.into_body().into_reader().take(MAX_DOWNLOAD + 1);
    let total = m.asset.size.max(1);
    let mut data = Vec::with_capacity(m.asset.size.min(MAX_DOWNLOAD) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    let mut last = u64::MAX;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let pct = (data.len() as u64 * 100 / total).min(100);
        if pct != last {
            last = pct;
            set_state(format!("downloading {pct}% ({} / {} MB)", data.len() / 1_048_576, total / 1_048_576));
        }
    }
    if data.len() as u64 > MAX_DOWNLOAD {
        return Err("Browser component file is unexpectedly large".into());
    }
    Ok(data)
}

/// Extracts into `<root>.tmp`, then swaps it in. Entries outside `obs-browser/` are refused.
fn extract(data: &[u8], data_dir: &Path) -> Result<(), String> {
    let target = root(data_dir);
    let tmp = target.with_file_name(format!("cef-{OBS_RUNTIME}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("Bad archive: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = f.enclosed_name() else { return Err("Archive contains an unsafe path".into()) };
        if !rel.starts_with("obs-browser") {
            return Err("Unexpected file in archive".into());
        }
        let out = tmp.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut w = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut w).map_err(|e| e.to_string())?;
    }
    let bin = tmp.join("obs-browser/bin/64bit");
    if !bin.join("obs-browser.dll").is_file() || !bin.join("libcef.dll").is_file() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err("Archive is missing obs-browser.dll or libcef.dll".into());
    }
    std::fs::write(bin.join(".complete"), OBS_RUNTIME).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&tmp, &target).map_err(|e| format!("Cannot install: {e}"))
}

/// Downloads (or reads `local_zip`), verifies and installs. Runs on the caller's thread.
pub fn install(local_zip: Option<PathBuf>) -> Result<(), String> {
    let ctx = CTX.get().ok_or("not initialised")?;
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("Already installing".into());
    }
    let result = (|| {
        set_state("checking…");
        let m = fetch_manifest(local_zip.as_deref())?;
        let data = match &local_zip {
            Some(p) => std::fs::read(p).map_err(|e| e.to_string())?,
            None => download(&m)?,
        };
        set_state("verifying…");
        crate::updater::verify(&data, &m.asset).map_err(|e| e.replace("Update rejected", "Browser component rejected"))?;
        set_state("installing…");
        extract(&data, &ctx.data_dir)
    })();
    BUSY.store(false, Ordering::SeqCst);
    match &result {
        Ok(()) => {
            PENDING_RESTART.store(true, Ordering::SeqCst);
            set_state("installed: restart Streamtubah to use it");
        }
        Err(e) => set_state(format!("error: {e}")),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, data) in files {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extract_checks_layout() {
        let dir = std::env::temp_dir().join(format!("st-cef-{}", crate::settings::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let good = zip_with(&[
            ("obs-browser/bin/64bit/obs-browser.dll", b"x"),
            ("obs-browser/bin/64bit/libcef.dll", b"x"),
            ("obs-browser/data/locale/en-US.ini", b"x"),
        ]);
        extract(&good, &dir).unwrap();
        assert!(installed_in(&dir));
        assert_eq!(plugin_root(&dir, &dir), Some(root(&dir)));
        assert!(extract(&zip_with(&[("../evil.dll", b"x")]), &dir).is_err());
        assert!(extract(&zip_with(&[("other/x.dll", b"x")]), &dir).is_err());
        assert!(extract(&zip_with(&[("obs-browser/bin/64bit/obs-browser.dll", b"x")]), &dir).is_err());
        // A previous good install survives a failed one.
        assert!(installed_in(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }
}
