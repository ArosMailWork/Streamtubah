//! Global audio channels (Desktop Audio, Mic/Aux). A channel can be closed
//! without losing its filters/volume: the source is serialized and recreated
//! on reopen. A closed channel never touches its audio device.

use serde_json::{Value, json};

use crate::obs::{Data, Source, sys};

pub struct AudioChannel {
    pub source: Option<Source>,
    saved: Value,
    channel: u32,
    name: &'static str,
    type_id: &'static str,
}

impl AudioChannel {
    pub fn new(channel: u32, name: &'static str, type_id: &'static str) -> Self {
        Self { source: None, saved: Value::Null, channel, name, type_id }
    }

    /// Restores serialized state; takes effect on the next open.
    pub fn load(&mut self, saved: Value) {
        self.saved = saved;
    }

    pub fn save(&self) -> Value {
        match &self.source {
            Some(s) => Data::from_raw(unsafe { sys::obs_save_source(s.as_ptr()) }).map_or(Value::Null, |d| d.to_json()),
            None => self.saved.clone(),
        }
    }

    /// `Some(device_id)` opens (or retargets) the channel; `None` closes it.
    pub fn set_device(&mut self, device: Option<&str>) {
        match device {
            None => self.close(),
            Some(id) => self.open(id),
        }
    }

    fn open(&mut self, device_id: &str) {
        if self.source.is_none() {
            let source = if self.saved.is_object() {
                let mut saved = self.saved.clone();
                // Apply the new device before the source opens anything.
                saved["settings"]["device_id"] = json!(device_id);
                Source::from_raw(unsafe { sys::obs_load_private_source(Data::from_json(&saved).as_ptr()) })
            } else {
                let data = Data::new();
                data.set_string("device_id", device_id);
                Source::create_private(self.type_id, self.name, Some(&data))
            };
            unsafe { sys::obs_set_output_source(self.channel, source.as_ref().map_or(std::ptr::null_mut(), |s| s.as_ptr())) };
            self.source = source;
            return;
        }
        let source = self.source.as_ref().unwrap();
        if source.settings().to_json()["device_id"] != json!(device_id) {
            let patch = Data::new();
            patch.set_string("device_id", device_id);
            source.update(&patch);
        }
    }

    fn close(&mut self) {
        if self.source.is_some() {
            self.saved = self.save();
            unsafe { sys::obs_set_output_source(self.channel, std::ptr::null_mut()) };
            self.source = None;
        }
    }
}
