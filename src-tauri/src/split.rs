//! Stream-only / recording-only sources.
//!
//! A source marked "stream only" or "recording only" carries a hidden video filter (the gate).
//! Recording and the replay buffer use a second video mix whose only source is the "record
//! program": it renders the normal program (channel 0, transitions included) with a
//! graphics-thread flag set, and the gate hides its source in whichever render it does not
//! belong to. Everything happens on the graphics thread, so the flag needs no locking.
//! Item visibility (the eye toggle) is untouched, and the main mix (preview, program, stream,
//! virtual camera) is the stream version.

use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU8, Ordering};

use crate::obs::{Source, cstr, from_cstr, sys};

pub const GATE_ID: &str = "streamtubah_output_gate";
const PROGRAM_ID: &str = "streamtubah_record_program";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShowIn {
    Both,
    Stream,
    Record,
}

impl ShowIn {
    pub fn as_str(self) -> &'static str {
        match self {
            ShowIn::Both => "both",
            ShowIn::Stream => "stream",
            ShowIn::Record => "record",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "both" => Some(ShowIn::Both),
            "stream" => Some(ShowIn::Stream),
            "record" => Some(ShowIn::Record),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        self as u8
    }
}

thread_local! {
    /// True while the record program is rendering (graphics thread only).
    static RENDERING_RECORD: Cell<bool> = const { Cell::new(false) };
}

/// Whether the gate draws its source in the current render.
fn gate_open(mode: u8, rendering_record: bool) -> bool {
    match mode {
        m if m == ShowIn::Stream.code() => !rendering_record,
        m if m == ShowIn::Record.code() => rendering_record,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Gate filter

struct Gate {
    ctx: *mut sys::obs_source_t,
    mode: AtomicU8,
}

fn read_mode(settings: *mut sys::obs_data_t) -> ShowIn {
    ShowIn::parse(&from_cstr(unsafe { sys::obs_data_get_string(settings, c"mode".as_ptr()) })).unwrap_or(ShowIn::Both)
}

unsafe extern "C" fn gate_name(_: *mut c_void) -> *const c_char {
    c"Show in (stream / recording)".as_ptr()
}

unsafe extern "C" fn gate_create(settings: *mut sys::obs_data_t, source: *mut sys::obs_source_t) -> *mut c_void {
    Box::into_raw(Box::new(Gate { ctx: source, mode: AtomicU8::new(read_mode(settings).code()) })) as *mut c_void
}

unsafe extern "C" fn gate_destroy(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data as *mut Gate) });
}

unsafe extern "C" fn gate_update(data: *mut c_void, settings: *mut sys::obs_data_t) {
    unsafe { (*(data as *const Gate)).mode.store(read_mode(settings).code(), Ordering::Relaxed) }
}

unsafe extern "C" fn gate_render(data: *mut c_void, _effect: *mut sys::gs_effect_t) {
    let g = unsafe { &*(data as *const Gate) };
    if gate_open(g.mode.load(Ordering::Relaxed), RENDERING_RECORD.with(Cell::get)) {
        unsafe { sys::obs_source_skip_video_filter(g.ctx) };
    }
}

// ---------------------------------------------------------------------------
// Record program source

unsafe extern "C" fn program_name(_: *mut c_void) -> *const c_char {
    c"Recording program".as_ptr()
}

unsafe extern "C" fn program_create(_: *mut sys::obs_data_t, _: *mut sys::obs_source_t) -> *mut c_void {
    // libobs treats a null data pointer as a failed create.
    Box::into_raw(Box::new(0u8)) as *mut c_void
}

unsafe extern "C" fn program_destroy(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data as *mut u8) });
}

fn canvas_size() -> (u32, u32) {
    let mut ovi: sys::obs_video_info = unsafe { std::mem::zeroed() };
    if unsafe { sys::obs_get_video_info(&mut ovi) } { (ovi.base_width, ovi.base_height) } else { (0, 0) }
}

unsafe extern "C" fn program_width(_: *mut c_void) -> u32 {
    canvas_size().0
}

unsafe extern "C" fn program_height(_: *mut c_void) -> u32 {
    canvas_size().1
}

unsafe extern "C" fn program_render(_: *mut c_void, _: *mut sys::gs_effect_t) {
    let program = unsafe { sys::obs_get_output_source(0) };
    if program.is_null() {
        return;
    }
    RENDERING_RECORD.with(|f| f.set(true));
    unsafe { sys::obs_source_video_render(program) };
    RENDERING_RECORD.with(|f| f.set(false));
    unsafe { sys::obs_source_release(program) };
}

/// Registers the gate filter and the record program source. Call once after startup,
/// before loading a scene collection (saved gates must find their type).
pub fn register() {
    unsafe {
        let id = Box::leak(Box::new(cstr(GATE_ID)));
        let mut info: sys::obs_source_info = std::mem::zeroed();
        info.id = id.as_ptr();
        info.type_ = sys::obs_source_type_OBS_SOURCE_TYPE_FILTER;
        // CAP_DISABLED keeps it out of the "add filter" list.
        info.output_flags = sys::OBS_SOURCE_VIDEO | sys::OBS_SOURCE_CAP_DISABLED;
        info.get_name = Some(gate_name);
        info.create = Some(gate_create);
        info.destroy = Some(gate_destroy);
        info.update = Some(gate_update);
        info.video_render = Some(gate_render);
        sys::obs_register_source_s(&info, std::mem::size_of::<sys::obs_source_info>());

        let id = Box::leak(Box::new(cstr(PROGRAM_ID)));
        let mut info: sys::obs_source_info = std::mem::zeroed();
        info.id = id.as_ptr();
        info.type_ = sys::obs_source_type_OBS_SOURCE_TYPE_INPUT;
        info.output_flags = sys::OBS_SOURCE_VIDEO | sys::OBS_SOURCE_CUSTOM_DRAW | sys::OBS_SOURCE_CAP_DISABLED;
        info.get_name = Some(program_name);
        info.create = Some(program_create);
        info.destroy = Some(program_destroy);
        info.get_width = Some(program_width);
        info.get_height = Some(program_height);
        info.video_render = Some(program_render);
        sys::obs_register_source_s(&info, std::mem::size_of::<sys::obs_source_info>());
    }
}

// ---------------------------------------------------------------------------
// Per-source mode

/// Gate filters of a source (normally zero or one).
fn gates(src: &Source) -> Vec<Source> {
    unsafe extern "C" fn cb(_p: *mut sys::obs_source_t, f: *mut sys::obs_source_t, data: *mut c_void) {
        if from_cstr(unsafe { sys::obs_source_get_id(f) }) == GATE_ID {
            let out = unsafe { &mut *(data as *mut Vec<*mut sys::obs_source_t>) };
            out.push(f);
        }
    }
    let mut raw: Vec<*mut sys::obs_source_t> = Vec::new();
    unsafe { sys::obs_source_enum_filters(src.as_ptr(), Some(cb), &mut raw as *mut _ as *mut c_void) };
    raw.into_iter().filter_map(|f| Source::from_raw(unsafe { sys::obs_source_get_ref(f) })).collect()
}

pub fn show_in(src: &Source) -> ShowIn {
    gates(src)
        .first()
        .map(|g| unsafe {
            let s = sys::obs_source_get_settings(g.as_ptr());
            let m = read_mode(s);
            sys::obs_data_release(s);
            m
        })
        .unwrap_or(ShowIn::Both)
}

/// Sets where a source appears. "Both" removes the gate.
pub fn set_show_in(src: &Source, mode: ShowIn) -> Result<(), String> {
    if unsafe { sys::obs_source_get_output_flags(src.as_ptr()) } & sys::OBS_SOURCE_VIDEO == 0 {
        return Err("Only video sources can be limited to the stream or the recording".into());
    }
    for g in gates(src) {
        unsafe { sys::obs_source_filter_remove(src.as_ptr(), g.as_ptr()) };
    }
    if mode == ShowIn::Both {
        return Ok(());
    }
    let data = crate::obs::Data::new();
    data.set_string("mode", mode.as_str());
    let gate = Source::create_private(GATE_ID, "Show in", Some(&data)).ok_or("Cannot create the stream/recording filter")?;
    unsafe {
        sys::obs_source_filter_add(src.as_ptr(), gate.as_ptr());
        // First in the chain: hiding happens before any other filter does work.
        sys::obs_source_filter_set_order(src.as_ptr(), gate.as_ptr(), sys::obs_order_movement_OBS_ORDER_MOVE_BOTTOM);
    }
    Ok(())
}

/// True when any source is limited to the stream or the recording.
pub fn any_split() -> bool {
    unsafe extern "C" fn cb(data: *mut c_void, src: *mut sys::obs_source_t) -> bool {
        let found = unsafe { &mut *(data as *mut bool) };
        if let Some(s) = Source::from_raw(unsafe { sys::obs_source_get_ref(src) }) {
            *found = show_in(&s) != ShowIn::Both;
        }
        !*found
    }
    let mut found = false;
    unsafe { sys::obs_enum_sources(Some(cb), &mut found as *mut _ as *mut c_void) };
    // Scenes are not part of obs_enum_sources but can carry filters too.
    if !found {
        unsafe { sys::obs_enum_scenes(Some(cb), &mut found as *mut _ as *mut c_void) };
    }
    found
}

// ---------------------------------------------------------------------------
// Second video mix for recording

/// A view rendering the record program into its own video output.
pub struct RecordView {
    view: *mut sys::obs_view_t,
    _program: Source,
    video: *mut sys::video_t,
}

// SAFETY: libobs views are thread safe; the pointers are owned by this struct.
unsafe impl Send for RecordView {}

impl RecordView {
    pub fn new() -> Result<Self, String> {
        let program = Source::create_private(PROGRAM_ID, "Recording program", None).ok_or("Cannot create the recording program")?;
        unsafe {
            let mut ovi: sys::obs_video_info = std::mem::zeroed();
            if !sys::obs_get_video_info(&mut ovi) {
                return Err("Video is not initialised".into());
            }
            let view = sys::obs_view_create();
            sys::obs_view_set_source(view, 0, program.as_ptr());
            let video = sys::obs_view_add2(view, &mut ovi);
            if video.is_null() {
                sys::obs_view_destroy(view);
                return Err("Cannot create the recording canvas".into());
            }
            Ok(Self { view, _program: program, video })
        }
    }

    pub fn video(&self) -> *mut sys::video_t {
        self.video
    }
}

impl Drop for RecordView {
    fn drop(&mut self) {
        unsafe {
            sys::obs_view_remove(self.view);
            sys::obs_view_set_source(self.view, 0, std::ptr::null_mut());
            sys::obs_view_destroy(self.view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_rules() {
        let (b, s, r) = (ShowIn::Both.code(), ShowIn::Stream.code(), ShowIn::Record.code());
        assert!(gate_open(b, false) && gate_open(b, true));
        assert!(gate_open(s, false) && !gate_open(s, true));
        assert!(!gate_open(r, false) && gate_open(r, true));
        for m in ["both", "stream", "record"] {
            assert_eq!(ShowIn::parse(m).unwrap().as_str(), m);
        }
    }
}
