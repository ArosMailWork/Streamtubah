//! Thin safe layer over the raw `libobs` FFI bindings.
//!
//! Every strong libobs reference is owned by an RAII wrapper that releases it
//! on drop. libobs reference counting is thread-safe, so the wrappers are
//! `Send + Sync`.

pub mod data;
pub mod properties;
pub mod runtime;
pub mod signals;

use std::ffi::{CStr, CString, c_char};

pub use libobs as sys;

pub use data::Data;

/// Converts a Rust string into a C string, stripping interior NULs.
pub fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Copies a borrowed C string into an owned Rust string (empty on NULL).
pub fn from_cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: libobs returns valid NUL-terminated strings or NULL.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

// Helpers exported by obs.dll that the generated bindings do not include.
#[link(name = "obs")]
unsafe extern "C" {
    pub fn os_generate_formatted_filename(
        extension: *const c_char,
        space: bool,
        format: *const c_char,
    ) -> *mut c_char;
    pub fn os_cpu_usage_info_start() -> *mut std::ffi::c_void;
    pub fn os_cpu_usage_info_query(info: *mut std::ffi::c_void) -> f64;
}

macro_rules! obs_ref {
    ($name:ident, $raw:ty, $get_ref:path, $release:path) => {
        pub struct $name(*mut $raw);

        // SAFETY: libobs objects are internally synchronized and ref-counted atomically.
        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}

        impl $name {
            /// Takes ownership of an already-strong reference. Returns None for NULL.
            pub fn from_raw(ptr: *mut $raw) -> Option<Self> {
                (!ptr.is_null()).then_some(Self(ptr))
            }
            pub fn as_ptr(&self) -> *mut $raw {
                self.0
            }
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                // SAFETY: self.0 is a valid strong reference.
                Self(unsafe { $get_ref(self.0) })
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: we own exactly one strong reference.
                unsafe { $release(self.0) }
            }
        }
    };
}

obs_ref!(Source, sys::obs_source_t, sys::obs_source_get_ref, sys::obs_source_release);
obs_ref!(Output, sys::obs_output_t, sys::obs_output_get_ref, sys::obs_output_release);
obs_ref!(Encoder, sys::obs_encoder_t, sys::obs_encoder_get_ref, sys::obs_encoder_release);
obs_ref!(Service, sys::obs_service_t, sys::obs_service_get_ref, sys::obs_service_release);

impl Source {
    /// Looks up a public source by name (returns a new strong reference).
    pub fn by_name(name: &str) -> Option<Self> {
        let c = cstr(name);
        Self::from_raw(unsafe { sys::obs_get_source_by_name(c.as_ptr()) })
    }

    pub fn create(type_id: &str, name: &str, settings: Option<&Data>) -> Option<Self> {
        let (t, n) = (cstr(type_id), cstr(name));
        let s = settings.map_or(std::ptr::null_mut(), |d| d.as_ptr());
        Self::from_raw(unsafe { sys::obs_source_create(t.as_ptr(), n.as_ptr(), s, std::ptr::null_mut()) })
    }

    pub fn create_private(type_id: &str, name: &str, settings: Option<&Data>) -> Option<Self> {
        let (t, n) = (cstr(type_id), cstr(name));
        let s = settings.map_or(std::ptr::null_mut(), |d| d.as_ptr());
        Self::from_raw(unsafe { sys::obs_source_create_private(t.as_ptr(), n.as_ptr(), s) })
    }

    pub fn name(&self) -> String {
        from_cstr(unsafe { sys::obs_source_get_name(self.0) })
    }

    pub fn id(&self) -> String {
        from_cstr(unsafe { sys::obs_source_get_id(self.0) })
    }

    pub fn output_flags(&self) -> u32 {
        unsafe { sys::obs_source_get_output_flags(self.0) }
    }

    pub fn settings(&self) -> Data {
        Data::from_raw(unsafe { sys::obs_source_get_settings(self.0) }).unwrap_or_else(Data::new)
    }

    pub fn update(&self, patch: &Data) {
        unsafe { sys::obs_source_update(self.0, patch.as_ptr()) }
    }

    /// Returns the scene pointer if this source is a scene (borrowed, not a new ref).
    pub fn as_scene(&self) -> *mut sys::obs_scene_t {
        unsafe { sys::obs_scene_from_source(self.0) }
    }
}

impl Service {
    pub fn settings(&self) -> Data {
        Data::from_raw(unsafe { sys::obs_service_get_settings(self.0) }).unwrap_or_else(Data::new)
    }
}

impl Output {
    pub fn active(&self) -> bool {
        unsafe { sys::obs_output_active(self.0) }
    }

    pub fn last_error(&self) -> Option<String> {
        let p = unsafe { sys::obs_output_get_last_error(self.0) };
        (!p.is_null()).then(|| from_cstr(p)).filter(|s| !s.is_empty())
    }
}

/// Column-major 4x4 matrix with the same layout as libobs `struct matrix4`.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug)]
pub struct Matrix4 {
    pub x: [f32; 4],
    pub y: [f32; 4],
    pub z: [f32; 4],
    pub t: [f32; 4],
}

impl Matrix4 {
    /// Applies the affine part of the matrix to a 2D point.
    pub fn apply(&self, u: f32, v: f32) -> (f32, f32) {
        (
            u * self.x[0] + v * self.y[0] + self.t[0],
            u * self.x[1] + v * self.y[1] + self.t[1],
        )
    }

    /// Inverse-maps a point back to unit-box space. None if degenerate.
    pub fn unapply(&self, px: f32, py: f32) -> Option<(f32, f32)> {
        let (a, b, c, d) = (self.x[0], self.x[1], self.y[0], self.y[1]);
        let det = a * d - b * c;
        if det.abs() < 1e-6 {
            return None;
        }
        let (dx, dy) = (px - self.t[0], py - self.t[1]);
        Some(((d * dx - c * dy) / det, (-b * dx + a * dy) / det))
    }
}

pub fn vec2(x: f32, y: f32) -> sys::vec2 {
    sys::vec2 { __bindgen_anon_1: sys::vec2__bindgen_ty_1 { ptr: [x, y] } }
}

pub fn vec2_get(v: &sys::vec2) -> (f32, f32) {
    // SAFETY: both union views are plain f32 pairs.
    let p = unsafe { v.__bindgen_anon_1.ptr };
    (p[0], p[1])
}
