//! Native displays: Win32 child windows layered over the WebView. libobs
//! renders into them via `obs_display`. A display shows either the program
//! output (main texture) or one scene (Studio Mode preview). Editable displays
//! also draw selection boxes and handle mouse editing of scene items, because
//! HTML cannot be drawn on top of a native child window.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DELETE, VK_DOWN, VK_LEFT, VK_RIGHT,
    VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HCURSOR, HTCLIENT,
    HWND_TOP, IDC_ARROW, SetWindowLongPtrW,
    IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW, RegisterClassExW, SW_HIDE,
    SW_SHOWNA, SWP_NOACTIVATE, SetCursor, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows::core::w;

use crate::obs::{Matrix4, Source, runtime, sys, vec2, vec2_get};

/// Per-display state shared between the UI thread (window proc) and the graphics thread (draw callback).
pub struct PreviewShared {
    /// Scene shown (when not rendering program) and/or edited.
    pub scene: Mutex<Option<Source>>,
    /// Render the program output (main texture) instead of `scene`.
    program: AtomicBool,
    /// Mouse/keyboard editing and selection boxes.
    editable: AtomicBool,
    pub dirty: Arc<AtomicBool>,
    app: AppHandle,
}

impl PreviewShared {
    fn scene(&self) -> Option<Source> {
        self.scene.lock().ok()?.clone()
    }

    fn changed(&self) {
        let _ = self.app.emit("studio:changed", ());
    }
}

pub struct Preview {
    hwnd: HWND,
    display: *mut sys::obs_display_t,
    shared: Arc<PreviewShared>,
}

// SAFETY: Preview is only created/destroyed on the main thread; the raw handles are plain ids.
unsafe impl Send for Preview {}

impl Preview {
    pub fn create(parent: isize, app: AppHandle, dirty: Arc<AtomicBool>) -> Result<Self, String> {
        let shared = Arc::new(PreviewShared {
            scene: Mutex::new(None),
            program: AtomicBool::new(true),
            editable: AtomicBool::new(true),
            dirty,
            app,
        });
        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: w!("StreamtubahPreview"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("StreamtubahPreview"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut c_void)),
                None,
                Some(hinstance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowEx failed: {e}"))?;
            // The window proc finds its display through this pointer; the Arc outlives the window (see Drop).
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::as_ptr(&shared) as isize);
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 1, 1, SWP_NOACTIVATE);

            let init = sys::gs_init_data {
                window: sys::gs_window { hwnd: hwnd.0 },
                cx: 1,
                cy: 1,
                num_backbuffers: 0,
                format: sys::gs_color_format_GS_BGRA,
                zsformat: sys::gs_zstencil_format_GS_ZS_NONE,
                adapter: 0,
            };
            let display = sys::obs_display_create(&init, 0xFF0E0E12);
            if display.is_null() {
                let _ = DestroyWindow(hwnd);
                return Err("obs_display_create failed".into());
            }
            let param = Arc::as_ptr(&shared) as *mut c_void;
            sys::obs_display_add_draw_callback(display, Some(draw), param);
            Ok(Self { hwnd, display, shared })
        }
    }

    pub fn set_scene(&self, scene: Option<Source>) {
        if let Ok(mut s) = self.shared.scene.lock() {
            *s = scene;
        }
    }

    /// `program`: show the program output; otherwise show `scene`. `editable`: allow item editing.
    pub fn set_mode(&self, program: bool, editable: bool) {
        self.shared.program.store(program, Ordering::Relaxed);
        self.shared.editable.store(editable, Ordering::Relaxed);
    }

    /// Bounds are physical pixels relative to the main window's client area.
    pub fn set_bounds(&self, x: i32, y: i32, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOP), x, y, w as i32, h as i32, SWP_NOACTIVATE);
            sys::obs_display_resize(self.display, w, h);
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
            sys::obs_display_set_enabled(self.display, visible);
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        unsafe {
            let param = Arc::as_ptr(&self.shared) as *mut c_void;
            sys::obs_display_remove_draw_callback(self.display, Some(draw), param);
            sys::obs_display_destroy(self.display);
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
        self.set_scene(None);
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers

/// Letterboxed placement of the canvas inside the preview window.
#[derive(Clone, Copy)]
struct Viewport {
    x: f32,
    y: f32,
    scale: f32,
    w: f32,
    h: f32,
}

impl Viewport {
    fn fit(cx: u32, cy: u32) -> Self {
        let (bw, bh) = runtime::canvas_size();
        let scale = (cx as f32 / bw as f32).min(cy as f32 / bh as f32).max(0.0001);
        let (w, h) = (bw as f32 * scale, bh as f32 * scale);
        Self { x: (cx as f32 - w) / 2.0, y: (cy as f32 - h) / 2.0, scale, w, h }
    }

    fn to_scene(&self, px: f32, py: f32) -> (f32, f32) {
        ((px - self.x) / self.scale, (py - self.y) / self.scale)
    }
}

const HANDLES: [(f32, f32); 8] =
    [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)];

struct ItemGeom {
    item: *mut sys::obs_sceneitem_t,
    id: i64,
    box_tf: Matrix4,
    selected: bool,
    locked: bool,
    visible: bool,
}

impl ItemGeom {
    fn rect(&self) -> (f32, f32, f32, f32) {
        let pts = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(u, v)| self.box_tf.apply(u, v));
        let xs = pts.map(|p| p.0);
        let ys = pts.map(|p| p.1);
        let min = |a: [f32; 4]| a.into_iter().fold(f32::MAX, f32::min);
        let max = |a: [f32; 4]| a.into_iter().fold(f32::MIN, f32::max);
        (min(xs), min(ys), max(xs), max(ys))
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        self.box_tf.unapply(x, y).is_some_and(|(u, v)| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v))
    }

    /// Resizing is supported for axis-aligned items (rotation a multiple of 90°, flips allowed).
    fn resizable(&self) -> bool {
        unsafe {
            let mut s = vec2(0.0, 0.0);
            sys::obs_sceneitem_get_scale(self.item, &mut s);
            let (sx, sy) = vec2_get(&s);
            let rot = sys::obs_sceneitem_get_rot(self.item).rem_euclid(90.0);
            !self.locked && (rot < 0.01 || rot > 89.99) && sx != 0.0 && sy != 0.0
        }
    }
}

/// Collects geometry of all items, bottom to top. Item pointers are borrowed
/// from the scene and are only valid while `_scene` is alive.
fn items(scene: &Source) -> Vec<ItemGeom> {
    unsafe extern "C" fn cb(_s: *mut sys::obs_scene_t, item: *mut sys::obs_sceneitem_t, p: *mut c_void) -> bool {
        let out = unsafe { &mut *(p as *mut Vec<ItemGeom>) };
        let mut box_tf = Matrix4::default();
        unsafe {
            sys::obs_sceneitem_get_box_transform(item, &mut box_tf as *mut Matrix4 as *mut sys::matrix4);
            out.push(ItemGeom {
                item,
                id: sys::obs_sceneitem_get_id(item),
                box_tf,
                selected: sys::obs_sceneitem_selected(item),
                locked: sys::obs_sceneitem_locked(item),
                visible: sys::obs_sceneitem_visible(item),
            });
        }
        true
    }
    let mut out: Vec<ItemGeom> = Vec::new();
    unsafe { sys::obs_scene_enum_items(scene.as_scene(), Some(cb), &mut out as *mut _ as *mut c_void) };
    out
}

// ---------------------------------------------------------------------------
// Rendering (graphics thread)

unsafe extern "C" fn draw(param: *mut c_void, cx: u32, cy: u32) {
    let shared = unsafe { &*(param as *const PreviewShared) };
    let vp = Viewport::fit(cx, cy);
    let (bw, bh) = runtime::canvas_size();
    unsafe {
        sys::gs_viewport_push();
        sys::gs_projection_push();
        sys::gs_ortho(0.0, bw as f32, 0.0, bh as f32, -100.0, 100.0);
        sys::gs_set_viewport(vp.x as i32, vp.y as i32, vp.w as i32, vp.h as i32);
        let scene = shared.scene();
        if shared.program.load(Ordering::Relaxed) {
            sys::obs_render_main_texture();
        } else if let Some(s) = &scene {
            sys::obs_source_video_render(s.as_ptr());
        }
        if let Some(scene) = scene.filter(|_| shared.editable.load(Ordering::Relaxed)) {
            draw_selection(&scene, vp.scale);
        }
        sys::gs_projection_pop();
        sys::gs_viewport_pop();
    }
}

unsafe fn draw_selection(scene: &Source, scale: f32) {
    let geoms = items(scene);
    if !geoms.iter().any(|g| g.selected) {
        return;
    }
    unsafe {
        let solid = sys::obs_get_base_effect(sys::obs_base_effect_OBS_EFFECT_SOLID);
        let color = sys::gs_effect_get_param_by_name(solid, c"color".as_ptr());
        let tech = sys::gs_effect_get_technique(solid, c"Solid".as_ptr());
        sys::gs_technique_begin(tech);
        sys::gs_technique_begin_pass(tech, 0);
        for g in geoms.iter().filter(|g| g.selected) {
            sys::gs_effect_set_color(color, if g.locked { 0xFFE5484D } else { 0xFF3B82F6 });
            sys::gs_render_start(true);
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)] {
                let (x, y) = g.box_tf.apply(u, v);
                sys::gs_vertex2f(x, y);
            }
            sys::gs_render_stop(sys::gs_draw_mode_GS_LINESTRIP);
            if g.locked {
                continue;
            }
            let r = 4.0 / scale;
            for (u, v) in HANDLES {
                let (x, y) = g.box_tf.apply(u, v);
                sys::gs_render_start(true);
                sys::gs_vertex2f(x - r, y - r);
                sys::gs_vertex2f(x + r, y - r);
                sys::gs_vertex2f(x - r, y + r);
                sys::gs_vertex2f(x + r, y + r);
                sys::gs_render_stop(sys::gs_draw_mode_GS_TRISTRIP);
            }
        }
        sys::gs_technique_end_pass(tech);
        sys::gs_technique_end(tech);
    }
}

// ---------------------------------------------------------------------------
// Mouse / keyboard editing (UI thread)

enum Drag {
    Move { id: i64, start: (f32, f32), start_pos: (f32, f32), start_rect: (f32, f32, f32, f32) },
    Resize { id: i64, handle: (f32, f32), start_rect: (f32, f32, f32, f32) },
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
    static CURSOR: RefCell<windows::core::PCWSTR> = const { RefCell::new(IDC_ARROW) };
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r);
    }
    ((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32)
}

fn mouse_pos(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x, y)
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}

fn handle_at(g: &ItemGeom, x: f32, y: f32, tol: f32) -> Option<(f32, f32)> {
    HANDLES.into_iter().find(|&(u, v)| {
        let (hx, hy) = g.box_tf.apply(u, v);
        (hx - x).abs() <= tol && (hy - y).abs() <= tol
    })
}

fn cursor_for_handle(h: (f32, f32)) -> windows::core::PCWSTR {
    match h {
        (0.5, _) => IDC_SIZENS,
        (_, 0.5) => IDC_SIZEWE,
        (0.0, 0.0) | (1.0, 1.0) => IDC_SIZENWSE,
        _ => IDC_SIZENESW,
    }
}

fn find_item(scene: &Source, id: i64) -> *mut sys::obs_sceneitem_t {
    unsafe { sys::obs_scene_find_sceneitem_by_id(scene.as_scene(), id) }
}

fn on_mouse_down(shared: &PreviewShared, hwnd: HWND, lparam: LPARAM) {
    let Some(scene) = shared.scene() else { return };
    let (cx, cy) = client_size(hwnd);
    let vp = Viewport::fit(cx, cy);
    let (mx, my) = mouse_pos(lparam);
    let (x, y) = vp.to_scene(mx, my);
    let tol = 6.0 / vp.scale;
    let geoms = items(&scene);

    // 1) Resize handle of an already-selected item.
    for g in geoms.iter().rev().filter(|g| g.selected && g.resizable()) {
        if let Some(h) = handle_at(g, x, y, tol) {
            DRAG.with(|d| *d.borrow_mut() = Some(Drag::Resize { id: g.id, handle: h, start_rect: g.rect() }));
            return;
        }
    }

    // 2) Topmost visible item under the cursor.
    let hit = geoms.iter().rev().find(|g| g.visible && g.contains(x, y));
    let ctrl = key_down(VK_CONTROL.0);
    unsafe {
        if !ctrl {
            for g in &geoms {
                if hit.is_none_or(|h| h.id != g.id) {
                    sys::obs_sceneitem_select(g.item, false);
                }
            }
        }
        if let Some(g) = hit {
            sys::obs_sceneitem_select(g.item, if ctrl { !g.selected } else { true });
            if !g.locked {
                let mut p = vec2(0.0, 0.0);
                sys::obs_sceneitem_get_pos(g.item, &mut p);
                DRAG.with(|d| {
                    *d.borrow_mut() =
                        Some(Drag::Move { id: g.id, start: (x, y), start_pos: vec2_get(&p), start_rect: g.rect() })
                });
            }
        }
    }
    shared.changed();
}

/// Snaps a moved rect's edges/center to the canvas edges/center.
fn snap(rect: (f32, f32, f32, f32), tol: f32) -> (f32, f32) {
    let (bw, bh) = runtime::canvas_size();
    let (bw, bh) = (bw as f32, bh as f32);
    let axis = |lo: f32, hi: f32, size: f32| -> f32 {
        let mid = (lo + hi) / 2.0;
        [(0.0 - lo), (size - hi), (size / 2.0 - mid)]
            .into_iter()
            .filter(|d| d.abs() <= tol)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(0.0)
    };
    (axis(rect.0, rect.2, bw), axis(rect.1, rect.3, bh))
}

fn on_mouse_move(shared: &PreviewShared, hwnd: HWND, lparam: LPARAM) {
    let Some(scene) = shared.scene() else { return };
    let (cx, cy) = client_size(hwnd);
    let vp = Viewport::fit(cx, cy);
    let (mx, my) = mouse_pos(lparam);
    let (x, y) = vp.to_scene(mx, my);
    let tol = 6.0 / vp.scale;

    let dragging = DRAG.with(|d| {
        let d = d.borrow();
        let Some(drag) = d.as_ref() else { return false };
        match *drag {
            Drag::Move { id, start, start_pos, start_rect } => {
                let item = find_item(&scene, id);
                if item.is_null() {
                    return true;
                }
                let (mut dx, mut dy) = (x - start.0, y - start.1);
                if !key_down(VK_CONTROL.0) {
                    let moved = (start_rect.0 + dx, start_rect.1 + dy, start_rect.2 + dx, start_rect.3 + dy);
                    let (sx, sy) = snap(moved, 10.0 / vp.scale);
                    dx += sx;
                    dy += sy;
                }
                unsafe { sys::obs_sceneitem_set_pos(item, &vec2(start_pos.0 + dx, start_pos.1 + dy)) };
            }
            Drag::Resize { id, handle, start_rect } => {
                let item = find_item(&scene, id);
                if !item.is_null() {
                    resize(item, handle, start_rect, (x, y), !key_down(VK_SHIFT.0));
                }
            }
        }
        true
    });
    if dragging {
        return;
    }

    // Hover cursor feedback.
    let geoms = items(&scene);
    let cursor = geoms
        .iter()
        .rev()
        .filter(|g| g.selected && g.resizable())
        .find_map(|g| handle_at(g, x, y, tol).map(cursor_for_handle))
        .or_else(|| geoms.iter().rev().find(|g| g.visible && !g.locked && g.contains(x, y)).map(|_| IDC_SIZEALL))
        .unwrap_or(IDC_ARROW);
    CURSOR.with(|c| *c.borrow_mut() = cursor);
}

/// Applies a handle drag to an axis-aligned item, anchoring the opposite edge.
fn resize(item: *mut sys::obs_sceneitem_t, handle: (f32, f32), r0: (f32, f32, f32, f32), p: (f32, f32), keep_aspect: bool) {
    let (mut l, mut t, mut r, mut b) = r0;
    let (w0, h0) = ((r0.2 - r0.0).max(1.0), (r0.3 - r0.1).max(1.0));
    match handle.0 {
        0.0 => l = p.0.min(r - 1.0),
        1.0 => r = p.0.max(l + 1.0),
        _ => {}
    }
    match handle.1 {
        0.0 => t = p.1.min(b - 1.0),
        1.0 => b = p.1.max(t + 1.0),
        _ => {}
    }
    let corner = handle.0 != 0.5 && handle.1 != 0.5;
    if corner && keep_aspect {
        let (w, h) = (r - l, b - t);
        let (w, h) = if w / w0 > h / h0 { (w, w * h0 / w0) } else { (h * w0 / h0, h) };
        if handle.0 == 0.0 { l = r - w } else { r = l + w }
        if handle.1 == 0.0 { t = b - h } else { b = t + h }
    }
    let (w, h) = (r - l, b - t);

    unsafe {
        // At 90°/270° the item's own width runs along the screen's vertical axis.
        let quarter = (sys::obs_sceneitem_get_rot(item).rem_euclid(180.0) - 90.0).abs() < 1.0;
        let (iw, ih) = if quarter { (h, w) } else { (w, h) };
        if sys::obs_sceneitem_get_bounds_type(item) != sys::obs_bounds_type_OBS_BOUNDS_NONE {
            sys::obs_sceneitem_set_bounds(item, &vec2(iw, ih));
        } else {
            let src = sys::obs_sceneitem_get_source(item);
            let mut crop = sys::obs_sceneitem_crop { left: 0, top: 0, right: 0, bottom: 0 };
            sys::obs_sceneitem_get_crop(item, &mut crop);
            let sw = (sys::obs_source_get_width(src) as i32 - crop.left - crop.right).max(1) as f32;
            let sh = (sys::obs_source_get_height(src) as i32 - crop.top - crop.bottom).max(1) as f32;
            let mut s = vec2(0.0, 0.0);
            sys::obs_sceneitem_get_scale(item, &mut s);
            let (sx, sy) = vec2_get(&s);
            // Keep flips: only the magnitude follows the drag.
            sys::obs_sceneitem_set_scale(item, &vec2(sx.signum() * iw / sw, sy.signum() * ih / sh));
        }
        // Whatever the alignment, rotation or flip: move so the box's top-left lands on (l, t).
        sys::obs_sceneitem_force_update_transform(item);
        let mut m = Matrix4::default();
        sys::obs_sceneitem_get_box_transform(item, &mut m as *mut Matrix4 as *mut sys::matrix4);
        let (cl, ct) = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .map(|(u, v)| m.apply(u, v))
            .into_iter()
            .fold((f32::MAX, f32::MAX), |(ax, ay), (x, y)| (ax.min(x), ay.min(y)));
        let mut pos = vec2(0.0, 0.0);
        sys::obs_sceneitem_get_pos(item, &mut pos);
        let (px, py) = vec2_get(&pos);
        sys::obs_sceneitem_set_pos(item, &vec2(px + l - cl, py + t - ct));
    }
}

fn on_mouse_up(shared: &PreviewShared) {
    let had_drag = DRAG.with(|d| d.borrow_mut().take().is_some());
    if had_drag {
        shared.dirty.store(true, Ordering::Relaxed);
        shared.changed();
    }
}

fn on_key(shared: &PreviewShared, vk: u16) {
    if vk == VK_DELETE.0 {
        let _ = shared.app.emit("preview:delete", ());
        return;
    }
    let step = if key_down(VK_SHIFT.0) { 10.0 } else { 1.0 };
    let (dx, dy) = match vk {
        v if v == VK_LEFT.0 => (-step, 0.0),
        v if v == VK_RIGHT.0 => (step, 0.0),
        v if v == VK_UP.0 => (0.0, -step),
        v if v == VK_DOWN.0 => (0.0, step),
        _ => return,
    };
    let Some(scene) = shared.scene() else { return };
    for g in items(&scene).iter().filter(|g| g.selected && !g.locked) {
        unsafe {
            let mut p = vec2(0.0, 0.0);
            sys::obs_sceneitem_get_pos(g.item, &mut p);
            let (x, y) = vec2_get(&p);
            sys::obs_sceneitem_set_pos(g.item, &vec2(x + dx, y + dy));
        }
    }
    shared.dirty.store(true, Ordering::Relaxed);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const PreviewShared;
    // SAFETY: set at creation, cleared before the owning Preview drops its Arc.
    let Some(shared) = (unsafe { ptr.as_ref() }).filter(|s| s.editable.load(Ordering::Relaxed)) else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    unsafe {
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_LBUTTONDOWN => {
                let _ = SetFocus(Some(hwnd));
                SetCapture(hwnd);
                on_mouse_down(shared, hwnd, lparam);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                on_mouse_move(shared, hwnd, lparam);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                on_mouse_up(shared);
                return LRESULT(0);
            }
            WM_KEYDOWN => {
                on_key(shared, wparam.0 as u16);
                return LRESULT(0);
            }
            WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
                let id = CURSOR.with(|c| *c.borrow());
                if let Ok(c) = LoadCursorW(None, id) {
                    SetCursor(Some(HCURSOR(c.0)));
                }
                return LRESULT(1);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
