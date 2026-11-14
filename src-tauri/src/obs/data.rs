//! `obs_data_t` wrapper. Settings travel between Rust/JS and libobs as JSON,
//! which keeps conversion code to a single round-trip through libobs' own parser.

use super::{cstr, from_cstr, sys};

pub struct Data(*mut sys::obs_data_t);

// SAFETY: obs_data is ref-counted atomically; we never share mutable access across threads.
unsafe impl Send for Data {}
unsafe impl Sync for Data {}

impl Data {
    pub fn new() -> Self {
        Self(unsafe { sys::obs_data_create() })
    }

    pub fn from_raw(ptr: *mut sys::obs_data_t) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }

    pub fn from_json(value: &serde_json::Value) -> Self {
        if !value.is_object() {
            return Self::new();
        }
        let s = cstr(&value.to_string());
        Self::from_raw(unsafe { sys::obs_data_create_from_json(s.as_ptr()) }).unwrap_or_else(Self::new)
    }

    pub fn as_ptr(&self) -> *mut sys::obs_data_t {
        self.0
    }

    /// User-set values only.
    pub fn to_json(&self) -> serde_json::Value {
        parse(from_cstr(unsafe { sys::obs_data_get_json(self.0) }))
    }

    /// User-set values merged over defaults; what a properties form should display.
    pub fn to_json_with_defaults(&self) -> serde_json::Value {
        parse(from_cstr(unsafe { sys::obs_data_get_json_with_defaults(self.0) }))
    }

    pub fn set_string(&self, key: &str, value: &str) {
        let (k, v) = (cstr(key), cstr(value));
        unsafe { sys::obs_data_set_string(self.0, k.as_ptr(), v.as_ptr()) }
    }

    pub fn set_bool(&self, key: &str, value: bool) {
        let k = cstr(key);
        unsafe { sys::obs_data_set_bool(self.as_ptr(), k.as_ptr(), value) }
    }

    pub fn set_int(&self, key: &str, value: i64) {
        let k = cstr(key);
        unsafe { sys::obs_data_set_int(self.0, k.as_ptr(), value) }
    }

    pub fn apply(&self, patch: &Data) {
        unsafe { sys::obs_data_apply(self.0, patch.0) }
    }
}

impl Clone for Data {
    fn clone(&self) -> Self {
        unsafe { sys::obs_data_addref(self.0) };
        Self(self.0)
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        unsafe { sys::obs_data_release(self.0) }
    }
}

fn parse(s: String) -> serde_json::Value {
    serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}))
}
