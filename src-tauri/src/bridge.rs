//! "Audio Bridge" — a libobs audio filter registered by this app. It streams
//! the source's audio to an external process over WebSocket and plays back
//! what that process returns. Any app or language (voice changers, AI voice,
//! DSP scripts) can plug into a voice chain this way.
//!
//! Protocol: see docs/audio-bridge.md. Endpoint: ws://127.0.0.1:<api port>/audio/<channel>
//! Audio: 48 kHz, stereo, interleaved little-endian f32, binary frames both ways.

use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, LazyLock, Mutex};

use crate::obs::{cstr, from_cstr, sys};

pub const FILTER_ID: &str = "streamtubah_audio_bridge";
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: usize = 2;

/// One named connection point between filters and an external client.
#[derive(Default)]
pub struct Channel {
    pub connected: AtomicBool,
    /// Blocks of interleaved samples going to the client.
    pub to_client: Mutex<Option<SyncSender<Vec<f32>>>>,
    /// Interleaved samples returned by the client, waiting to be played.
    pub from_client: Mutex<VecDeque<f32>>,
    primed: AtomicBool,
}

impl Channel {
    pub fn attach(&self, tx: SyncSender<Vec<f32>>) {
        self.from_client.lock().unwrap().clear();
        self.primed.store(false, Ordering::Relaxed);
        *self.to_client.lock().unwrap() = Some(tx);
        self.connected.store(true, Ordering::Release);
    }

    pub fn detach(&self) {
        self.connected.store(false, Ordering::Release);
        *self.to_client.lock().unwrap() = None;
        self.from_client.lock().unwrap().clear();
    }

    pub fn push_from_client(&self, samples: impl IntoIterator<Item = f32>) {
        self.from_client.lock().unwrap().extend(samples);
    }
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Arc<Channel>>>> = LazyLock::new(Default::default);

pub fn channel(name: &str) -> Arc<Channel> {
    REGISTRY.lock().unwrap().entry(name.to_string()).or_default().clone()
}

/// Connection state of every channel a filter or client has touched.
pub fn channels() -> Vec<(String, bool)> {
    let mut v: Vec<_> =
        REGISTRY.lock().unwrap().iter().map(|(k, c)| (k.clone(), c.connected.load(Ordering::Relaxed))).collect();
    v.sort();
    v
}

/// Port shown in the filter UI; set by the API server.
pub static API_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(4455);

struct Filter {
    channel: Arc<Channel>,
    channel_name: String,
    /// Target buffered latency, in interleaved samples.
    latency: usize,
    dry_when_disconnected: bool,
}

impl Filter {
    fn update(&mut self, settings: *mut sys::obs_data_t) {
        unsafe {
            let name = from_cstr(sys::obs_data_get_string(settings, c"channel".as_ptr()));
            let name = if name.trim().is_empty() { "voice".to_string() } else { name.trim().to_string() };
            if name != self.channel_name {
                self.channel = channel(&name);
                self.channel_name = name;
            }
            let ms = sys::obs_data_get_int(settings, c"latency_ms".as_ptr()).clamp(10, 1000) as usize;
            self.latency = ms * SAMPLE_RATE as usize / 1000 * CHANNELS;
            self.dry_when_disconnected = sys::obs_data_get_bool(settings, c"dry_when_disconnected".as_ptr());
        }
    }
}

unsafe extern "C" fn get_name(_: *mut c_void) -> *const c_char {
    c"Audio Bridge (external app)".as_ptr()
}

unsafe extern "C" fn create(settings: *mut sys::obs_data_t, _source: *mut sys::obs_source_t) -> *mut c_void {
    let mut f = Box::new(Filter {
        channel: channel("voice"),
        channel_name: String::new(),
        latency: 0,
        dry_when_disconnected: true,
    });
    f.update(settings);
    Box::into_raw(f) as *mut c_void
}

unsafe extern "C" fn destroy(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data as *mut Filter) });
}

unsafe extern "C" fn update(data: *mut c_void, settings: *mut sys::obs_data_t) {
    unsafe { (*(data as *mut Filter)).update(settings) }
}

unsafe extern "C" fn get_defaults(settings: *mut sys::obs_data_t) {
    unsafe {
        sys::obs_data_set_default_string(settings, c"channel".as_ptr(), c"voice".as_ptr());
        sys::obs_data_set_default_int(settings, c"latency_ms".as_ptr(), 60);
        sys::obs_data_set_default_bool(settings, c"dry_when_disconnected".as_ptr(), true);
    }
}

unsafe extern "C" fn get_properties(data: *mut c_void) -> *mut sys::obs_properties_t {
    unsafe {
        let props = sys::obs_properties_create();
        sys::obs_properties_add_text(props, c"channel".as_ptr(), c"Channel name".as_ptr(), sys::obs_text_type_OBS_TEXT_DEFAULT);
        sys::obs_properties_add_int(props, c"latency_ms".as_ptr(), c"Buffer (ms)".as_ptr(), 10, 1000, 10);
        sys::obs_properties_add_bool(
            props,
            c"dry_when_disconnected".as_ptr(),
            c"Pass original audio when no app is connected".as_ptr(),
        );
        let status = if data.is_null() {
            "Not connected".to_string()
        } else {
            let f = &*(data as *const Filter);
            let port = API_PORT.load(Ordering::Relaxed);
            let state = if f.channel.connected.load(Ordering::Relaxed) { "Connected" } else { "Waiting for app" };
            format!("{state} — ws://127.0.0.1:{port}/audio/{}", f.channel_name)
        };
        let s = cstr(&status);
        sys::obs_properties_add_text(props, c"status".as_ptr(), s.as_ptr(), sys::obs_text_type_OBS_TEXT_INFO);
        props
    }
}

unsafe extern "C" fn filter_audio(data: *mut c_void, audio: *mut sys::obs_audio_data) -> *mut sys::obs_audio_data {
    let f = unsafe { &*(data as *const Filter) };
    let a = unsafe { &mut *audio };
    let n = a.frames as usize;
    if a.data[0].is_null() || a.data[1].is_null() || n == 0 {
        return audio;
    }
    let (l, r) = unsafe {
        (
            std::slice::from_raw_parts_mut(a.data[0] as *mut f32, n),
            std::slice::from_raw_parts_mut(a.data[1] as *mut f32, n),
        )
    };
    let ch = &f.channel;
    if !ch.connected.load(Ordering::Acquire) {
        if !f.dry_when_disconnected {
            l.fill(0.0);
            r.fill(0.0);
        }
        return audio;
    }

    // Send (never block the audio thread; drop the block if the client is slow).
    let mut block = Vec::with_capacity(n * CHANNELS);
    for i in 0..n {
        block.push(l[i]);
        block.push(r[i]);
    }
    if let Ok(g) = ch.to_client.try_lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.try_send(block);
        }
    }

    // Receive: wait until `latency` is buffered, then play; re-prime after an underrun.
    let need = n * CHANNELS;
    let mut q = ch.from_client.lock().unwrap();
    if !ch.primed.load(Ordering::Relaxed) && q.len() >= f.latency + need {
        ch.primed.store(true, Ordering::Relaxed);
    }
    if ch.primed.load(Ordering::Relaxed) && q.len() >= need {
        for i in 0..n {
            l[i] = q.pop_front().unwrap_or(0.0);
            r[i] = q.pop_front().unwrap_or(0.0);
        }
    } else {
        ch.primed.store(false, Ordering::Relaxed);
        l.fill(0.0);
        r.fill(0.0);
    }
    // Keep latency bounded if the client sends faster than real time.
    let max = f.latency * 3 + need;
    if q.len() > max {
        let excess = (q.len() - max) / CHANNELS * CHANNELS;
        q.drain(..excess);
    }
    audio
}

/// Registers the filter type with libobs. Call once after startup.
pub fn register() {
    let id = Box::leak(Box::new(cstr(FILTER_ID)));
    unsafe {
        let mut info: sys::obs_source_info = std::mem::zeroed();
        info.id = id.as_ptr();
        info.type_ = sys::obs_source_type_OBS_SOURCE_TYPE_FILTER;
        info.output_flags = sys::OBS_SOURCE_AUDIO;
        info.get_name = Some(get_name);
        info.create = Some(create);
        info.destroy = Some(destroy);
        info.update = Some(update);
        info.get_defaults = Some(get_defaults);
        info.get_properties = Some(get_properties);
        info.filter_audio = Some(filter_audio);
        sys::obs_register_source_s(&info, std::mem::size_of::<sys::obs_source_info>());
    }
}
