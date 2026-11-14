//! Stream / record / replay-buffer outputs sharing one pair of encoders
//! (like Streamlabs "simple" output mode). Encoders are created lazily on the
//! first start so they always bind to the current video/audio pipeline.

mod multistream;

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::obs::{self, Data, Encoder, Output, Service, cstr, from_cstr, signals, sys};
use crate::settings::{AppSettings, OutputSettings, StreamSettings};

#[derive(Serialize, Clone)]
pub struct EncoderInfo {
    pub id: String,
    pub name: String,
    pub codec: String,
    pub texture: bool,
}

/// Video encoders usable for new outputs, best first.
pub fn video_encoders() -> Vec<EncoderInfo> {
    let mut out = Vec::new();
    let mut idx = 0;
    let mut id = std::ptr::null();
    while unsafe { sys::obs_enum_encoder_types(idx, &mut id) } {
        idx += 1;
        unsafe {
            if sys::obs_get_encoder_type(id) != sys::obs_encoder_type_OBS_ENCODER_VIDEO {
                continue;
            }
            let caps = sys::obs_get_encoder_caps(id);
            if caps & (sys::OBS_ENCODER_CAP_DEPRECATED | sys::OBS_ENCODER_CAP_INTERNAL) != 0 {
                continue;
            }
            out.push(EncoderInfo {
                id: from_cstr(id),
                name: from_cstr(sys::obs_encoder_get_display_name(id)),
                codec: from_cstr(sys::obs_get_encoder_codec(id)),
                texture: caps & sys::OBS_ENCODER_CAP_PASS_TEXTURE != 0,
            });
        }
    }
    // Hardware texture encoders (zero-copy from GPU) first, then H.264 before other codecs.
    out.sort_by_key(|e| (!e.texture, e.codec != "h264"));
    out
}

pub fn resolve_video_encoder(wanted: &str) -> Option<String> {
    let all = video_encoders();
    all.iter()
        .find(|e| e.id == wanted)
        .or_else(|| all.first())
        .map(|e| e.id.clone())
}

struct SignalCtx {
    /// "stream" | "record" | "replay" | "virtualcam" | "stream:<destination id>"
    kind: String,
    /// Name shown in notifications.
    label: String,
    signal: &'static str,
    app: AppHandle,
}

unsafe extern "C" fn on_signal(data: *mut c_void, cd: *mut sys::calldata_t) {
    // SAFETY: data is a SignalCtx owned by the OutputSlot that owns this output.
    let ctx = unsafe { &*(data as *const SignalCtx) };
    let output = signals::get_ptr(cd, "output") as *mut sys::obs_output_t;
    let mut payload = json!({ "output": ctx.kind, "label": ctx.label, "signal": ctx.signal });
    if ctx.signal == "stop" {
        let code = signals::get_int(cd, "code").unwrap_or(0);
        payload["code"] = json!(code);
        if code != sys::OBS_OUTPUT_SUCCESS as i64 && !output.is_null() {
            let err = from_cstr(unsafe { sys::obs_output_get_last_error(output) });
            payload["error"] = json!(if err.is_empty() { stop_reason(code) } else { err });
        }
    }
    if ctx.signal == "saved" && !output.is_null() {
        let ph = unsafe { sys::obs_output_get_proc_handler(output) };
        if let Some(r) = signals::CallData::call(ph, "get_last_replay") {
            payload["path"] = json!(r.string("path"));
        }
    }
    let _ = ctx.app.emit("obs:output", payload);
}

fn stop_reason(code: i64) -> String {
    match code as i32 {
        sys::OBS_OUTPUT_BAD_PATH => "Invalid output path",
        sys::OBS_OUTPUT_CONNECT_FAILED => "Could not connect to server",
        sys::OBS_OUTPUT_INVALID_STREAM => "Invalid stream key or server",
        sys::OBS_OUTPUT_DISCONNECTED => "Disconnected from server",
        sys::OBS_OUTPUT_UNSUPPORTED => "Unsupported settings for this output",
        sys::OBS_OUTPUT_NO_SPACE => "Disk is full",
        sys::OBS_OUTPUT_ENCODE_ERROR => "Encoder error",
        _ => "Output stopped with an error",
    }
    .into()
}

/// An output plus the signal contexts it points at. Field order matters:
/// the output is destroyed before the contexts are freed.
struct OutputSlot {
    output: Output,
    type_id: String,
    _ctx: Vec<Box<SignalCtx>>,
}

impl OutputSlot {
    fn create(app: &AppHandle, kind: &str, label: &str, type_id: &str, settings: &Data) -> Result<Self, String> {
        let (t, n) = (cstr(type_id), cstr(kind));
        let output = Output::from_raw(unsafe {
            sys::obs_output_create(t.as_ptr(), n.as_ptr(), settings.as_ptr(), std::ptr::null_mut())
        })
        .ok_or_else(|| format!("Failed to create output '{type_id}'"))?;
        let handler = unsafe { sys::obs_output_get_signal_handler(output.as_ptr()) };
        let mut ctx = Vec::new();
        for signal in ["starting", "start", "stopping", "stop", "reconnect", "reconnect_success", "saved"] {
            let c = Box::new(SignalCtx { kind: kind.into(), label: label.into(), signal, app: app.clone() });
            signals::connect(handler, signal, on_signal, &*c as *const SignalCtx as *mut c_void);
            ctx.push(c);
        }
        Ok(Self { output, type_id: type_id.into(), _ctx: ctx })
    }
}

pub struct Outputs {
    app: AppHandle,
    service: Option<(Service, String)>,
    stream: Option<OutputSlot>,
    record: Option<OutputSlot>,
    replay: Option<OutputSlot>,
    /// Raw program video for the "OBS Virtual Camera" DirectShow device; no encoders.
    virtualcam: Option<OutputSlot>,
    video_enc: Option<Encoder>,
    /// Second canvas + its own video encoder for recording / replay buffer, used when a source
    /// is limited to the stream or the recording (see split.rs). Created on start, dropped when
    /// both are idle. Tuple order matters: the encoder is released before its canvas.
    record_view: Option<(Encoder, crate::split::RecordView)>,
    /// One AAC encoder per audio track (mixer index 0-5), created on demand.
    aac_enc: [Option<Encoder>; 6],
    /// WebRTC (WHIP) only accepts Opus; streams carry a single track.
    opus_enc: Option<Encoder>,
    /// Extra multistream destinations by id, and their service objects.
    targets: HashMap<String, multistream::TargetOut>,
    target_services: HashMap<String, (Service, String)>,
    started: [Option<Instant>; 4],
    last_bytes: u64,
    last_tick: Instant,
}

fn kind_index(kind: &str) -> usize {
    match kind {
        "stream" => 0,
        "record" => 1,
        "virtualcam" => 3,
        _ => 2,
    }
}

impl Outputs {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            service: None,
            stream: None,
            record: None,
            replay: None,
            virtualcam: None,
            video_enc: None,
            record_view: None,
            aac_enc: Default::default(),
            opus_enc: None,
            targets: HashMap::new(),
            target_services: HashMap::new(),
            started: [None; 4],
            last_bytes: 0,
            last_tick: Instant::now(),
        }
    }

    fn slots(&self) -> impl Iterator<Item = &OutputSlot> {
        [&self.stream, &self.record, &self.replay, &self.virtualcam].into_iter().flatten()
    }

    fn slot(&self, kind: &str) -> &Option<OutputSlot> {
        match kind {
            "stream" => &self.stream,
            "record" => &self.record,
            "virtualcam" => &self.virtualcam,
            _ => &self.replay,
        }
    }

    pub fn any_active(&self) -> bool {
        self.slots().any(|s| s.output.active()) || self.targets_active()
    }

    /// Outputs that carry audio (the virtual camera is video only).
    pub fn any_audio_active(&self) -> bool {
        [&self.stream, &self.record, &self.replay].into_iter().any(Self::is_active) || self.targets_active()
    }

    /// "stream" is running when the main destination or any extra destination is live.
    pub fn is_running(&self, kind: &str) -> bool {
        Self::is_active(self.slot(kind)) || (kind == "stream" && self.targets_active())
    }

    pub fn record_paused(&self) -> bool {
        self.record.as_ref().is_some_and(|s| unsafe { sys::obs_output_paused(s.output.as_ptr()) })
    }

    fn is_active(slot: &Option<OutputSlot>) -> bool {
        slot.as_ref().is_some_and(|s| s.output.active())
    }

    /// Drops encoders so the next start picks up new settings. No-op while live.
    pub fn invalidate_encoders(&mut self) {
        if !self.any_active() {
            self.video_enc = None;
            self.record_view = None;
            self.aac_enc = Default::default();
            self.opus_enc = None;
        }
    }

    /// Video encoder plus audio encoders for the given tracks (mixer indexes 0-5).
    fn ensure_encoders(&mut self, s: &OutputSettings, opus: bool, tracks: &[usize]) -> Result<(), String> {
        if self.video_enc.is_none() {
            self.create_video_encoder(s)?;
        }
        for &track in tracks {
            let slot = if opus { &mut self.opus_enc } else { &mut self.aac_enc[track] };
            if slot.is_some() {
                continue;
            }
            let (type_id, name) = if opus { ("ffmpeg_opus", "main_audio_opus".to_string()) } else { ("ffmpeg_aac", format!("audio_track{}", track + 1)) };
            let adata = Data::new();
            adata.set_int("bitrate", s.audio_bitrate as i64);
            let (aid, aname) = (cstr(type_id), cstr(&name));
            let aenc = Encoder::from_raw(unsafe {
                sys::obs_audio_encoder_create(aid.as_ptr(), aname.as_ptr(), adata.as_ptr(), track, std::ptr::null_mut())
            })
            .ok_or_else(|| format!("Failed to create audio encoder '{type_id}'"))?;
            unsafe { sys::obs_encoder_set_audio(aenc.as_ptr(), sys::obs_get_audio()) };
            *slot = Some(aenc);
        }
        Ok(())
    }

    fn create_video_encoder(&mut self, s: &OutputSettings) -> Result<(), String> {
        self.video_enc = Some(new_video_encoder(s, "main_video", unsafe { sys::obs_get_video() })?);
        Ok(())
    }

    /// Video encoder for recording / replay buffer: the main one, or the record canvas's own
    /// encoder while some source is stream-only / recording-only.
    fn record_video_encoder(&mut self, s: &OutputSettings) -> Result<*mut sys::obs_encoder_t, String> {
        let record_idle = !Self::is_active(&self.record) && !Self::is_active(&self.replay);
        if record_idle {
            self.record_view = None;
        }
        if self.record_view.is_none() && record_idle && crate::split::any_split() {
            let view = crate::split::RecordView::new()?;
            let enc = new_video_encoder(s, "record_video", view.video())?;
            self.record_view = Some((enc, view));
        }
        match &self.record_view {
            Some((enc, _)) => Ok(enc.as_ptr()),
            None => Ok(self.video_enc.as_ref().ok_or("No video encoder")?.as_ptr()),
        }
    }

    /// Frees the record canvas once recording and replay buffer have both stopped (GPU time).
    pub fn release_idle_record_view(&mut self) {
        if self.record_view.is_some() && !Self::is_active(&self.record) && !Self::is_active(&self.replay) {
            self.record_view = None;
        }
    }

    /// True while recording / replay buffer use the separate record canvas.
    pub fn record_split_active(&self) -> bool {
        self.record_view.is_some()
    }

    /// Attaches the video encoder and one audio encoder per track, in track order.
    fn attach_encoders(&self, output: &Output, opus: bool, tracks: &[usize]) {
        self.attach_encoders_with(output, self.video_enc.as_ref().unwrap().as_ptr(), opus, tracks)
    }

    fn attach_encoders_with(&self, output: &Output, video: *mut sys::obs_encoder_t, opus: bool, tracks: &[usize]) {
        unsafe {
            sys::obs_output_set_video_encoder(output.as_ptr(), video);
            for (idx, &track) in tracks.iter().enumerate() {
                let enc = if opus { &self.opus_enc } else { &self.aac_enc[track] };
                sys::obs_output_set_audio_encoder(output.as_ptr(), enc.as_ref().unwrap().as_ptr(), idx);
            }
        }
    }

    /// The streaming service object, (re)created when the service type changes.
    pub fn service(&mut self, s: &StreamSettings) -> Result<&Service, String> {
        if self.service.as_ref().is_none_or(|(_, t)| *t != s.service_type) {
            let data = Data::from_json(&s.service_settings);
            let (t, n) = (cstr(&s.service_type), cstr("main_service"));
            let svc = Service::from_raw(unsafe {
                sys::obs_service_create(t.as_ptr(), n.as_ptr(), data.as_ptr(), std::ptr::null_mut())
            })
            .ok_or_else(|| format!("Unknown service type '{}'", s.service_type))?;
            self.service = Some((svc, s.service_type.clone()));
        }
        Ok(&self.service.as_ref().unwrap().0)
    }

    /// Forgets the service so the next stream start uses freshly loaded settings.
    pub fn reset_service(&mut self) {
        if !self.is_running("stream") {
            self.service = None;
            self.target_services.clear();
        }
    }

    /// Goes live on the main destination (if enabled) and every enabled extra destination.
    /// Destinations that start keep running even if others fail; failures are reported.
    pub fn start_stream(&mut self, settings: &AppSettings) -> Result<(), String> {
        let mut errors = Vec::new();
        let mut any = false;
        if settings.stream.main_enabled {
            any = true;
            if let Err(e) = self.start_main_stream(settings) {
                errors.push(format!("Main: {e}"));
            }
        }
        for t in settings.stream.targets.iter().filter(|t| t.enabled) {
            any = true;
            if let Err(e) = self.start_target(settings, &t.id) {
                errors.push(format!("{}: {e}", t.name));
            }
        }
        if !any {
            return Err("No destination is enabled (Settings → Stream)".into());
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    fn start_main_stream(&mut self, settings: &AppSettings) -> Result<(), String> {
        if Self::is_active(&self.stream) {
            return Ok(());
        }
        let service = self.service(&settings.stream)?.clone();
        let preferred = from_cstr(unsafe { sys::obs_service_get_preferred_output_type(service.as_ptr()) });
        let type_id = if preferred.is_empty() { "rtmp_output".to_string() } else { preferred };
        if self.stream.as_ref().is_none_or(|s| s.type_id != type_id) {
            self.stream = Some(OutputSlot::create(&self.app, "stream", "Stream", &type_id, &Data::new())?);
        }
        let opus = service_wants_opus(&service);
        let tracks = [settings.output.stream_track.min(5) as usize];
        self.ensure_encoders(&settings.output, opus, &tracks)?;
        let slot = self.stream.as_ref().unwrap();
        self.attach_encoders(&slot.output, opus, &tracks);
        unsafe {
            sys::obs_output_set_service(slot.output.as_ptr(), service.as_ptr());
            sys::obs_output_set_reconnect_settings(slot.output.as_ptr(), 20, 2);
        }
        self.last_bytes = 0;
        start(&slot.output)?;
        self.started[0] = Some(Instant::now());
        Ok(())
    }

    pub fn start_record(&mut self, settings: &AppSettings) -> Result<(), String> {
        if Self::is_active(&self.record) {
            return Ok(());
        }
        let o = &settings.output;
        std::fs::create_dir_all(&o.record_path).map_err(|e| format!("Recording folder: {e}"))?;
        let (ext, muxer) = record_container(&o.record_format);
        let path = std::path::Path::new(&o.record_path).join(formatted_filename(ext));
        let data = Data::new();
        data.set_string("path", &path.to_string_lossy());
        data.set_string("muxer_settings", muxer);
        match &self.record {
            Some(slot) => unsafe { sys::obs_output_update(slot.output.as_ptr(), data.as_ptr()) },
            None => self.record = Some(OutputSlot::create(&self.app, "record", "Recording", "ffmpeg_muxer", &data)?),
        }
        // FLV holds a single audio track.
        let tracks = record_tracks(o.record_tracks, ext == "flv");
        self.ensure_encoders(o, false, &tracks)?;
        let venc = self.record_video_encoder(o)?;
        let slot = self.record.as_ref().unwrap();
        self.attach_encoders_with(&slot.output, venc, false, &tracks);
        start(&slot.output)?;
        self.started[1] = Some(Instant::now());
        Ok(())
    }

    pub fn start_replay(&mut self, settings: &AppSettings) -> Result<(), String> {
        if Self::is_active(&self.replay) {
            return Ok(());
        }
        let o = &settings.output;
        std::fs::create_dir_all(&o.record_path).map_err(|e| format!("Recording folder: {e}"))?;
        let data = Data::new();
        data.set_string("directory", &o.record_path);
        data.set_string("format", "Replay %CCYY-%MM-%DD %hh-%mm-%ss");
        data.set_string("extension", "mp4");
        data.set_int("max_time_sec", o.replay_seconds.clamp(5, 21600) as i64);
        data.set_int("max_size_mb", 2048);
        match &self.replay {
            Some(slot) => unsafe { sys::obs_output_update(slot.output.as_ptr(), data.as_ptr()) },
            None => self.replay = Some(OutputSlot::create(&self.app, "replay", "Replay buffer", "replay_buffer", &data)?),
        }
        let tracks = record_tracks(o.record_tracks, false);
        self.ensure_encoders(o, false, &tracks)?;
        let venc = self.record_video_encoder(o)?;
        let slot = self.replay.as_ref().unwrap();
        self.attach_encoders_with(&slot.output, venc, false, &tracks);
        start(&slot.output)?;
        self.started[2] = Some(Instant::now());
        Ok(())
    }

    pub fn start_virtualcam(&mut self) -> Result<(), String> {
        if Self::is_active(&self.virtualcam) {
            return Ok(());
        }
        if self.virtualcam.is_none() {
            self.virtualcam = Some(OutputSlot::create(&self.app, "virtualcam", "Virtual camera", "virtualcam_output", &Data::new())?);
        }
        start(&self.virtualcam.as_ref().unwrap().output)?;
        self.started[3] = Some(Instant::now());
        Ok(())
    }

    pub fn save_replay(&self) -> Result<(), String> {
        let slot = self.replay.as_ref().filter(|s| s.output.active()).ok_or("Replay buffer is not running")?;
        let ph = unsafe { sys::obs_output_get_proc_handler(slot.output.as_ptr()) };
        signals::CallData::call(ph, "save").map(|_| ()).ok_or("Replay save failed".into())
    }

    pub fn stop(&mut self, kind: &str) {
        if kind == "stream" {
            let ids: Vec<String> = self.targets.keys().cloned().collect();
            for id in ids {
                self.stop_target(&id);
            }
        }
        if let Some(s) = self.slot(kind).as_ref().filter(|s| s.output.active()) {
            unsafe { sys::obs_output_stop(s.output.as_ptr()) }
        }
    }

    pub fn set_record_paused(&self, paused: bool) -> Result<(), String> {
        let slot = self.record.as_ref().filter(|s| s.output.active()).ok_or("Not recording")?;
        unsafe { sys::obs_output_pause(slot.output.as_ptr(), paused) }
            .then_some(())
            .ok_or("This recording cannot be paused".into())
    }

    pub fn status(&self) -> Value {
        let state = |s: &Option<OutputSlot>| s.as_ref().is_some_and(|s| s.output.active());
        json!({
            "streaming": state(&self.stream) || self.targets_active(),
            "recording": state(&self.record),
            "recording_paused": self.record_paused(),
            "replay": state(&self.replay),
            "virtualcam": state(&self.virtualcam),
        })
    }

    /// Detailed status of one output (used by the control API).
    pub fn detail(&self, kind: &str) -> Value {
        // With the main destination off, report the first live extra destination.
        let extra = (kind == "stream").then(|| self.targets.values().find(|t| t.active())).flatten();
        let Some(s) = self.slot(kind).as_ref().filter(|s| s.output.active()).or(extra.map(|t| &t.slot)) else {
            return json!({ "active": false, "paused": false, "duration_ms": 0, "bytes": 0, "dropped": 0, "total": 0, "congestion": 0.0 });
        };
        let o = s.output.as_ptr();
        let started = if std::ptr::eq(s, extra.map_or(std::ptr::null(), |t| &t.slot)) { extra.and_then(|t| t.started) } else { self.started[kind_index(kind)] };
        let ms = started.map_or(0, |t| t.elapsed().as_millis() as u64);
        unsafe {
            json!({
                "active": true,
                "paused": sys::obs_output_paused(o),
                "duration_ms": ms,
                "bytes": sys::obs_output_get_total_bytes(o),
                "dropped": sys::obs_output_get_frames_dropped(o),
                "total": sys::obs_output_get_total_frames(o),
                "congestion": sys::obs_output_get_congestion(o),
            })
        }
    }

    /// Per-second stream statistics.
    /// Per-second statistics: totals over all live destinations plus one entry per destination.
    pub fn stream_stats(&mut self) -> Value {
        let secs = self.last_tick.elapsed().as_secs_f64().max(0.001);
        self.last_tick = Instant::now();
        let mut list = Vec::new();
        if let Some(slot) = self.stream.as_ref().filter(|s| s.output.active()) {
            let mut s = multistream::stats(slot.output.as_ptr(), &mut self.last_bytes, secs);
            s["id"] = json!("main");
            list.push(s);
        }
        list.extend(self.target_stats(secs));
        if list.is_empty() {
            return Value::Null;
        }
        let sum = |k: &str| list.iter().map(|s| s[k].as_f64().unwrap_or(0.0)).sum::<f64>();
        let congestion = list.iter().map(|s| s["congestion"].as_f64().unwrap_or(0.0)).fold(0.0, f64::max);
        json!({
            "kbps": sum("kbps"),
            "dropped": sum("dropped"),
            "total": sum("total"),
            "congestion": congestion,
            "destinations": list,
        })
    }

    /// Status of every configured destination (main first) for the Controls panel.
    pub fn destination_status(&self, settings: &AppSettings) -> Vec<Value> {
        let main_active = Self::is_active(&self.stream);
        let main_reconnecting = main_active && unsafe { sys::obs_output_reconnecting(self.stream.as_ref().unwrap().output.as_ptr()) };
        let mut out = vec![json!({
            "id": "main", "name": "Main", "enabled": settings.stream.main_enabled,
            "active": main_active, "reconnecting": main_reconnecting,
        })];
        let names: Vec<(String, String)> = settings.stream.targets.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
        for (mut d, t) in self.destinations(&names).into_iter().zip(&settings.stream.targets) {
            d["enabled"] = json!(t.enabled);
            out.push(d);
        }
        out
    }

    /// Stops everything immediately (used at shutdown).
    pub fn force_stop_all(&mut self) {
        for s in self.slots().chain(self.targets.values().map(|t| &t.slot)) {
            if s.output.active() {
                unsafe { sys::obs_output_force_stop(s.output.as_ptr()) }
            }
        }
        self.stream = None;
        self.record = None;
        self.replay = None;
        self.virtualcam = None;
        self.targets.clear();
        self.target_services.clear();
        self.record_view = None;
        self.video_enc = None;
        self.aac_enc = Default::default();
        self.opus_enc = None;
        self.service = None;
    }
}

fn new_video_encoder(s: &OutputSettings, name: &str, video: *mut sys::video_t) -> Result<Encoder, String> {
    let id = resolve_video_encoder(&s.video_encoder).ok_or("No video encoder available")?;
    let vdata = Data::from_json(&s.video_encoder_settings);
    let (vid, vname) = (cstr(&id), cstr(name));
    let venc = Encoder::from_raw(unsafe {
        sys::obs_video_encoder_create(vid.as_ptr(), vname.as_ptr(), vdata.as_ptr(), std::ptr::null_mut())
    })
    .ok_or_else(|| format!("Failed to create video encoder '{id}'"))?;
    unsafe { sys::obs_encoder_set_video(venc.as_ptr(), video) };
    Ok(venc)
}

/// True when the service cannot take AAC but accepts Opus (e.g. WHIP / WebRTC).
fn service_wants_opus(service: &Service) -> bool {
    let list = unsafe { sys::obs_service_get_supported_audio_codecs(service.as_ptr()) };
    if list.is_null() {
        return false;
    }
    let mut codecs = Vec::new();
    let mut i = 0;
    loop {
        let p = unsafe { *list.add(i) };
        if p.is_null() {
            break;
        }
        codecs.push(from_cstr(p));
        i += 1;
    }
    !codecs.iter().any(|c| c == "aac") && codecs.iter().any(|c| c == "opus")
}

/// Track indexes (0-5) from a bit mask; at least track 1. `single` keeps only the first.
fn record_tracks(mask: u32, single: bool) -> Vec<usize> {
    let mut tracks: Vec<usize> = (0..6).filter(|i| mask & (1 << i) != 0).collect();
    if tracks.is_empty() {
        tracks.push(0);
    }
    if single {
        tracks.truncate(1);
    }
    tracks
}

fn start(output: &Output) -> Result<(), String> {
    if unsafe { sys::obs_output_start(output.as_ptr()) } {
        Ok(())
    } else {
        Err(output.last_error().unwrap_or_else(|| "Output failed to start".into()))
    }
}

fn record_container(format: &str) -> (&'static str, &'static str) {
    match format {
        "mkv" => ("mkv", ""),
        "mov" => ("mov", ""),
        "flv" => ("flv", ""),
        "mp4" => ("mp4", ""),
        // Survives crashes: the file stays playable even if recording is cut off.
        _ => ("mp4", "movflags=frag_keyframe+empty_moov+delay_moov"),
    }
}

fn formatted_filename(ext: &str) -> String {
    let (e, f) = (cstr(ext), cstr("%CCYY-%MM-%DD %hh-%mm-%ss"));
    unsafe {
        let p = obs::os_generate_formatted_filename(e.as_ptr(), true, f.as_ptr());
        let s = from_cstr(p);
        sys::bfree(p as *mut c_void);
        s
    }
}
