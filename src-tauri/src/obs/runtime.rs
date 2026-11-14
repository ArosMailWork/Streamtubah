//! libobs process lifecycle: startup, logging, module loading, video/audio reset, shutdown.

use std::ffi::{c_char, c_int, c_void};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::{cstr, from_cstr, sys};

static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

unsafe extern "C" fn log_handler(lvl: c_int, msg: *const c_char, args: sys::va_list, _p: *mut c_void) {
    let text = unsafe { vsprintf::vsprintf(msg, args as *mut c_void) }.unwrap_or_else(|_| from_cstr(msg));
    let level = match lvl {
        sys::LOG_ERROR => "error",
        sys::LOG_WARNING => "warn",
        sys::LOG_INFO => "info",
        _ => "debug",
    };
    if level == "debug" && !cfg!(debug_assertions) {
        return;
    }
    let line = format!("[{level}] {text}\n");
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    if let Some(f) = LOG_FILE.get() {
        if let Ok(mut f) = f.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Canvas configuration applied through `obs_reset_video`.
#[derive(Clone, Debug)]
pub struct VideoConfig {
    pub base_width: u32,
    pub base_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub scale_type: sys::obs_scale_type,
}

/// Directory layout of the OBS runtime that sits next to our executable.
pub struct RuntimePaths {
    pub bin_dir: PathBuf,
    pub config_dir: PathBuf,
    /// Third-party plugin roots using the OBS layout `<root>/<name>/bin/64bit/<name>.dll`
    /// and `<root>/<name>/data/`.
    pub plugin_roots: Vec<PathBuf>,
}

pub fn startup(paths: &RuntimePaths, log_path: &Path) -> Result<String, String> {
    if let Ok(f) = std::fs::File::create(log_path) {
        let _ = LOG_FILE.set(Mutex::new(f));
    }
    unsafe {
        sys::base_set_log_handler(Some(log_handler), std::ptr::null_mut());
        if sys::obs_initialized() {
            return Err("libobs already initialized".into());
        }
        let locale = cstr("en-US");
        let cfg = cstr(&path_str(&paths.config_dir));
        if !sys::obs_startup(locale.as_ptr(), cfg.as_ptr(), std::ptr::null_mut()) {
            return Err("obs_startup failed".into());
        }

        let data = cstr(&path_str(&paths.bin_dir.join("data/libobs/")));
        sys::obs_add_data_path(data.as_ptr());
        let mod_bin = cstr(&path_str(&paths.bin_dir.join("obs-plugins/64bit/")));
        let mod_data = cstr(&path_str(&paths.bin_dir.join("data/obs-plugins/%module%/")));
        sys::obs_add_module_path(mod_bin.as_ptr(), mod_data.as_ptr());
        for root in &paths.plugin_roots {
            let bin = cstr(&format!("{}/%module%/bin/64bit/", path_str(root)));
            let data = cstr(&format!("{}/%module%/data/", path_str(root)));
            sys::obs_add_module_path(bin.as_ptr(), data.as_ptr());
        }

        Ok(from_cstr(sys::obs_get_version_string()))
    }
}

/// Audio is fixed at 48 kHz stereo; libobs cannot change it after startup.
pub fn reset_audio() -> Result<(), String> {
    let info = sys::obs_audio_info2 {
        samples_per_sec: 48000,
        speakers: sys::speaker_layout_SPEAKERS_STEREO,
        max_buffering_ms: 0,
        fixed_buffering: false,
    };
    unsafe { sys::obs_reset_audio2(&info) }.then_some(()).ok_or("obs_reset_audio2 failed".into())
}

pub fn reset_video(cfg: &VideoConfig) -> Result<(), String> {
    let module = cstr("libobs-d3d11");
    let mut ovi = sys::obs_video_info {
        graphics_module: module.as_ptr(),
        fps_num: cfg.fps_num,
        fps_den: cfg.fps_den.max(1),
        base_width: cfg.base_width,
        base_height: cfg.base_height,
        output_width: cfg.output_width,
        output_height: cfg.output_height,
        output_format: sys::video_format_VIDEO_FORMAT_NV12,
        adapter: 0,
        gpu_conversion: true,
        colorspace: sys::video_colorspace_VIDEO_CS_709,
        range: sys::video_range_type_VIDEO_RANGE_PARTIAL,
        scale_type: cfg.scale_type,
    };
    match unsafe { sys::obs_reset_video(&mut ovi) } {
        0 => Ok(()),
        -4 => Err("Cannot change video settings while streaming or recording".into()),
        code => Err(format!("obs_reset_video failed (code {code})")),
    }
}

pub fn load_modules() {
    unsafe {
        let mut failures: sys::obs_module_failure_info = std::mem::zeroed();
        sys::obs_load_all_modules2(&mut failures);
        sys::obs_log_loaded_modules();
        sys::obs_post_load_modules();
        sys::obs_module_failure_info_free(&mut failures);
    }
}

/// Base canvas size currently in effect.
pub fn canvas_size() -> (u32, u32) {
    unsafe {
        let mut ovi: sys::obs_video_info = std::mem::zeroed();
        if sys::obs_get_video_info(&mut ovi) {
            (ovi.base_width, ovi.base_height)
        } else {
            (1920, 1080)
        }
    }
}

pub fn shutdown() {
    unsafe {
        for ch in 0..64 {
            sys::obs_set_output_source(ch, std::ptr::null_mut());
        }
        sys::obs_shutdown();
    }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
