//! Extra stream destinations (multistream). Each destination is its own libobs stream output
//! sending straight to its platform (no relay server). By default it reuses the main video and
//! audio encoders, so extra destinations cost upload bandwidth but almost no GPU/CPU; with
//! `own_encoder` it gets a separate video encoder at its own bitrate.

use std::time::Instant;

use serde_json::{Value, json};

use super::{OutputSlot, Outputs, resolve_video_encoder, service_wants_opus, start};
use crate::obs::{Data, Encoder, Service, cstr, from_cstr, sys};
use crate::settings::{AppSettings, StreamTarget};

pub(super) struct TargetOut {
    pub(super) slot: OutputSlot,
    label: String,
    /// Own video encoder; must outlive the running output.
    own_video: Option<Encoder>,
    last_bytes: u64,
    pub(super) started: Option<Instant>,
}

impl TargetOut {
    pub(super) fn active(&self) -> bool {
        self.slot.output.active()
    }
}

pub(super) fn stats(o: *mut sys::obs_output_t, last_bytes: &mut u64, secs: f64) -> Value {
    unsafe {
        let bytes = sys::obs_output_get_total_bytes(o);
        let kbps = if *last_bytes == 0 { 0.0 } else { bytes.saturating_sub(*last_bytes) as f64 * 8.0 / 1000.0 / secs };
        *last_bytes = bytes;
        json!({
            "kbps": kbps.round(),
            "dropped": sys::obs_output_get_frames_dropped(o),
            "total": sys::obs_output_get_total_frames(o),
            "congestion": sys::obs_output_get_congestion(o),
            "reconnecting": sys::obs_output_reconnecting(o),
        })
    }
}

impl Outputs {
    /// Service object of a destination, recreated when its type changes.
    pub fn target_service(&mut self, t: &StreamTarget) -> Result<&Service, String> {
        if self.target_services.get(&t.id).is_none_or(|(_, ty)| *ty != t.service_type) {
            let data = Data::from_json(&t.service_settings);
            let (ty, n) = (cstr(&t.service_type), cstr(&format!("service_{}", t.id)));
            let svc = Service::from_raw(unsafe { sys::obs_service_create(ty.as_ptr(), n.as_ptr(), data.as_ptr(), std::ptr::null_mut()) })
                .ok_or_else(|| format!("Unknown service type '{}'", t.service_type))?;
            self.target_services.insert(t.id.clone(), (svc, t.service_type.clone()));
        }
        Ok(&self.target_services[&t.id].0)
    }

    pub fn start_target(&mut self, settings: &AppSettings, id: &str) -> Result<(), String> {
        let t = settings.stream.targets.iter().find(|t| t.id == id).cloned().ok_or("Destination not found")?;
        if self.targets.get(id).is_some_and(TargetOut::active) {
            return Ok(());
        }
        let service = self.target_service(&t)?.clone();
        let preferred = from_cstr(unsafe { sys::obs_service_get_preferred_output_type(service.as_ptr()) });
        let type_id = if preferred.is_empty() { "rtmp_output".to_string() } else { preferred };
        let label = format!("Stream · {}", t.name);
        if self.targets.get(id).is_none_or(|x| x.slot.type_id != type_id || x.label != label) {
            let slot = OutputSlot::create(&self.app, &format!("stream:{id}"), &label, &type_id, &Data::new())?;
            self.targets.insert(id.into(), TargetOut { slot, label, own_video: None, last_bytes: 0, started: None });
        }

        let opus = service_wants_opus(&service);
        let track = settings.output.stream_track.min(5) as usize;
        self.ensure_encoders(&settings.output, opus, &[track])?;
        let own_video = if t.own_encoder { Some(own_video_encoder(settings, &t)?) } else { None };

        let audio = if opus { &self.opus_enc } else { &self.aac_enc[track] };
        let audio = audio.as_ref().unwrap().as_ptr();
        let video = own_video.as_ref().or(self.video_enc.as_ref()).unwrap().as_ptr();
        let target = self.targets.get_mut(id).unwrap();
        let o = target.slot.output.as_ptr();
        unsafe {
            sys::obs_output_set_video_encoder(o, video);
            sys::obs_output_set_audio_encoder(o, audio, 0);
            sys::obs_output_set_service(o, service.as_ptr());
            sys::obs_output_set_reconnect_settings(o, 20, 2);
        }
        target.own_video = own_video;
        target.last_bytes = 0;
        start(&target.slot.output)?;
        target.started = Some(Instant::now());
        Ok(())
    }

    pub fn stop_target(&mut self, id: &str) {
        if let Some(t) = self.targets.get(id).filter(|t| t.active()) {
            unsafe { sys::obs_output_stop(t.slot.output.as_ptr()) }
        }
    }

    pub(super) fn targets_active(&self) -> bool {
        self.targets.values().any(TargetOut::active)
    }

    /// While live: start destinations that were enabled (main included), stop disabled or
    /// removed ones. Returns one message per destination that failed to start.
    pub fn sync_destinations(&mut self, settings: &AppSettings) -> Vec<String> {
        let mut errors = Vec::new();
        let main_live = self.stream.as_ref().is_some_and(|s| s.output.active());
        if settings.stream.main_enabled && !main_live {
            if let Err(e) = self.start_main_stream(settings) {
                errors.push(format!("Main: {e}"));
            }
        } else if !settings.stream.main_enabled && main_live {
            unsafe { sys::obs_output_stop(self.stream.as_ref().unwrap().output.as_ptr()) }
        }
        let ids: Vec<String> = self.targets.keys().cloned().collect();
        for id in ids {
            if !settings.stream.targets.iter().any(|t| t.id == id && t.enabled) {
                self.stop_target(&id);
            }
        }
        for t in settings.stream.targets.iter().filter(|t| t.enabled) {
            if let Err(e) = self.start_target(settings, &t.id) {
                errors.push(format!("{}: {e}", t.name));
            }
        }
        errors
    }

    /// Per-destination status for the UI, main first.
    pub(super) fn destinations(&self, settings_names: &[(String, String)]) -> Vec<Value> {
        settings_names
            .iter()
            .map(|(id, name)| {
                let active = self.targets.get(id).is_some_and(TargetOut::active);
                let reconnecting = active && unsafe { sys::obs_output_reconnecting(self.targets[id].slot.output.as_ptr()) };
                json!({ "id": id, "name": name, "active": active, "reconnecting": reconnecting })
            })
            .collect()
    }

    pub(super) fn target_stats(&mut self, secs: f64) -> Vec<Value> {
        self.targets
            .iter_mut()
            .filter(|(_, t)| t.active())
            .map(|(id, t)| {
                let mut s = stats(t.slot.output.as_ptr(), &mut t.last_bytes, secs);
                s["id"] = json!(id);
                s
            })
            .collect()
    }
}

/// Video encoder dedicated to one destination: the main encoder type and settings with the
/// destination's bitrate.
fn own_video_encoder(settings: &AppSettings, t: &StreamTarget) -> Result<Encoder, String> {
    let o = &settings.output;
    let id = resolve_video_encoder(&o.video_encoder).ok_or("No video encoder available")?;
    let mut s = if o.video_encoder_settings.is_object() { o.video_encoder_settings.clone() } else { json!({}) };
    s["bitrate"] = json!(t.bitrate_kbps.clamp(500, 50_000));
    s["rate_control"] = json!("CBR");
    let data = Data::from_json(&s);
    let (vid, name) = (cstr(&id), cstr(&format!("video_{}", t.id)));
    let enc = Encoder::from_raw(unsafe { sys::obs_video_encoder_create(vid.as_ptr(), name.as_ptr(), data.as_ptr(), std::ptr::null_mut()) })
        .ok_or_else(|| format!("Failed to create video encoder '{id}'"))?;
    unsafe { sys::obs_encoder_set_video(enc.as_ptr(), sys::obs_get_video()) };
    Ok(enc)
}
