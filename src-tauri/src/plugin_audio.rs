//! "Plugin Audio" — an audio input source fed over WebSocket (`/audio-source/<channel>` on the API
//! port). JavaScript plugins (or any external app) send 48 kHz stereo interleaved f32le blocks;
//! every Plugin Audio source set to that channel plays them, so a plugin's voice changer or
//! sound player shows up in the mixer like any other source.

use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::sync::{LazyLock, Mutex};

use crate::bridge::{CHANNELS, SAMPLE_RATE};
use crate::obs::{cstr, from_cstr, sys};

pub const SOURCE_ID: &str = "streamtubah_plugin_audio";

/// Raw source pointers by channel name. Entries are added in `create`/`update` and removed in
/// `destroy`, so a pointer in the map is always alive.
struct SourcePtr(*mut sys::obs_source_t);
// SAFETY: libobs sources are thread safe; the pointer is only used while registered.
unsafe impl Send for SourcePtr {}

static SOURCES: LazyLock<Mutex<HashMap<String, Vec<SourcePtr>>>> = LazyLock::new(Default::default);

fn channel_of(settings: *mut sys::obs_data_t) -> String {
    let name = from_cstr(unsafe { sys::obs_data_get_string(settings, c"channel".as_ptr()) });
    if name.trim().is_empty() { "plugin".into() } else { name.trim().to_string() }
}

struct Data {
    source: *mut sys::obs_source_t,
    channel: String,
}

fn register_ptr(channel: &str, src: *mut sys::obs_source_t) {
    SOURCES.lock().unwrap().entry(channel.to_string()).or_default().push(SourcePtr(src));
}

fn unregister_ptr(channel: &str, src: *mut sys::obs_source_t) {
    let mut map = SOURCES.lock().unwrap();
    if let Some(list) = map.get_mut(channel) {
        list.retain(|p| p.0 != src);
        if list.is_empty() {
            map.remove(channel);
        }
    }
}

unsafe extern "C" fn get_name(_: *mut c_void) -> *const c_char {
    c"Plugin Audio".as_ptr()
}

unsafe extern "C" fn create(settings: *mut sys::obs_data_t, source: *mut sys::obs_source_t) -> *mut c_void {
    let channel = channel_of(settings);
    register_ptr(&channel, source);
    Box::into_raw(Box::new(Data { source, channel })) as *mut c_void
}

unsafe extern "C" fn destroy(data: *mut c_void) {
    let d = unsafe { Box::from_raw(data as *mut Data) };
    unregister_ptr(&d.channel, d.source);
}

unsafe extern "C" fn update(data: *mut c_void, settings: *mut sys::obs_data_t) {
    let d = unsafe { &mut *(data as *mut Data) };
    let channel = channel_of(settings);
    if channel != d.channel {
        unregister_ptr(&d.channel, d.source);
        register_ptr(&channel, d.source);
        d.channel = channel;
    }
}

unsafe extern "C" fn get_defaults(settings: *mut sys::obs_data_t) {
    unsafe { sys::obs_data_set_default_string(settings, c"channel".as_ptr(), c"plugin".as_ptr()) };
}

unsafe extern "C" fn get_properties(data: *mut c_void) -> *mut sys::obs_properties_t {
    unsafe {
        let props = sys::obs_properties_create();
        sys::obs_properties_add_text(props, c"channel".as_ptr(), c"Channel name".as_ptr(), sys::obs_text_type_OBS_TEXT_DEFAULT);
        let channel = if data.is_null() { "plugin".to_string() } else { (*(data as *const Data)).channel.clone() };
        let port = crate::bridge::API_PORT.load(std::sync::atomic::Ordering::Relaxed);
        let info = cstr(&format!(
            "Plays audio sent by a plugin or app to ws://127.0.0.1:{port}/audio-source/{channel} (48 kHz stereo f32, interleaved)."
        ));
        sys::obs_properties_add_text(props, c"info".as_ptr(), info.as_ptr(), sys::obs_text_type_OBS_TEXT_INFO);
        props
    }
}

pub fn register() {
    let id = Box::leak(Box::new(cstr(SOURCE_ID)));
    unsafe {
        let mut info: sys::obs_source_info = std::mem::zeroed();
        info.id = id.as_ptr();
        info.type_ = sys::obs_source_type_OBS_SOURCE_TYPE_INPUT;
        info.output_flags = sys::OBS_SOURCE_AUDIO;
        info.get_name = Some(get_name);
        info.create = Some(create);
        info.destroy = Some(destroy);
        info.update = Some(update);
        info.get_defaults = Some(get_defaults);
        info.get_properties = Some(get_properties);
        sys::obs_register_source_s(&info, std::mem::size_of::<sys::obs_source_info>());
    }
}

/// Keeps the timestamps of one WebSocket client continuous, resyncing when the client falls
/// behind or runs ahead of real time.
pub struct Clock {
    next_ns: u64,
}

const NS: u64 = 1_000_000_000;
/// Allowed gap between the stream of blocks and real time before resyncing.
const MAX_DRIFT_NS: u64 = 200_000_000;

impl Clock {
    pub fn new() -> Self {
        Self { next_ns: 0 }
    }

    /// Timestamp for a block of `frames` arriving at `now_ns`.
    pub fn stamp(&mut self, frames: u64, now_ns: u64) -> u64 {
        let dur = frames * NS / SAMPLE_RATE as u64;
        let ideal = now_ns.saturating_sub(dur);
        if self.next_ns == 0 || self.next_ns.abs_diff(ideal) > MAX_DRIFT_NS {
            self.next_ns = ideal;
        }
        let ts = self.next_ns;
        self.next_ns += dur;
        ts
    }
}

/// Plays one block (interleaved stereo f32le bytes) on every source of `channel`.
/// Returns the number of sources that received it.
pub fn push(channel: &str, bytes: &[u8], clock: &mut Clock) -> usize {
    let usable = bytes.len() / 4 / CHANNELS * CHANNELS * 4;
    if usable == 0 {
        return 0;
    }
    let samples: Vec<f32> = bytes[..usable].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    let frames = (samples.len() / CHANNELS) as u32;
    let ts = clock.stamp(frames as u64, unsafe { crate::obs::os_gettime_ns() });
    let map = SOURCES.lock().unwrap();
    let Some(list) = map.get(channel) else { return 0 };
    let mut audio: sys::obs_source_audio = unsafe { std::mem::zeroed() };
    audio.data[0] = samples.as_ptr() as *const u8;
    audio.frames = frames;
    audio.speakers = sys::speaker_layout_SPEAKERS_STEREO;
    audio.format = sys::audio_format_AUDIO_FORMAT_FLOAT;
    audio.samples_per_sec = SAMPLE_RATE;
    audio.timestamp = ts;
    for p in list {
        unsafe { sys::obs_source_output_audio(p.0, &audio) };
    }
    list.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_continuous_and_resyncs() {
        let mut c = Clock::new();
        let block = 480; // 10 ms
        let t0 = 10 * NS;
        let a = c.stamp(block, t0);
        assert_eq!(a, t0 - 10_000_000);
        // Slight jitter keeps the stamps continuous.
        let b = c.stamp(block, t0 + 13_000_000);
        assert_eq!(b, a + 10_000_000);
        // A long pause resyncs to real time.
        let d = c.stamp(block, t0 + 5 * NS);
        assert_eq!(d, t0 + 5 * NS - 10_000_000);
    }
}
