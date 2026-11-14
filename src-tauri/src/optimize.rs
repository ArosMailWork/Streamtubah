//! Auto-Optimize: measures this PC and proposes encoder, bitrate, output resolution and FPS.
//!
//! 1. Upload speed: throw-away data posted to Cloudflare's public speed test (no account).
//! 2. Hardware: encoders, CPU name/threads, GPU name (also the "same machine?" fingerprint).
//! 3. Real encode test: from the best candidate down, encode a few seconds at each
//!    resolution/FPS with the chosen encoder preset and keep the first one that drops no frames.
//!
//! A 5-level preference (1 = smoother PC … 5 = best stream) bounds resolution, FPS, bitrate and
//! encoder preset. Level 5 may reach 1440p ("2K") when canvas, platform and upload allow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::obs::runtime::{self, VideoConfig};
use crate::obs::{Data, Encoder, Output, cstr, sys};
use crate::studio::Studio;
use crate::studio::outputs::{self, EncoderInfo};

const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
/// Encode test length per candidate (after a 1 s warm-up).
const TRIAL: Duration = Duration::from_secs(3);
/// A candidate passes when at most this share of frames is skipped (encoder) or lagged (render).
const MAX_DROP_PCT: f64 = 0.5;

#[derive(Serialize, Clone)]
pub struct Measurement {
    pub upload_mbps: Option<f64>,
    pub upload_error: Option<String>,
    pub cpu_name: String,
    pub cpu_threads: usize,
    pub gpu_name: String,
    pub encoders: Vec<EncoderInfo>,
    pub canvas: (u32, u32),
    pub service: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Trial {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub passed: bool,
    pub skipped_pct: f64,
    pub lagged_pct: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Recommendation {
    pub level: u8,
    pub encoder_id: String,
    pub encoder_name: String,
    pub hardware: bool,
    pub bitrate_kbps: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Merged into the profile's encoder settings on apply (rate control, bitrate, preset…).
    pub encoder_settings: Value,
    pub reasons: Vec<String>,
    pub trials: Vec<Trial>,
}

// ---------------------------------------------------------------------------
// Hardware

fn cpu_name() -> String {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            w!("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    }
    .is_ok();
    if !ok {
        return "Unknown CPU".into();
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n]).trim().to_string()
}

/// First hardware (non-software) graphics adapter.
fn gpu_name() -> String {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return "Unknown GPU".into() };
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let n = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            return String::from_utf16_lossy(&desc.Description[..n]);
        }
    }
    "Unknown GPU".into()
}

/// Identifies this PC: a different value means the app now runs on other hardware.
pub fn machine_id() -> String {
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!("{} | {} threads | {}", cpu_name(), threads, gpu_name())
}

fn upload_mbps() -> Result<f64, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let post = |bytes: usize| -> Result<f64, String> {
        let body = vec![0u8; bytes];
        let t = Instant::now();
        agent.post(UPLOAD_URL).send(&body[..]).map_err(|e| format!("Upload test failed: {e}"))?;
        Ok(bytes as f64 * 8.0 / t.elapsed().as_secs_f64().max(0.001) / 1e6)
    };
    let warm = post(1_000_000)?;
    // Size the real test from the warm-up (TCP ramp-up makes the warm-up pessimistic).
    let bytes = ((warm * 1e6 / 8.0) * 3.0).clamp(2e6, 40e6) as usize;
    Ok(post(bytes)?.max(warm))
}

pub fn measure(canvas: (u32, u32), service: String) -> Measurement {
    let (upload_mbps, upload_error) = match upload_mbps() {
        Ok(v) => (Some((v * 10.0).round() / 10.0), None),
        Err(e) => (None, Some(e)),
    };
    Measurement {
        upload_mbps,
        upload_error,
        cpu_name: cpu_name(),
        cpu_threads: std::thread::available_parallelism().map_or(4, |n| n.get()),
        gpu_name: gpu_name(),
        encoders: outputs::video_encoders(),
        canvas,
        service,
    }
}

// ---------------------------------------------------------------------------
// Levels and candidates

struct Level {
    max_height: u32,
    max_fps: u32,
    /// Share of the usable upload/platform budget used as bitrate.
    bitrate_share: f64,
    nvenc_preset: &'static str,
    nvenc_multipass: &'static str,
    x264_preset: &'static str,
    qsv_usage: &'static str,
    amf_preset: &'static str,
}

const LEVELS: [Level; 5] = [
    Level { max_height: 720, max_fps: 30, bitrate_share: 0.6, nvenc_preset: "p1", nvenc_multipass: "disabled", x264_preset: "ultrafast", qsv_usage: "TU7", amf_preset: "speed" },
    Level { max_height: 720, max_fps: 60, bitrate_share: 0.75, nvenc_preset: "p3", nvenc_multipass: "disabled", x264_preset: "superfast", qsv_usage: "TU7", amf_preset: "speed" },
    Level { max_height: 1080, max_fps: 60, bitrate_share: 1.0, nvenc_preset: "p5", nvenc_multipass: "qres", x264_preset: "veryfast", qsv_usage: "TU5", amf_preset: "balanced" },
    Level { max_height: 1080, max_fps: 60, bitrate_share: 1.0, nvenc_preset: "p6", nvenc_multipass: "qres", x264_preset: "faster", qsv_usage: "TU3", amf_preset: "balanced" },
    Level { max_height: 1440, max_fps: 60, bitrate_share: 1.0, nvenc_preset: "p7", nvenc_multipass: "fullres", x264_preset: "fast", qsv_usage: "TU1", amf_preset: "balanced" },
];

pub const LEVEL_NAMES: [&str; 5] = ["Smoothest PC", "Favor PC", "Balanced", "Favor stream", "Best stream"];

/// (bitrate cap kbps, note, 1440p allowed)
fn platform_cap(service: &str) -> (u32, &'static str, bool) {
    let s = service.to_lowercase();
    if s.contains("twitch") {
        (6000, "Twitch recommends at most 6000 kbps (1080p max)", false)
    } else if s.contains("youtube") {
        (16000, "YouTube accepts up to ~16000 kbps for 1440p60", true)
    } else if s.contains("kick") {
        (8000, "Kick accepts up to 8000 kbps (1080p max)", false)
    } else if s.contains("facebook") {
        (6000, "Facebook recommends at most 6000 kbps (1080p max)", false)
    } else {
        (12000, "Custom server: capped at 12000 kbps", true)
    }
}

/// (width, height, fps, minimum kbps that looks clean), best first.
const LADDER: [(u32, u32, u32, u32); 7] = [
    (2560, 1440, 60, 12000),
    (2560, 1440, 30, 9000),
    (1920, 1080, 60, 6000),
    (1920, 1080, 30, 4500),
    (1280, 720, 60, 4500),
    (1280, 720, 30, 3000),
    (852, 480, 30, 1500),
];

pub fn encoder_settings(encoder_id: &str, level: u8, bitrate: u32) -> Value {
    let l = &LEVELS[(level.clamp(1, 5) - 1) as usize];
    let mut s = json!({ "rate_control": "CBR", "bitrate": bitrate, "keyint_sec": 2 });
    let id = encoder_id.to_lowercase();
    if id.contains("nvenc") {
        s["preset"] = json!(l.nvenc_preset);
        s["multipass"] = json!(l.nvenc_multipass);
        s["tune"] = json!("hq");
    } else if id.contains("x264") {
        s["preset"] = json!(l.x264_preset);
    } else if id.contains("qsv") {
        s["target_usage"] = json!(l.qsv_usage);
    } else if id.contains("amf") {
        s["preset"] = json!(l.amf_preset);
    }
    s
}

/// Encoder, bitrate and the candidate list for the encode test (best first, never empty).
fn plan(m: &Measurement, level: u8, reasons: &mut Vec<String>) -> (Option<EncoderInfo>, u32, Vec<(u32, u32, u32)>) {
    let l = &LEVELS[(level.clamp(1, 5) - 1) as usize];
    // Encoders come sorted best-first: GPU (zero-copy) H.264 before everything else.
    let enc = m.encoders.iter().find(|e| e.texture && e.codec == "h264")
        .or_else(|| m.encoders.iter().find(|e| e.id == "obs_x264"))
        .or(m.encoders.first())
        .cloned();
    match &enc {
        Some(e) if e.texture => reasons.push(format!("{}: encodes on the graphics card ({}), so games keep the CPU", e.name, m.gpu_name)),
        Some(e) => reasons.push(format!("No hardware H.264 encoder; {} on the CPU ({}, {} threads)", e.name, m.cpu_name, m.cpu_threads)),
        None => reasons.push("No video encoder found".into()),
    }

    let (cap, cap_note, allow_1440) = platform_cap(&m.service);
    let usable = match m.upload_mbps {
        // Keep ~25 % of the upload free for the game, chat and other traffic.
        Some(up) => {
            let b = ((up * 1000.0 * 0.75) as u32).saturating_sub(160);
            reasons.push(format!("Upload {up} Mbit/s → about {b} kbps usable"));
            b
        }
        None => {
            reasons.push("Upload test unavailable: assuming 6000 kbps".into());
            6000
        }
    };
    reasons.push(cap_note.into());
    let budget = usable.min(cap);
    let bitrate = ((budget as f64 * l.bitrate_share) as u32).max(1000);
    reasons.push(format!("Level \"{}\": up to {}p{}, {} % of the budget → {bitrate} kbps",
        LEVEL_NAMES[(level.clamp(1, 5) - 1) as usize], l.max_height, l.max_fps, (l.bitrate_share * 100.0) as u32));

    let hardware = enc.as_ref().is_some_and(|e| e.texture);
    let mut list: Vec<(u32, u32, u32)> = LADDER
        .iter()
        .filter(|&&(w, h, f, min)| {
            h <= l.max_height && f <= l.max_fps && min <= bitrate
                && w <= m.canvas.0.max(852) && h <= m.canvas.1.max(480)
                && (h < 1440 || allow_1440)
                // CPU encoding above 1080p60 is not realistic while gaming.
                && (hardware || h <= 1080)
        })
        .map(|&(w, h, f, _)| (w, h, f))
        .collect();
    if list.is_empty() {
        list.push((852, 480, 30));
    }
    (enc, bitrate, list)
}

// ---------------------------------------------------------------------------
// Encode test

fn video_counters() -> (u64, u64, u64, u64) {
    unsafe {
        let v = sys::obs_get_video();
        (
            sys::video_output_get_skipped_frames(v) as u64,
            sys::video_output_get_total_frames(v) as u64,
            sys::obs_get_lagged_frames() as u64,
            sys::obs_get_total_frames() as u64,
        )
    }
}

/// Encodes `TRIAL` seconds at one resolution/FPS into a temp file and reports dropped frames.
fn run_trial(studio: &Arc<Mutex<Studio>>, base: &VideoConfig, cand: (u32, u32, u32), encoder_id: &str, enc_settings: &Value) -> Result<Trial, String> {
    let (w, h, fps) = cand;
    {
        let s = studio.lock().map_err(|_| "Studio state poisoned")?;
        if s.outputs.any_active() {
            return Err("Stop streaming, recording, replay buffer and virtual camera first".into());
        }
        let cfg = VideoConfig { output_width: w, output_height: h, fps_num: fps, fps_den: 1, ..*base };
        runtime::reset_video(&cfg)?;
    }
    let path = std::env::temp_dir().join("streamtubah-encode-test.mkv");
    let result = (|| -> Result<Trial, String> {
        let (vid, vname) = (cstr(encoder_id), cstr("optimize_test_video"));
        let venc = Encoder::from_raw(unsafe {
            sys::obs_video_encoder_create(vid.as_ptr(), vname.as_ptr(), Data::from_json(enc_settings).as_ptr(), std::ptr::null_mut())
        })
        .ok_or("Could not create the test encoder")?;
        let (aid, aname) = (cstr("ffmpeg_aac"), cstr("optimize_test_audio"));
        let aenc = Encoder::from_raw(unsafe { sys::obs_audio_encoder_create(aid.as_ptr(), aname.as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut()) })
            .ok_or("Could not create the test audio encoder")?;
        let data = Data::new();
        data.set_string("path", &path.to_string_lossy());
        let (oid, oname) = (cstr("ffmpeg_muxer"), cstr("optimize_test"));
        let out = Output::from_raw(unsafe { sys::obs_output_create(oid.as_ptr(), oname.as_ptr(), data.as_ptr(), std::ptr::null_mut()) })
            .ok_or("Could not create the test output")?;
        unsafe {
            sys::obs_encoder_set_video(venc.as_ptr(), sys::obs_get_video());
            sys::obs_encoder_set_audio(aenc.as_ptr(), sys::obs_get_audio());
            sys::obs_output_set_video_encoder(out.as_ptr(), venc.as_ptr());
            sys::obs_output_set_audio_encoder(out.as_ptr(), aenc.as_ptr(), 0);
            if !sys::obs_output_start(out.as_ptr()) {
                return Err(out.last_error().unwrap_or_else(|| "The encoder could not start at this setting".into()));
            }
        }
        std::thread::sleep(Duration::from_secs(1));
        let a = video_counters();
        std::thread::sleep(TRIAL);
        let b = video_counters();
        unsafe { sys::obs_output_force_stop(out.as_ptr()) };
        drop(out);
        let pct = |d: u64, t: u64| if t == 0 { 100.0 } else { (d as f64 / t as f64 * 1000.0).round() / 10.0 };
        let skipped_pct = pct(b.0.saturating_sub(a.0), b.1.saturating_sub(a.1));
        let lagged_pct = pct(b.2.saturating_sub(a.2), b.3.saturating_sub(a.3));
        Ok(Trial { width: w, height: h, fps, passed: skipped_pct <= MAX_DROP_PCT && lagged_pct <= MAX_DROP_PCT, skipped_pct, lagged_pct })
    })();
    let _ = std::fs::remove_file(&path);
    // An encoder that refuses a setting (e.g. a GPU without 1440p support) simply fails that step.
    Ok(result.unwrap_or_else(|e| {
        eprintln!("encode test {w}x{h}@{fps}: {e}");
        Trial { width: w, height: h, fps, passed: false, skipped_pct: 100.0, lagged_pct: 0.0 }
    }))
}

/// Full Auto-Optimize run. Emits `optimize:progress` {step} while testing; restores the
/// current video settings afterwards (nothing is applied here).
pub fn run(app: &AppHandle, studio: &Arc<Mutex<Studio>>, level: u8) -> Result<(Measurement, Recommendation), String> {
    // Two runs at once would change the video settings under each other and skew the results.
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Auto-Optimize is already running".into());
    }
    let result = run_inner(app, studio, level);
    RUNNING.store(false, Ordering::SeqCst);
    result
}

fn run_inner(app: &AppHandle, studio: &Arc<Mutex<Studio>>, level: u8) -> Result<(Measurement, Recommendation), String> {
    let level = level.clamp(1, 5);
    let (base, service) = {
        let s = studio.lock().map_err(|_| "Studio state poisoned")?;
        if s.outputs.any_active() {
            return Err("Stop streaming, recording, replay buffer and virtual camera first".into());
        }
        let svc = &s.settings.stream;
        let name = svc.service_settings["service"].as_str().unwrap_or("").to_string();
        (s.settings.video.clone(), if svc.service_type == "rtmp_common" { name } else { svc.service_type.clone() })
    };
    let progress = |step: String| {
        let _ = app.emit("optimize:progress", json!({ "step": step }));
    };

    progress("Measuring upload speed…".into());
    let base_cfg = base.to_config();
    let m = measure((base_cfg.base_width, base_cfg.base_height), service);
    let mut reasons = Vec::new();
    let (enc, bitrate, candidates) = plan(&m, level, &mut reasons);
    let enc = enc.ok_or("No video encoder found")?;
    let enc_settings = encoder_settings(&enc.id, level, bitrate);

    let mut trials = Vec::new();
    let mut chosen = *candidates.last().unwrap();
    for cand in &candidates {
        progress(format!("Testing {}p{} with {}…", cand.1, cand.2, enc.name));
        let t = run_trial(studio, &base_cfg, *cand, &enc.id, &enc_settings);
        let restore = runtime::reset_video(&base_cfg);
        let t = t?;
        restore?;
        let passed = t.passed;
        trials.push(t);
        if passed {
            chosen = *cand;
            break;
        }
    }
    let (width, height, fps) = chosen;
    match trials.iter().find(|t| t.passed) {
        Some(_) => reasons.push(format!("Encode test: {width}x{height} at {fps} FPS ran without dropped frames on this PC")),
        None => reasons.push("Encode test: every setting dropped frames; using the lightest one".into()),
    }
    let rec = Recommendation {
        level,
        encoder_id: enc.id.clone(),
        encoder_name: enc.name.clone(),
        hardware: enc.texture,
        bitrate_kbps: bitrate,
        width,
        height,
        fps,
        encoder_settings: enc_settings,
        reasons,
        trials,
    };
    Ok((m, rec))
}
