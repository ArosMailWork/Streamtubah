//! "OBS Virtual Camera" DirectShow device. libobs' `virtualcam_output` only feeds shared
//! memory; apps see the camera once the DirectShow filter DLLs are COM-registered, which
//! needs admin rights once (a UAC prompt). OBS Studio's registration works too.

use std::path::Path;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RegCloseKey, RegOpenKeyExW};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

use crate::commands::AppState;

const CLSID_KEY: &str = r"SOFTWARE\Classes\CLSID\{A3FCE0F5-3493-419F-958A-ABA1250EC20B}";

fn key_exists(view: windows::Win32::System::Registry::REG_SAM_FLAGS) -> bool {
    let mut key = HKEY::default();
    let ok = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(CLSID_KEY), None, KEY_READ | view, &mut key) }.is_ok();
    if ok {
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    ok
}

/// Both 64-bit and 32-bit filters registered (32-bit apps such as some meeting clients use the latter).
pub fn installed() -> bool {
    key_exists(KEY_WOW64_64KEY) && key_exists(KEY_WOW64_32KEY)
}

/// Registers both filter DLLs with one elevated `cmd` (one UAC prompt). Blocks until done.
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe dir")?.join("data/obs-plugins/win-dshow");
    let dll = |bits: &str| -> Result<String, String> {
        let p = dir.join(format!("obs-virtualcam-module{bits}.dll"));
        if !Path::new(&p).exists() {
            return Err(format!("Missing {}", p.display()));
        }
        Ok(p.to_string_lossy().replace('/', "\\"))
    };
    let (d64, d32) = (dll("64")?, dll("32")?);
    // `/s /c "..."`: cmd strips exactly the outer quotes, so the inner quoted paths survive.
    let params = HSTRING::from(format!(r#"/s /c "regsvr32 /i /s "{d64}" & regsvr32 /i /s "{d32}"""#));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: w!("cmd.exe"),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|_| "Administrator permission was not granted".to_string())?;
    if !info.hProcess.is_invalid() {
        unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
    if installed() { Ok(()) } else { Err("Virtual camera registration failed".into()) }
}

/// Start/stop from the UI or a hotkey. Runs on its own thread because the first start may
/// wait on a UAC prompt.
pub fn toggle(app: AppHandle) {
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let state = app.state::<AppState>();
            if state.0.lock().map_err(|_| "Studio state poisoned")?.outputs.is_running("virtualcam") {
                state.0.lock().map_err(|_| "Studio state poisoned")?.outputs.stop("virtualcam");
                return Ok(());
            }
            if !installed() {
                install()?;
            }
            state.0.lock().map_err(|_| "Studio state poisoned")?.start_output("virtualcam")
        })();
        if let Err(e) = result {
            let _ = app.emit("app:error", json!(e));
        }
        let _ = app.emit("studio:changed", serde_json::Value::Null);
    });
}
