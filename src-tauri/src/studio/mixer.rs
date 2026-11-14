//! Audio level meters. libobs volmeter callbacks fire on the audio thread at
//! a high rate; they only store peaks into atomics. A UI timer drains them.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::obs::{Source, sys};

const CHANNELS: usize = 2;
const SILENCE_DB: f32 = -100.0;

struct MeterState {
    peak: [AtomicU32; CHANNELS],
}

impl MeterState {
    fn new() -> Self {
        Self { peak: std::array::from_fn(|_| AtomicU32::new(SILENCE_DB.to_bits())) }
    }

    /// Returns the highest peak since the last call and resets it.
    fn take(&self) -> [f32; CHANNELS] {
        std::array::from_fn(|i| f32::from_bits(self.peak[i].swap(SILENCE_DB.to_bits(), Ordering::Relaxed)))
    }
}

unsafe extern "C" fn on_levels(param: *mut c_void, _mag: *const f32, peak: *const f32, _input: *const f32) {
    // SAFETY: param is the boxed MeterState, alive until the callback is removed.
    let state = unsafe { &*(param as *const MeterState) };
    let peaks = unsafe { std::slice::from_raw_parts(peak, sys::MAX_AUDIO_CHANNELS as usize) };
    for (i, slot) in state.peak.iter().enumerate() {
        let v = if peaks[i].is_finite() { peaks[i] } else { SILENCE_DB };
        let _ = slot.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
            (v > f32::from_bits(old)).then_some(v.to_bits())
        });
    }
}

struct Meter {
    volmeter: *mut sys::obs_volmeter_t,
    state: Box<MeterState>,
    _source: Source,
}

impl Drop for Meter {
    fn drop(&mut self) {
        unsafe {
            let p = &*self.state as *const MeterState as *mut c_void;
            sys::obs_volmeter_remove_callback(self.volmeter, Some(on_levels), p);
            sys::obs_volmeter_destroy(self.volmeter);
        }
    }
}

// SAFETY: the volmeter pointer is only touched from Drop and libobs locks internally.
unsafe impl Send for Meter {}

#[derive(Default)]
pub struct Mixer {
    meters: HashMap<String, Meter>,
}

impl Mixer {
    /// Makes the set of meters match `sources` (keyed by source name).
    pub fn sync(&mut self, sources: &[Source]) {
        let wanted: HashMap<String, &Source> = sources.iter().map(|s| (s.name(), s)).collect();
        self.meters.retain(|name, m| wanted.get(name).is_some_and(|s| s.as_ptr() == m._source.as_ptr()));
        for (name, source) in wanted {
            if self.meters.contains_key(&name) {
                continue;
            }
            unsafe {
                let volmeter = sys::obs_volmeter_create(sys::obs_fader_type_OBS_FADER_LOG);
                let state = Box::new(MeterState::new());
                sys::obs_volmeter_attach_source(volmeter, source.as_ptr());
                let p = &*state as *const MeterState as *mut c_void;
                sys::obs_volmeter_add_callback(volmeter, Some(on_levels), p);
                self.meters.insert(name, Meter { volmeter, state, _source: source.clone() });
            }
        }
    }

    /// Peak dBFS per channel for every metered source.
    pub fn levels(&self) -> HashMap<String, [f32; CHANNELS]> {
        self.meters.iter().map(|(k, m)| (k.clone(), m.state.take())).collect()
    }

    pub fn clear(&mut self) {
        self.meters.clear();
    }
}
