//! Advanced audio properties (OBS "Advanced Audio Properties"): sync offset, balance,
//! mono, output tracks, and hiding a source from the mixer.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Result, Studio};
use crate::obs::{Data, Source, sys};

const HIDDEN_KEY: &str = "mixer_hidden";

#[derive(Deserialize, Default)]
pub struct AudioPatch {
    pub sync_offset_ms: Option<i64>,
    /// 0.0 = left, 0.5 = center, 1.0 = right.
    pub balance: Option<f32>,
    pub mono: Option<bool>,
    /// Bit mask of output tracks 1-6.
    pub tracks: Option<u32>,
    pub hidden: Option<bool>,
}

pub fn hidden(s: &Source) -> bool {
    Data::from_raw(unsafe { sys::obs_source_get_private_settings(s.as_ptr()) })
        .is_some_and(|d| d.to_json()[HIDDEN_KEY] == json!(true))
}

/// Fields added to each mixer entry.
pub fn info(s: &Source) -> Value {
    unsafe {
        let p = s.as_ptr();
        json!({
            "sync_offset_ms": sys::obs_source_get_sync_offset(p) / 1_000_000,
            "balance": sys::obs_source_get_balance_value(p),
            "mono": sys::obs_source_get_flags(p) & sys::OBS_SOURCE_FLAG_FORCE_MONO != 0,
            "tracks": sys::obs_source_get_audio_mixers(p),
            "hidden": hidden(s),
        })
    }
}

impl Studio {
    pub fn audio_set_advanced(&mut self, name: &str, patch: &AudioPatch) -> Result<()> {
        let s = self.find_source(name)?;
        let p = s.as_ptr();
        unsafe {
            if let Some(ms) = patch.sync_offset_ms {
                sys::obs_source_set_sync_offset(p, ms.clamp(-950, 20_000) * 1_000_000);
            }
            if let Some(b) = patch.balance {
                sys::obs_source_set_balance_value(p, b.clamp(0.0, 1.0));
            }
            if let Some(mono) = patch.mono {
                let flags = sys::obs_source_get_flags(p);
                let flags = if mono { flags | sys::OBS_SOURCE_FLAG_FORCE_MONO } else { flags & !sys::OBS_SOURCE_FLAG_FORCE_MONO };
                sys::obs_source_set_flags(p, flags);
            }
            if let Some(t) = patch.tracks {
                sys::obs_source_set_audio_mixers(p, t & 0x3F);
            }
            if let Some(h) = patch.hidden {
                if let Some(d) = Data::from_raw(sys::obs_source_get_private_settings(p)) {
                    d.set_bool(HIDDEN_KEY, h);
                }
            }
        }
        self.touch();
        self.emit("studio:changed", Value::Null);
        Ok(())
    }
}
