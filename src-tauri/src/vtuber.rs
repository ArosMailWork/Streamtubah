//! VTuber mode: keeps the real camera (your face) off the stream and recordings.
//!
//! While it is on:
//! - every webcam source (Video Capture Device) is always covered;
//! - Windows' own camera log (the registry behind the taskbar camera icon) is read every second;
//!   when an app outside the allow list (face-tracking apps) uses the camera, the user is warned and
//!   Window/Game Capture sources showing that app are covered, and so is every Display Capture
//!   (the face could be anywhere on screen). Browser Sources are covered when a browser page of
//!   this app uses the camera.
//! Other apps are never closed or blocked (user decision); only Streamtubah's output is protected.
//!
//! Covering = a hidden video filter (`streamtubah_vtuber_cover`) that draws a background plus the
//! cover image (default: an animated character, or an image the user imported) instead of the
//! source. Preview, stream, recording and virtual camera all see the cover.

use std::collections::BTreeSet;
use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::obs::{Data, Source, cstr, from_cstr, sys};

pub const COVER_ID: &str = "streamtubah_vtuber_cover";
const DEFAULT_COVER: &[u8] = include_bytes!("../assets/vtuber-cover.gif");
/// Background behind the cover image (matches the default GIF).
const BG: [f32; 4] = [30.0 / 255.0, 33.0 / 255.0, 48.0 / 255.0, 1.0];

/// Face-tracking apps that may use the camera (they show an avatar, not the face).
/// Matched case-insensitively against the app's file name.
pub const DEFAULT_ALLOWED: [&str; 11] = [
    "vtube studio", "vtubestudio", "vseeface", "warudo", "vnyan", "animaze", "3tene", "xr animator",
    "webcam motion capture", "webcammotioncapture", "pngtuber",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct VtuberSettings {
    pub enabled: bool,
    pub allowed_apps: Vec<String>,
    /// Imported image in alert-media ("" = default character).
    pub cover_image: String,
}

impl Default for VtuberSettings {
    fn default() -> Self {
        Self { enabled: false, allowed_apps: DEFAULT_ALLOWED.iter().map(|s| s.to_string()).collect(), cover_image: String::new() }
    }
}

/// An app currently using the camera.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CameraUser {
    /// Full path (desktop apps) or package name (Store apps).
    pub app: String,
    /// File name / package name shown to the user.
    pub name: String,
    pub allowed: bool,
}

#[derive(Default)]
struct State {
    cfg: VtuberSettings,
    users: Vec<CameraUser>,
    /// Names already warned about (warn again only after they stopped using the camera).
    warned: BTreeSet<String>,
    cover: Option<Source>,
    cover_file: PathBuf,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static APP: OnceLock<(AppHandle, PathBuf)> = OnceLock::new();
/// True while gates exist on sources (so turning the mode off removes them once).
static ATTACHED: AtomicBool = AtomicBool::new(false);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

// ---------------------------------------------------------------------------
// Camera users from the registry (HKCU ...\CapabilityAccessManager\ConsentStore\webcam)

const WEBCAM_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam";

fn subkeys(path: &str) -> Vec<String> {
    use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW};
    use windows::core::{HSTRING, PWSTR};
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, KEY_READ, &mut key) }.is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut buf = [0u16; 512];
    for i in 0.. {
        let mut len = buf.len() as u32;
        if unsafe { RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) }.is_err() {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    out
}

fn qword(path: &str, value: &str) -> Option<u64> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_QWORD, RegGetValueW};
    use windows::core::HSTRING;
    let mut v: u64 = 0;
    let mut len = 8u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            &HSTRING::from(value),
            RRF_RT_REG_QWORD,
            None,
            Some(&mut v as *mut u64 as *mut c_void),
            Some(&mut len),
        )
    }
    .is_ok()
    .then_some(v)
}

/// In use = started and not stopped since (Windows writes LastUsedTimeStop = 0 while active).
fn in_use(path: &str) -> bool {
    qword(path, "LastUsedTimeStart").unwrap_or(0) != 0 && qword(path, "LastUsedTimeStop") == Some(0)
}

/// Apps using the camera right now: (app id, display name).
fn camera_users() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let desktop = format!(r"{WEBCAM_KEY}\NonPackaged");
    for k in subkeys(&desktop) {
        if in_use(&format!(r"{desktop}\{k}")) {
            // Key names are paths with '#' instead of '\'.
            let app = k.replace('#', r"\");
            let name = app.rsplit('\\').next().unwrap_or(&app).to_string();
            out.push((app, name));
        }
    }
    for k in subkeys(WEBCAM_KEY) {
        if k != "NonPackaged" && in_use(&format!(r"{WEBCAM_KEY}\{k}")) {
            let name = k.split('_').next().unwrap_or(&k).to_string();
            out.push((k, name));
        }
    }
    out
}

pub fn is_allowed(name: &str, allowed: &[String]) -> bool {
    let n = name.to_lowercase();
    allowed.iter().map(|a| a.trim().to_lowercase()).any(|a| !a.is_empty() && n.contains(&a))
}

// ---------------------------------------------------------------------------
// Cover filter

struct Cover {
    ctx: *mut sys::obs_source_t,
    covered: AtomicBool,
}

unsafe extern "C" fn cover_name(_: *mut c_void) -> *const c_char {
    c"VTuber mode cover".as_ptr()
}

unsafe extern "C" fn cover_create(_: *mut sys::obs_data_t, source: *mut sys::obs_source_t) -> *mut c_void {
    // Covered from the first frame; the next refresh opens it if allowed.
    Box::into_raw(Box::new(Cover { ctx: source, covered: AtomicBool::new(true) })) as *mut c_void
}

unsafe extern "C" fn cover_destroy(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data as *mut Cover) });
}

unsafe extern "C" fn cover_render(data: *mut c_void, _effect: *mut sys::gs_effect_t) {
    let c = unsafe { &*(data as *const Cover) };
    if !c.covered.load(Ordering::Relaxed) {
        unsafe { sys::obs_source_skip_video_filter(c.ctx) };
        return;
    }
    unsafe {
        let target = sys::obs_filter_get_target(c.ctx);
        let (w, h) = (sys::obs_source_get_base_width(target) as f32, sys::obs_source_get_base_height(target) as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let solid = sys::obs_get_base_effect(sys::obs_base_effect_OBS_EFFECT_SOLID);
        let param = sys::gs_effect_get_param_by_name(solid, c"color".as_ptr());
        let mut color: sys::vec4 = std::mem::zeroed();
        *(&mut color as *mut sys::vec4 as *mut [f32; 4]) = BG;
        sys::gs_effect_set_vec4(param, &color);
        while sys::gs_effect_loop(solid, c"Solid".as_ptr()) {
            sys::gs_draw_sprite(std::ptr::null_mut(), 0, w as u32, h as u32);
        }
        // The cover image, fitted into 80 % of the source and centred. try_lock: never block
        // the graphics thread on the settings thread.
        let Ok(guard) = STATE.try_lock() else { return };
        let Some(img) = guard.as_ref().and_then(|s| s.cover.as_ref()) else { return };
        let (iw, ih) = (sys::obs_source_get_width(img.as_ptr()) as f32, sys::obs_source_get_height(img.as_ptr()) as f32);
        if iw > 0.0 && ih > 0.0 {
            let scale = (w * 0.8 / iw).min(h * 0.8 / ih);
            sys::gs_matrix_push();
            sys::gs_matrix_translate3f((w - iw * scale) / 2.0, (h - ih * scale) / 2.0, 0.0);
            sys::gs_matrix_scale3f(scale, scale, 1.0);
            sys::obs_source_video_render(img.as_ptr());
            sys::gs_matrix_pop();
        }
    }
}

pub fn register() {
    let id = Box::leak(Box::new(cstr(COVER_ID)));
    unsafe {
        let mut info: sys::obs_source_info = std::mem::zeroed();
        info.id = id.as_ptr();
        info.type_ = sys::obs_source_type_OBS_SOURCE_TYPE_FILTER;
        info.output_flags = sys::OBS_SOURCE_VIDEO | sys::OBS_SOURCE_CAP_DISABLED;
        info.get_name = Some(cover_name);
        info.create = Some(cover_create);
        info.destroy = Some(cover_destroy);
        info.video_render = Some(cover_render);
        sys::obs_register_source_s(&info, std::mem::size_of::<sys::obs_source_info>());
    }
}

// ---------------------------------------------------------------------------
// Which sources to cover

/// Executable shown by a Window / Game Capture source ("title:class:exe", ':' escaped as "#3A").
fn captured_exe(settings: &Value) -> Option<String> {
    let window = settings["window"].as_str()?;
    let exe = window.rsplit(':').next()?.replace("#3A", ":").replace("#22", "#");
    (!exe.is_empty()).then(|| exe.to_lowercase())
}

/// Whether a source must be covered, given the names of disallowed apps using the camera.
pub fn should_cover(type_id: &str, settings: &Value, offenders: &[String]) -> Option<bool> {
    let any = !offenders.is_empty();
    Some(match type_id {
        "dshow_input" => true,
        "monitor_capture" => any,
        "window_capture" | "game_capture" => captured_exe(settings).is_some_and(|exe| offenders.iter().any(|o| o.to_lowercase() == exe)),
        "browser_source" => offenders.iter().any(|o| o.eq_ignore_ascii_case("obs-browser-page.exe")),
        _ => return None,
    })
}

fn cover_filters(src: *mut sys::obs_source_t) -> Vec<*mut sys::obs_source_t> {
    unsafe extern "C" fn cb(_p: *mut sys::obs_source_t, f: *mut sys::obs_source_t, data: *mut c_void) {
        if from_cstr(unsafe { sys::obs_source_get_id(f) }) == COVER_ID {
            unsafe { (*(data as *mut Vec<*mut sys::obs_source_t>)).push(f) };
        }
    }
    let mut out: Vec<*mut sys::obs_source_t> = Vec::new();
    unsafe { sys::obs_source_enum_filters(src, Some(cb), &mut out as *mut _ as *mut c_void) };
    out
}

fn all_sources() -> Vec<Source> {
    unsafe extern "C" fn cb(data: *mut c_void, src: *mut sys::obs_source_t) -> bool {
        if let Some(s) = Source::from_raw(unsafe { sys::obs_source_get_ref(src) }) {
            unsafe { (*(data as *mut Vec<Source>)).push(s) };
        }
        true
    }
    let mut out: Vec<Source> = Vec::new();
    unsafe { sys::obs_enum_sources(Some(cb), &mut out as *mut _ as *mut c_void) };
    out
}

/// Adds/updates/removes cover filters on every source. Cheap; called after any studio change
/// and every second.
pub fn refresh_sources() {
    let (enabled, offenders) = with(|s| (s.cfg.enabled, offenders(&s.users)));
    if !enabled && !ATTACHED.load(Ordering::Relaxed) {
        return;
    }
    for src in all_sources() {
        let existing = cover_filters(src.as_ptr());
        let cover = if enabled { should_cover(&src.id(), &src.settings().to_json(), &offenders) } else { None };
        // A copied filter chain ("Paste Filters") can bring a second cover: keep one.
        for &extra in existing.iter().skip(1) {
            unsafe { sys::obs_source_filter_remove(src.as_ptr(), extra) };
        }
        match cover {
            None => {
                for f in existing {
                    unsafe { sys::obs_source_filter_remove(src.as_ptr(), f) };
                }
            }
            Some(on) => {
                let filter = match existing.first() {
                    Some(&f) => f,
                    None => {
                        let Some(f) = Source::create_private(COVER_ID, "VTuber mode", None) else { continue };
                        unsafe {
                            sys::obs_source_filter_add(src.as_ptr(), f.as_ptr());
                            // Last in the chain: nothing after it can bring the picture back.
                            sys::obs_source_filter_set_order(src.as_ptr(), f.as_ptr(), sys::obs_order_movement_OBS_ORDER_MOVE_TOP);
                        }
                        f.as_ptr()
                    }
                };
                let data = unsafe { sys::obs_obj_get_data(filter as *mut c_void) } as *const Cover;
                if !data.is_null() {
                    unsafe { (*data).covered.store(on, Ordering::Relaxed) };
                }
            }
        }
    }
    ATTACHED.store(enabled, Ordering::Relaxed);
}

fn offenders(users: &[CameraUser]) -> Vec<String> {
    users.iter().filter(|u| !u.allowed).map(|u| u.name.clone()).collect()
}

/// Polls the camera users (every second, with the Studio lock held by the caller).
pub fn tick() {
    let (enabled, allowed) = with(|s| (s.cfg.enabled, s.cfg.allowed_apps.clone()));
    if !enabled {
        if ATTACHED.load(Ordering::Relaxed) {
            refresh_sources();
        }
        return;
    }
    let own = std::env::current_exe().ok().map(|p| p.to_string_lossy().to_lowercase());
    let mut users: Vec<CameraUser> = camera_users()
        .into_iter()
        // Streamtubah's own webcam sources are always covered.
        .filter(|(app, _)| Some(app.to_lowercase()) != own)
        .map(|(app, name)| CameraUser { allowed: is_allowed(&name, &allowed), app, name })
        .collect();
    users.sort();
    let (changed, new_offenders) = with(|s| {
        let changed = s.users != users;
        let now: BTreeSet<String> = offenders(&users).into_iter().collect();
        let fresh: Vec<String> = now.difference(&s.warned).cloned().collect();
        s.warned = now;
        s.users = users;
        (changed, fresh)
    });
    if changed {
        refresh_sources();
        notify();
    }
    if let Some((app, _)) = APP.get() {
        for name in new_offenders {
            let _ = app.emit("app:error", format!("VTuber mode: {name} is using your camera. Display / window captures that could show it are covered."));
        }
    }
}

pub fn status() -> Value {
    with(|s| json!({ "enabled": s.cfg.enabled, "users": s.users, "covering": s.cfg.enabled }))
}

fn notify() {
    if let Some((app, _)) = APP.get() {
        let _ = app.emit("vtuber:status", status());
    }
}

// ---------------------------------------------------------------------------
// Settings

pub fn init(app: &AppHandle, data_dir: &std::path::Path, cfg: &VtuberSettings) {
    let file = data_dir.join("vtuber-cover-default.gif");
    if std::fs::read(&file).ok().as_deref() != Some(DEFAULT_COVER) {
        let _ = std::fs::write(&file, DEFAULT_COVER);
    }
    let _ = APP.set((app.clone(), file));
    configure(cfg);
}

/// Applies new settings (call with the Studio lock held).
pub fn configure(cfg: &VtuberSettings) {
    let image = if cfg.cover_image.is_empty() {
        APP.get().map(|(_, f)| f.clone())
    } else {
        crate::alerts::media_path(&cfg.cover_image)
    };
    let old = with(|s| {
        s.cfg = cfg.clone();
        if !cfg.enabled {
            s.users.clear();
            s.warned.clear();
        }
        let want = if cfg.enabled { image.clone() } else { None };
        if want.as_ref() == Some(&s.cover_file) && s.cover.is_some() {
            return None;
        }
        s.cover_file = want.clone().unwrap_or_default();
        let new = want.and_then(|path| {
            let d = Data::new();
            d.set_string("file", &path.to_string_lossy());
            let src = Source::create_private("image_source", "VTuber cover", Some(&d))?;
            // GIFs only animate while "showing".
            unsafe { sys::obs_source_inc_showing(src.as_ptr()) };
            Some(src)
        });
        std::mem::replace(&mut s.cover, new)
    });
    if let Some(old) = old {
        unsafe { sys::obs_source_dec_showing(old.as_ptr()) };
    }
    refresh_sources();
    notify();
}

/// Releases the cover image (app exit, before obs_shutdown).
pub fn shutdown() {
    if let Some(old) = with(|s| s.cover.take()) {
        unsafe { sys::obs_source_dec_showing(old.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_matching() {
        let allowed: Vec<String> = DEFAULT_ALLOWED.iter().map(|s| s.to_string()).collect();
        assert!(is_allowed("VTube Studio.exe", &allowed));
        assert!(is_allowed("VSeeFace.exe", &allowed));
        assert!(!is_allowed("Discord.exe", &allowed));
        assert!(!is_allowed("anything.exe", &["  ".to_string()]));
    }

    #[test]
    fn cover_rules() {
        let none: Vec<String> = vec![];
        let discord = vec!["Discord.exe".to_string()];
        assert_eq!(should_cover("dshow_input", &json!({}), &none), Some(true));
        assert_eq!(should_cover("monitor_capture", &json!({}), &none), Some(false));
        assert_eq!(should_cover("monitor_capture", &json!({}), &discord), Some(true));
        let win = json!({ "window": "Discord#3A Voice:Chrome_WidgetWin_1:Discord.exe" });
        assert_eq!(should_cover("window_capture", &win, &discord), Some(true));
        assert_eq!(should_cover("window_capture", &win, &none), Some(false));
        let other = json!({ "window": "Game:UnityWndClass:game.exe" });
        assert_eq!(should_cover("window_capture", &other, &discord), Some(false));
        assert_eq!(should_cover("browser_source", &json!({}), &["obs-browser-page.exe".into()]), Some(true));
        assert_eq!(should_cover("image_source", &json!({}), &discord), None);
    }
}
