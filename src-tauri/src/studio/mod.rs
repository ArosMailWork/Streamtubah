//! Application model on top of libobs: scenes, sources, items, filters,
//! transitions, audio channels, outputs, persistence.

pub mod channels;
mod audio_adv;
pub use audio_adv::AudioPatch;
mod collections;
mod items;
pub mod mixer;
pub mod outputs;

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::obs::properties::Properties;
use crate::obs::{Data, Source, cstr, from_cstr, runtime, sys, vec2};
use crate::preview::Preview;
use crate::audio_devices;
use crate::settings::{AppSettings, write_atomic};
use channels::AudioChannel;
use mixer::Mixer;
use outputs::Outputs;

pub const DESKTOP_AUDIO: &str = "Desktop Audio";
pub const MIC_AUDIO: &str = "Mic/Aux";
const CH_TRANSITION: u32 = 0;
const CH_DESKTOP: u32 = 1;
const CH_MIC: u32 = 3;
/// Optional extra global devices: (output channel, name, source type).
const EXTRA_CHANNELS: [(u32, &str, &str); 4] = [
    (2, "Desktop Audio 2", "wasapi_output_capture"),
    (4, "Mic/Aux 2", "wasapi_input_capture"),
    (5, "Mic/Aux 3", "wasapi_input_capture"),
    (6, "Mic/Aux 4", "wasapi_input_capture"),
];
/// Keys of `channels` in the collection file, matching EXTRA_CHANNELS.
const EXTRA_KEYS: [&str; 4] = ["desktop2", "mic2", "mic3", "mic4"];

/// Names of global audio devices; user sources cannot take them.
fn reserved_name(n: &str) -> bool {
    n == DESKTOP_AUDIO || n == MIC_AUDIO || EXTRA_CHANNELS.iter().any(|c| c.1 == n)
}

pub type Result<T> = std::result::Result<T, String>;

/// obs-vst opens its plug-in editor as a Qt widget, which aborts without a
/// QApplication. Plug-ins still run with their saved/default parameters.
const HIDDEN_PROPS: &[&str] = &["open_vst_settings", "close_vst_settings", "open_when_active_vst_settings"];

#[derive(Serialize)]
pub struct ModuleInfo {
    pub name: String,
    pub file: String,
    pub description: String,
    pub author: String,
}

/// Every libobs module loaded at startup (built-in and user plugins).
pub fn modules() -> Vec<ModuleInfo> {
    unsafe extern "C" fn cb(p: *mut c_void, m: *mut sys::obs_module_t) {
        let out = unsafe { &mut *(p as *mut Vec<ModuleInfo>) };
        unsafe {
            out.push(ModuleInfo {
                name: from_cstr(sys::obs_get_module_name(m)),
                file: from_cstr(sys::obs_get_module_file_name(m)),
                description: from_cstr(sys::obs_get_module_description(m)),
                author: from_cstr(sys::obs_get_module_author(m)),
            });
        }
    }
    let mut out: Vec<ModuleInfo> = Vec::new();
    unsafe { sys::obs_enum_modules(Some(cb), &mut out as *mut _ as *mut c_void) };
    out.sort_by(|a, b| a.file.to_lowercase().cmp(&b.file.to_lowercase()));
    out
}

/// What a properties form is editing.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropTarget {
    Source { name: String },
    Filter { source: String, filter: String },
    Transition,
    Service,
    /// Service of an extra multistream destination.
    TargetService { id: String },
    VideoEncoder,
}

#[derive(Serialize)]
pub struct TypeInfo {
    pub id: String,
    pub name: String,
    pub video: bool,
    pub audio: bool,
}

#[derive(Serialize)]
pub struct ItemInfo {
    pub id: i64,
    pub name: String,
    pub type_id: String,
    pub type_name: String,
    pub visible: bool,
    pub locked: bool,
    pub selected: bool,
    pub video: bool,
    pub audio: bool,
    /// Folder (libobs group).
    pub group: bool,
    /// Id of the folder containing this item.
    pub parent: Option<i64>,
    /// "both" | "stream" | "record" (per source, see split.rs).
    pub show_in: &'static str,
}

pub struct StudioPaths {
    pub settings: PathBuf,
    /// `collections/` and `profiles/` live here.
    pub data_dir: PathBuf,
    /// File of the active scene collection.
    pub collection: PathBuf,
}

pub struct Studio {
    app: AppHandle,
    paths: StudioPaths,
    pub settings: AppSettings,
    scenes: Vec<Source>,
    current: Option<Source>,
    transition: Option<Source>,
    desktop: AudioChannel,
    mic: AudioChannel,
    /// Desktop Audio 2, Mic/Aux 2-4 (see EXTRA_CHANNELS).
    extra: Vec<AudioChannel>,
    /// Set when "default" mic resolves to a Bluetooth headset and was therefore not opened.
    mic_blocked_bluetooth: bool,
    /// Filters disabled by voice "bypass", per source, to restore afterwards.
    bypassed: HashMap<String, Vec<String>>,
    pub mixer: Arc<Mutex<Mixer>>,
    pub outputs: Outputs,
    /// Native displays by id: "main" (editable) and "program" (Studio Mode output).
    pub previews: HashMap<String, Preview>,
    /// Studio Mode: edit `preview_scene` while `current` stays on air.
    studio_mode: bool,
    preview_scene: Option<Source>,
    pub dirty: Arc<AtomicBool>,
}

impl Studio {
    /// Must be called after libobs startup, video/audio reset and module load.
    pub fn new(app: AppHandle, paths: StudioPaths, settings: AppSettings) -> Self {
        let mut s = Self {
            app: app.clone(),
            paths,
            settings,
            scenes: Vec::new(),
            current: None,
            transition: None,
            desktop: AudioChannel::new(CH_DESKTOP, DESKTOP_AUDIO, "wasapi_output_capture"),
            mic: AudioChannel::new(CH_MIC, MIC_AUDIO, "wasapi_input_capture"),
            extra: EXTRA_CHANNELS.iter().map(|&(ch, name, ty)| AudioChannel::new(ch, name, ty)).collect(),
            mic_blocked_bluetooth: false,
            bypassed: HashMap::new(),
            mixer: Arc::new(Mutex::new(Mixer::default())),
            outputs: Outputs::new(app),
            previews: HashMap::new(),
            studio_mode: false,
            preview_scene: None,
            dirty: Arc::new(AtomicBool::new(false)),
        };
        let type_id = s.settings.transition.type_id.clone();
        s.set_transition_type(&type_id);
        s.apply_monitoring_device();
        if !s.load_collection() {
            s.create_default_collection();
        }
        s
    }

    fn emit(&self, event: &str, payload: Value) {
        let _ = self.app.emit(event, payload);
    }

    fn touch(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    // -----------------------------------------------------------------------
    // Persistence

    fn create_default_collection(&mut self) {
        let _ = self.scene_create("Scene");
        self.sync_channels(false);
        self.touch();
    }

    /// Opens/closes the global audio channels according to settings.
    ///
    /// The mic is never opened implicitly on a Bluetooth headset ("default" resolving to
    /// a hands-free endpoint), because that degrades the headset to mono 16 kHz for the
    /// streamer and for captured desktop audio. Choosing that device explicitly is allowed.
    /// `starting` is true right before an output starts (for the "mic only while live" gate).
    pub fn sync_channels(&mut self, starting: bool) {
        let a = self.settings.audio.clone();
        self.desktop.set_device((!a.desktop_device.is_empty()).then_some(a.desktop_device.as_str()));

        let default_mic = (a.mic_device == "default").then(|| audio_devices::resolve_input("default"));
        self.mic_blocked_bluetooth = matches!(&default_mic, Some(Some(d)) if d.bluetooth);
        let no_device = matches!(default_mic, Some(None));
        let gate_open = !a.mic_live_only || starting || self.outputs.any_audio_active();
        let open = !a.mic_device.is_empty() && !self.mic_blocked_bluetooth && !no_device && gate_open;
        let was_open = self.mic.source.is_some();
        self.mic.set_device(open.then_some(a.mic_device.as_str()));
        let mut changed = was_open != open;

        // Extra devices are only ever explicit choices; extra mics follow the live-only gate too.
        let extra = [&a.desktop2_device, &a.mic2_device, &a.mic3_device, &a.mic4_device];
        for (i, (ch, dev)) in self.extra.iter_mut().zip(extra).enumerate() {
            let is_mic = i > 0;
            let open = !dev.is_empty() && (!is_mic || gate_open);
            changed |= ch.source.is_some() != open;
            ch.set_device(open.then_some(dev.as_str()));
        }
        if changed {
            self.emit("studio:changed", Value::Null);
        }
    }

    pub fn apply_monitoring_device(&self) {
        let id = &self.settings.audio.monitoring_device;
        let name = if id == "default" {
            "Default".to_string()
        } else {
            audio_devices::outputs().into_iter().find(|d| d.id == *id).map(|d| d.name).unwrap_or_default()
        };
        let (n, i) = (cstr(&name), cstr(id));
        unsafe { sys::obs_set_audio_monitoring_device(n.as_ptr(), i.as_ptr()) };
    }

    pub fn save_collection(&self) -> Result<()> {
        let holder = Data::new();
        unsafe {
            let arr = sys::obs_save_sources();
            sys::obs_data_set_array(holder.as_ptr(), c"sources".as_ptr(), arr);
            sys::obs_data_array_release(arr);
        }
        let mut channels = json!({ "desktop": self.desktop.save(), "mic": self.mic.save() });
        for (k, ch) in EXTRA_KEYS.iter().zip(&self.extra) {
            channels[*k] = ch.save();
        }
        let doc = json!({
            "version": 1,
            "sources": holder.to_json().get("sources").cloned().unwrap_or(json!([])),
            "scene_order": self.scenes.iter().map(Source::name).collect::<Vec<_>>(),
            "current_scene": self.current.as_ref().map(Source::name),
            "channels": channels,
        });
        write_atomic(&self.paths.collection, &doc.to_string())
    }

    fn load_collection(&mut self) -> bool {
        let Some(doc) = std::fs::read_to_string(&self.paths.collection)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        else {
            return false;
        };

        unsafe extern "C" fn on_loaded(data: *mut c_void, source: *mut sys::obs_source_t) {
            let out = unsafe { &mut *(data as *mut Vec<Source>) };
            if let Some(s) = Source::from_raw(unsafe { sys::obs_source_get_ref(source) }) {
                out.push(s);
            }
        }
        let holder = Data::from_json(&json!({ "sources": doc["sources"] }));
        let mut loaded: Vec<Source> = Vec::new();
        unsafe {
            let arr = sys::obs_data_get_array(holder.as_ptr(), c"sources".as_ptr());
            sys::obs_load_sources(arr, Some(on_loaded), &mut loaded as *mut _ as *mut c_void);
            sys::obs_data_array_release(arr);
        }

        let order: Vec<String> = serde_json::from_value(doc["scene_order"].clone()).unwrap_or_default();
        let mut scenes: Vec<Source> = loaded.iter().filter(|s| !s.as_scene().is_null()).cloned().collect();
        scenes.sort_by_key(|s| order.iter().position(|n| *n == s.name()).unwrap_or(usize::MAX));
        if scenes.is_empty() {
            return false;
        }
        self.scenes = scenes;

        self.desktop.load(doc["channels"]["desktop"].clone());
        self.mic.load(doc["channels"]["mic"].clone());
        for (k, ch) in EXTRA_KEYS.iter().zip(self.extra.iter_mut()) {
            ch.load(doc["channels"][*k].clone());
        }
        self.sync_channels(false);

        let current = doc["current_scene"].as_str().unwrap_or_default();
        let scene = self.scenes.iter().find(|s| s.name() == current).unwrap_or(&self.scenes[0]).clone();
        self.set_current(scene);
        true
    }

    // -----------------------------------------------------------------------
    // State snapshot for the UI

    pub fn state(&self) -> Value {
        self.sync_mixer();
        let (w, h) = runtime::canvas_size();
        let inputs = audio_devices::inputs();
        json!({
            "scenes": self.scenes.iter().map(Source::name).collect::<Vec<_>>(),
            "current": self.current.as_ref().map(Source::name),
            "studio_mode": self.studio_mode,
            "preview_scene": self.preview_scene.as_ref().map(Source::name),
            "items": self.items(),
            "audio": self.audio_sources().iter().map(|s| audio_info(s, self.is_channel(s), &inputs)).collect::<Vec<_>>(),
            "mic": {
                "device": self.settings.audio.mic_device,
                "open": self.mic.source.is_some(),
                "blocked_bluetooth": self.mic_blocked_bluetooth,
                "live_only": self.settings.audio.mic_live_only,
            },
            "outputs": self.outputs.status(),
            "destinations": self.outputs.destination_status(&self.settings),
            "canvas": { "width": w, "height": h },
        })
    }

    fn channel_sources(&self) -> Vec<Source> {
        [&self.desktop.source, &self.mic.source]
            .into_iter()
            .chain(self.extra.iter().map(|c| &c.source))
            .flatten()
            .cloned()
            .collect()
    }

    fn is_channel(&self, s: &Source) -> bool {
        self.channel_sources().iter().any(|c| c.as_ptr() == s.as_ptr())
    }

    /// Global channels first, then audio-capable sources in the current scene.
    fn audio_sources(&self) -> Vec<Source> {
        let mut out = self.channel_sources();
        if let Some(scene) = &self.current {
            for item in scene_items(scene) {
                let src = item_source(item);
                if src.output_flags() & sys::OBS_SOURCE_AUDIO != 0 && !out.iter().any(|s| s.as_ptr() == src.as_ptr()) {
                    out.push(src);
                }
            }
        }
        out
    }

    fn sync_mixer(&self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.sync(&self.audio_sources());
        }
    }

    /// The scene the user edits: the Studio Mode preview scene, otherwise the program scene.
    fn editing_scene(&self) -> Option<&Source> {
        if self.studio_mode { self.preview_scene.as_ref() } else { self.current.as_ref() }
    }

    fn items(&self) -> Vec<ItemInfo> {
        let Some(scene) = self.editing_scene() else { return Vec::new() };
        // Bottom-to-top with each folder followed by its children; reversed below so the
        // list reads top-most first and children directly follow their folder.
        let mut out: Vec<ItemInfo> = Vec::new();
        for top in scene_items(scene) {
            let mut children: Vec<*mut sys::obs_sceneitem_t> =
                if unsafe { sys::obs_sceneitem_is_group(top) } { items::group_items(top) } else { Vec::new() };
            children.reverse();
            let parent = unsafe { sys::obs_sceneitem_get_id(top) };
            let mut block: Vec<ItemInfo> = std::iter::once((top, None))
                .chain(children.into_iter().map(|c| (c, Some(parent))))
                .map(|(item, parent)| Self::item_info(item, parent))
                .collect();
            block.reverse();
            out.extend(block);
        }
        out.reverse();
        out
    }

    fn item_info(item: *mut sys::obs_sceneitem_t, parent: Option<i64>) -> ItemInfo {
        unsafe {
                let src = item_source(item);
                let id = src.id();
                let flags = src.output_flags();
                let c = cstr(&id);
                let group = sys::obs_sceneitem_is_group(item);
                ItemInfo {
                    id: sys::obs_sceneitem_get_id(item),
                    name: src.name(),
                    type_id: id,
                    visible: sys::obs_sceneitem_visible(item),
                    locked: sys::obs_sceneitem_locked(item),
                    selected: sys::obs_sceneitem_selected(item),
                    video: flags & sys::OBS_SOURCE_VIDEO != 0,
                    audio: flags & sys::OBS_SOURCE_AUDIO != 0,
                    type_name: if group { "Folder".into() } else { from_cstr(sys::obs_source_get_display_name(c.as_ptr())) },
                    group,
                    parent,
                    show_in: crate::split::show_in(&src).as_str(),
                }
        }
    }

    // -----------------------------------------------------------------------
    // Scenes

    fn scene_index(&self, name: &str) -> Result<usize> {
        self.scenes.iter().position(|s| s.name() == name).ok_or_else(|| format!("Scene '{name}' not found"))
    }

    pub fn scene_create(&mut self, name: &str) -> Result<String> {
        let name = unique_name(name.trim(), "Scene");
        let c = cstr(&name);
        let scene = unsafe { sys::obs_scene_create(c.as_ptr()) };
        let source = Source::from_raw(unsafe { sys::obs_source_get_ref(sys::obs_scene_get_source(scene)) })
            .ok_or("obs_scene_create failed")?;
        unsafe { sys::obs_scene_release(scene) };
        self.scenes.push(source.clone());
        self.set_current(source);
        self.emit("studio:scenes", json!(self.scene_names()));
        self.touch();
        Ok(name)
    }

    pub fn scene_remove(&mut self, name: &str) -> Result<()> {
        if self.scenes.len() <= 1 {
            return Err("At least one scene is required".into());
        }
        let idx = self.scene_index(name)?;
        let removed = self.scenes.remove(idx);
        let next = self.scenes[idx.min(self.scenes.len() - 1)].clone();
        if self.current.as_ref().is_some_and(|c| c.as_ptr() == removed.as_ptr()) {
            self.set_current(next.clone());
        }
        if self.preview_scene.as_ref().is_some_and(|c| c.as_ptr() == removed.as_ptr()) {
            self.set_preview_scene(next);
        }
        unsafe { sys::obs_source_remove(removed.as_ptr()) };
        self.emit("studio:scenes", json!(self.scene_names()));
        self.touch();
        Ok(())
    }

    pub fn scene_rename(&mut self, name: &str, new_name: &str) -> Result<()> {
        let scene = self.scenes[self.scene_index(name)?].clone();
        rename_source(&scene, new_name)?;
        self.touch();
        Ok(())
    }

    pub fn scene_move(&mut self, name: &str, delta: i32) -> Result<()> {
        let idx = self.scene_index(name)?;
        let to = (idx as i32 + delta).clamp(0, self.scenes.len() as i32 - 1) as usize;
        let s = self.scenes.remove(idx);
        self.scenes.insert(to, s);
        self.touch();
        Ok(())
    }

    /// Selecting a scene: in Studio Mode it goes to the preview, otherwise straight on air.
    pub fn scene_set_current(&mut self, name: &str) -> Result<()> {
        let scene = self.scenes[self.scene_index(name)?].clone();
        if self.studio_mode {
            self.set_preview_scene(scene);
        } else {
            self.set_current(scene);
        }
        self.touch();
        Ok(())
    }

    /// Puts a scene on air regardless of Studio Mode (used by the control API).
    pub fn scene_set_program(&mut self, name: &str) -> Result<()> {
        let scene = self.scenes[self.scene_index(name)?].clone();
        self.set_current(scene);
        self.touch();
        Ok(())
    }

    fn set_preview_scene(&mut self, scene: Source) {
        unsafe {
            sys::obs_source_inc_showing(scene.as_ptr());
            if let Some(old) = self.preview_scene.take() {
                sys::obs_source_dec_showing(old.as_ptr());
            }
        }
        self.preview_scene = Some(scene);
        self.sync_displays();
        self.emit("studio:changed", Value::Null);
    }

    pub fn studio_mode_set(&mut self, enabled: bool) {
        if enabled == self.studio_mode {
            return;
        }
        self.studio_mode = enabled;
        if enabled {
            if let Some(cur) = self.current.clone() {
                self.set_preview_scene(cur);
            }
        } else if let Some(old) = self.preview_scene.take() {
            unsafe { sys::obs_source_dec_showing(old.as_ptr()) };
        }
        self.sync_displays();
        self.emit("studio:changed", Value::Null);
    }

    pub fn studio_mode(&self) -> bool {
        self.studio_mode
    }

    pub fn preview_scene_name(&self) -> Option<String> {
        self.preview_scene.as_ref().map(Source::name)
    }

    /// Studio Mode "Transition": put the preview scene on air.
    pub fn studio_transition(&mut self) -> Result<()> {
        let scene = self.preview_scene.clone().ok_or("Studio Mode is off")?;
        self.set_current(scene);
        self.touch();
        Ok(())
    }

    /// Points each display at what it should render for the current mode.
    fn sync_displays(&self) {
        if let Some(main) = self.previews.get("main") {
            main.set_mode(!self.studio_mode, true);
            main.set_scene(self.editing_scene().cloned());
        }
        if let Some(program) = self.previews.get("program") {
            program.set_mode(true, false);
            program.set_scene(None);
        }
    }

    fn set_current(&mut self, scene: Source) {
        if let Some(tr) = &self.transition {
            unsafe {
                if self.current.is_some() {
                    sys::obs_transition_start(
                        tr.as_ptr(),
                        sys::obs_transition_mode_OBS_TRANSITION_MODE_AUTO,
                        self.settings.transition.duration_ms,
                        scene.as_ptr(),
                    );
                } else {
                    sys::obs_transition_set(tr.as_ptr(), scene.as_ptr());
                }
            }
        }
        self.emit("studio:scene", json!({ "name": scene.name() }));
        self.current = Some(scene);
        self.sync_displays();
    }

    pub fn scene_names(&self) -> Vec<String> {
        self.scenes.iter().map(Source::name).collect()
    }

    pub fn current_scene_name(&self) -> Option<String> {
        self.current.as_ref().map(Source::name)
    }

    /// Items of any scene by name: (id, source name, type id, enabled, locked), bottom to top.
    pub fn scene_item_list(&self, scene: &str) -> Result<Vec<(i64, String, String, bool, bool)>> {
        let scene = &self.scenes[self.scene_index(scene)?];
        Ok(scene_items(scene)
            .into_iter()
            .map(|item| unsafe {
                let src = item_source(item);
                (sys::obs_sceneitem_get_id(item), src.name(), src.id(), sys::obs_sceneitem_visible(item), sys::obs_sceneitem_locked(item))
            })
            .collect())
    }

    pub fn scene_item_set_enabled(&mut self, scene: &str, id: i64, enabled: bool) -> Result<()> {
        let scene = &self.scenes[self.scene_index(scene)?];
        let item = unsafe { sys::obs_scene_find_sceneitem_by_id(scene.as_scene(), id) };
        if item.is_null() {
            return Err(format!("Item {id} not found"));
        }
        unsafe { sys::obs_sceneitem_set_visible(item, enabled) };
        self.touch();
        Ok(())
    }

    /// All input sources including the global audio channels: (name, type id).
    pub fn input_list(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self.channel_sources().iter().map(|s| (s.name(), s.id())).collect();
        for name in self.existing_sources() {
            if let Some(s) = Source::by_name(&name) {
                out.push((name, s.id()));
            }
        }
        out
    }

    pub fn source(&self, name: &str) -> Result<Source> {
        self.find_source(name)
    }

    pub fn attach_preview(&mut self, id: &str, preview: Preview) {
        self.previews.insert(id.to_string(), preview);
        self.sync_displays();
    }

    // -----------------------------------------------------------------------
    // Transitions

    pub fn transition_types() -> Vec<TypeInfo> {
        enum_types(sys::obs_enum_transition_types)
    }

    pub fn set_transition_type(&mut self, type_id: &str) {
        let tr = Source::create_private(type_id, "Transition", None)
            .or_else(|| Source::create_private("fade_transition", "Transition", None));
        let Some(tr) = tr else { return };
        unsafe {
            if let Some(cur) = &self.current {
                sys::obs_transition_set(tr.as_ptr(), cur.as_ptr());
            }
            sys::obs_set_output_source(CH_TRANSITION, tr.as_ptr());
        }
        self.settings.transition.type_id = tr.id();
        self.transition = Some(tr);
    }

    // -----------------------------------------------------------------------
    // Sources & scene items

    pub fn input_types() -> Vec<TypeInfo> {
        let mut out = Vec::new();
        let (mut id, mut unversioned) = (std::ptr::null(), std::ptr::null());
        let mut idx = 0;
        while unsafe { sys::obs_enum_input_types2(idx, &mut id, &mut unversioned) } {
            idx += 1;
            let flags = unsafe { sys::obs_get_source_output_flags(id) };
            if flags & (sys::OBS_SOURCE_CAP_DISABLED | sys::OBS_SOURCE_CAP_OBSOLETE | sys::OBS_SOURCE_DEPRECATED) != 0 {
                continue;
            }
            out.push(TypeInfo {
                id: from_cstr(id),
                name: from_cstr(unsafe { sys::obs_source_get_display_name(id) }),
                video: flags & sys::OBS_SOURCE_VIDEO != 0,
                audio: flags & sys::OBS_SOURCE_AUDIO != 0,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    /// Public input sources that can be added to another scene.
    pub fn existing_sources(&self) -> Vec<String> {
        unsafe extern "C" fn cb(p: *mut c_void, src: *mut sys::obs_source_t) -> bool {
            let out = unsafe { &mut *(p as *mut Vec<String>) };
            out.push(from_cstr(unsafe { sys::obs_source_get_name(src) }));
            true
        }
        let mut out: Vec<String> = Vec::new();
        unsafe { sys::obs_enum_sources(Some(cb), &mut out as *mut _ as *mut c_void) };
        out.sort();
        out
    }

    /// Scene targeted by source/item editing commands.
    fn current_scene(&self) -> Result<&Source> {
        self.editing_scene().ok_or_else(|| "No scene selected".into())
    }

    /// Item by id in the editing scene, including items inside folders.
    fn item(&self, id: i64) -> Result<*mut sys::obs_sceneitem_t> {
        let scene = self.current_scene()?;
        items::all_items(scene)
            .into_iter()
            .map(|(i, _)| i)
            .find(|&i| unsafe { sys::obs_sceneitem_get_id(i) } == id)
            .ok_or_else(|| format!("Item {id} not found"))
    }

    pub fn source_create(&mut self, type_id: &str, name: &str) -> Result<String> {
        let name = unique_name(name.trim(), "Source");
        let source = Source::create(type_id, &name, None).ok_or_else(|| format!("Cannot create '{type_id}'"))?;
        preselect_lists(&source);
        self.add_to_scene(&source)?;
        Ok(name)
    }

    /// Creates an input in any scene (used by the control API). Returns the scene item id.
    pub fn input_create(&mut self, scene: &str, type_id: &str, name: &str, settings: &Value, enabled: bool) -> Result<i64> {
        if Source::by_name(name).is_some() || self.channel_sources().iter().any(|s| s.name() == name) {
            return Err(format!("'{name}' is already used"));
        }
        let scene = self.scenes[self.scene_index(scene)?].clone();
        let data = Data::from_json(settings);
        let source = Source::create(type_id, name, Some(&data)).ok_or_else(|| format!("Cannot create '{type_id}'"))?;
        if !settings.as_object().is_some_and(|o| !o.is_empty()) {
            preselect_lists(&source);
        }
        let item = unsafe { sys::obs_scene_add(scene.as_scene(), source.as_ptr()) };
        if item.is_null() {
            return Err("Failed to add input to scene".into());
        }
        unsafe { sys::obs_sceneitem_set_visible(item, enabled) };
        self.emit("studio:changed", Value::Null);
        self.touch();
        Ok(unsafe { sys::obs_sceneitem_get_id(item) })
    }

    /// Removes an input from every scene.
    pub fn input_remove(&mut self, name: &str) -> Result<()> {
        let src = Source::by_name(name).ok_or_else(|| format!("Source '{name}' not found"))?;
        unsafe { sys::obs_source_remove(src.as_ptr()) };
        self.emit("studio:changed", Value::Null);
        self.touch();
        Ok(())
    }

    pub fn source_add_existing(&mut self, name: &str) -> Result<()> {
        let source = Source::by_name(name).ok_or_else(|| format!("Source '{name}' not found"))?;
        self.add_to_scene(&source)
    }

    fn add_to_scene(&mut self, source: &Source) -> Result<()> {
        let scene = self.current_scene()?;
        if scene.as_ptr() == source.as_ptr() {
            return Err("A scene cannot contain itself".into());
        }
        let item = unsafe { sys::obs_scene_add(scene.as_scene(), source.as_ptr()) };
        if item.is_null() {
            return Err("Failed to add source to scene".into());
        }
        unsafe {
            for other in scene_items(scene) {
                sys::obs_sceneitem_select(other, false);
            }
            sys::obs_sceneitem_select(item, true);
        }
        self.touch();
        Ok(())
    }

    pub fn source_rename(&mut self, name: &str, new_name: &str) -> Result<()> {
        let source = self.find_source(name)?;
        if self.is_channel(&source) {
            return Err("Global audio devices cannot be renamed".into());
        }
        rename_source(&source, new_name)?;
        self.touch();
        Ok(())
    }

    pub fn item_remove(&mut self, id: i64) -> Result<()> {
        let item = self.item(id)?;
        unsafe { sys::obs_sceneitem_remove(item) };
        self.touch();
        Ok(())
    }

    pub fn item_remove_selected(&mut self) -> Result<()> {
        let scene = self.current_scene()?.clone();
        for (item, _) in items::all_items(&scene) {
            unsafe {
                if sys::obs_sceneitem_selected(item) && !sys::obs_sceneitem_locked(item) {
                    sys::obs_sceneitem_remove(item);
                }
            }
        }
        self.touch();
        Ok(())
    }

    pub fn item_set_visible(&mut self, id: i64, visible: bool) -> Result<()> {
        unsafe { sys::obs_sceneitem_set_visible(self.item(id)?, visible) };
        self.touch();
        Ok(())
    }

    pub fn item_set_locked(&mut self, id: i64, locked: bool) -> Result<()> {
        unsafe { sys::obs_sceneitem_set_locked(self.item(id)?, locked) };
        self.touch();
        Ok(())
    }

    pub fn item_select(&mut self, id: Option<i64>) -> Result<()> {
        let scene = self.current_scene()?;
        for (item, _) in items::all_items(scene) {
            unsafe { sys::obs_sceneitem_select(item, Some(sys::obs_sceneitem_get_id(item)) == id) };
        }
        Ok(())
    }

    pub fn item_order(&mut self, id: i64, movement: &str) -> Result<()> {
        let m = match movement {
            "up" => sys::obs_order_movement_OBS_ORDER_MOVE_UP,
            "down" => sys::obs_order_movement_OBS_ORDER_MOVE_DOWN,
            "top" => sys::obs_order_movement_OBS_ORDER_MOVE_TOP,
            _ => sys::obs_order_movement_OBS_ORDER_MOVE_BOTTOM,
        };
        unsafe { sys::obs_sceneitem_set_order(self.item(id)?, m) };
        self.touch();
        Ok(())
    }

    pub fn item_transform(&mut self, id: i64, action: &str) -> Result<()> {
        if action.starts_with("rotate_") || action.starts_with("flip_") {
            return self.item_rotate_flip(id, action);
        }
        let item = self.item(id)?;
        let (bw, bh) = runtime::canvas_size();
        unsafe {
            let mut info: sys::obs_transform_info = std::mem::zeroed();
            sys::obs_sceneitem_get_info2(item, &mut info);
            match action {
                "fit" | "stretch" => {
                    info.bounds_type = if action == "fit" {
                        sys::obs_bounds_type_OBS_BOUNDS_SCALE_INNER
                    } else {
                        sys::obs_bounds_type_OBS_BOUNDS_STRETCH
                    };
                    info.bounds = vec2(bw as f32, bh as f32);
                    info.bounds_alignment = sys::OBS_ALIGN_CENTER;
                    info.alignment = sys::OBS_ALIGN_LEFT | sys::OBS_ALIGN_TOP;
                    info.pos = vec2(0.0, 0.0);
                    info.rot = 0.0;
                }
                "reset" => {
                    info.bounds_type = sys::obs_bounds_type_OBS_BOUNDS_NONE;
                    info.scale = vec2(1.0, 1.0);
                    info.alignment = sys::OBS_ALIGN_LEFT | sys::OBS_ALIGN_TOP;
                    info.pos = vec2(0.0, 0.0);
                    info.rot = 0.0;
                    let crop = sys::obs_sceneitem_crop { left: 0, top: 0, right: 0, bottom: 0 };
                    sys::obs_sceneitem_set_crop(item, &crop);
                }
                "center" => {
                    let mut m = crate::obs::Matrix4::default();
                    sys::obs_sceneitem_get_box_transform(item, &mut m as *mut _ as *mut sys::matrix4);
                    let (cx, cy) = m.apply(0.5, 0.5);
                    let (px, py) = crate::obs::vec2_get(&info.pos);
                    info.pos = vec2(px + bw as f32 / 2.0 - cx, py + bh as f32 / 2.0 - cy);
                }
                _ => return Err(format!("Unknown transform action '{action}'")),
            }
            sys::obs_sceneitem_set_info2(item, &info);
        }
        self.touch();
        Ok(())
    }

    fn find_source(&self, name: &str) -> Result<Source> {
        self.channel_sources()
            .into_iter()
            .find(|s| s.name() == name)
            .or_else(|| Source::by_name(name))
            .ok_or_else(|| format!("Source '{name}' not found"))
    }

    // -----------------------------------------------------------------------
    // Filters

    pub fn filter_types(&self, source: &str) -> Result<Vec<TypeInfo>> {
        let sf = self.find_source(source)?.output_flags();
        let all = enum_types(sys::obs_enum_filter_types);
        Ok(all
            .into_iter()
            .filter(|t| {
                let c = cstr(&t.id);
                let ff = unsafe { sys::obs_get_source_output_flags(c.as_ptr()) };
                let hidden = ff & (sys::OBS_SOURCE_DEPRECATED | sys::OBS_SOURCE_CAP_DISABLED | sys::OBS_SOURCE_CAP_OBSOLETE) != 0;
                let audio_ok = ff & sys::OBS_SOURCE_AUDIO != 0 && sf & sys::OBS_SOURCE_AUDIO != 0;
                let video_ok = ff & sys::OBS_SOURCE_VIDEO != 0
                    && sf & sys::OBS_SOURCE_VIDEO != 0
                    && (ff & sys::OBS_SOURCE_ASYNC == 0 || sf & sys::OBS_SOURCE_ASYNC != 0);
                !hidden && (audio_ok || video_ok)
            })
            .collect())
    }

    pub fn filters(&self, source: &str) -> Result<Vec<Value>> {
        unsafe extern "C" fn cb(_p: *mut sys::obs_source_t, f: *mut sys::obs_source_t, data: *mut c_void) {
            let out = unsafe { &mut *(data as *mut Vec<Value>) };
            unsafe {
                let id = sys::obs_source_get_id(f);
                // The stream/recording gate is managed by "Show in", not the filter list.
                if from_cstr(id) == crate::split::GATE_ID {
                    return;
                }
                out.push(json!({
                    "name": from_cstr(sys::obs_source_get_name(f)),
                    "type_id": from_cstr(id),
                    "type_name": from_cstr(sys::obs_source_get_display_name(id)),
                    "enabled": sys::obs_source_enabled(f),
                }));
            }
        }
        let src = self.find_source(source)?;
        let mut out: Vec<Value> = Vec::new();
        unsafe { sys::obs_source_enum_filters(src.as_ptr(), Some(cb), &mut out as *mut _ as *mut c_void) };
        Ok(out)
    }

    fn filter(&self, source: &str, filter: &str) -> Result<(Source, Source)> {
        let src = self.find_source(source)?;
        let c = cstr(filter);
        let f = Source::from_raw(unsafe { sys::obs_source_get_filter_by_name(src.as_ptr(), c.as_ptr()) })
            .ok_or_else(|| format!("Filter '{filter}' not found"))?;
        Ok((src, f))
    }

    pub fn filter_add(&mut self, source: &str, type_id: &str, name: &str) -> Result<String> {
        let src = self.find_source(source)?;
        let mut final_name = name.trim().to_string();
        if final_name.is_empty() {
            final_name = "Filter".into();
        }
        let base = final_name.clone();
        let mut n = 2;
        while self.filter(source, &final_name).is_ok() {
            final_name = format!("{base} {n}");
            n += 1;
        }
        let f = Source::create(type_id, &final_name, None).ok_or_else(|| format!("Cannot create filter '{type_id}'"))?;
        unsafe { sys::obs_source_filter_add(src.as_ptr(), f.as_ptr()) };
        self.touch();
        Ok(final_name)
    }

    /// Limits a source to the stream or the recording ("both" = everywhere). Returns a note
    /// when a running recording cannot follow the change (it shares the stream's canvas).
    pub fn source_set_show_in(&mut self, source: &str, mode: &str) -> Result<Option<String>> {
        let mode = crate::split::ShowIn::parse(mode).ok_or("Unknown mode")?;
        crate::split::set_show_in(&self.find_source(source)?, mode)?;
        self.emit("studio:changed", Value::Null);
        self.touch();
        let recording = self.outputs.is_running("record") || self.outputs.is_running("replay");
        Ok((recording && !self.outputs.record_split_active() && mode != crate::split::ShowIn::Both)
            .then(|| "The running recording keeps showing the stream picture; the change applies from the next recording.".into()))
    }

    pub fn filter_remove(&mut self, source: &str, filter: &str) -> Result<()> {
        let (src, f) = self.filter(source, filter)?;
        unsafe { sys::obs_source_filter_remove(src.as_ptr(), f.as_ptr()) };
        self.touch();
        Ok(())
    }

    pub fn filter_set_enabled(&mut self, source: &str, filter: &str, enabled: bool) -> Result<()> {
        let (_, f) = self.filter(source, filter)?;
        unsafe { sys::obs_source_set_enabled(f.as_ptr(), enabled) };
        self.touch();
        Ok(())
    }

    pub fn filter_move(&mut self, source: &str, filter: &str, up: bool) -> Result<()> {
        let (src, f) = self.filter(source, filter)?;
        let m = if up { sys::obs_order_movement_OBS_ORDER_MOVE_UP } else { sys::obs_order_movement_OBS_ORDER_MOVE_DOWN };
        unsafe { sys::obs_source_filter_set_order(src.as_ptr(), f.as_ptr(), m) };
        self.touch();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Audio

    pub fn audio_set_volume(&mut self, name: &str, db: f32) -> Result<()> {
        let src = self.find_source(name)?;
        let mul = if db <= -60.0 { 0.0 } else { 10f32.powf(db / 20.0) };
        unsafe { sys::obs_source_set_volume(src.as_ptr(), mul) };
        self.emit("studio:volume", json!({ "name": name, "mul": mul, "db": db }));
        self.touch();
        Ok(())
    }

    pub fn audio_set_muted(&mut self, name: &str, muted: bool) -> Result<()> {
        let src = self.find_source(name)?;
        unsafe { sys::obs_source_set_muted(src.as_ptr(), muted) };
        self.emit("studio:mute", json!({ "name": name, "muted": muted }));
        self.touch();
        Ok(())
    }

    pub fn audio_toggle_muted(&mut self, name: &str) -> Result<()> {
        let src = self.find_source(name)?;
        let muted = unsafe { sys::obs_source_muted(src.as_ptr()) };
        self.audio_set_muted(name, !muted)
    }

    pub fn audio_set_monitoring(&mut self, name: &str, mode: i32) -> Result<()> {
        let src = self.find_source(name)?;
        unsafe { sys::obs_source_set_monitoring_type(src.as_ptr(), mode.clamp(0, 2) as sys::obs_monitoring_type) };
        self.touch();
        Ok(())
    }

    /// A/B comparison for a voice chain: disables every enabled filter and
    /// restores exactly those on un-bypass.
    pub fn voice_set_bypass(&mut self, source: &str, bypass: bool) -> Result<()> {
        if bypass {
            let enabled: Vec<String> = self
                .filters(source)?
                .into_iter()
                .filter(|f| f["enabled"] == true)
                .filter_map(|f| f["name"].as_str().map(String::from))
                .collect();
            for f in &enabled {
                self.filter_set_enabled(source, f, false)?;
            }
            self.bypassed.insert(source.to_string(), enabled);
        } else if let Some(list) = self.bypassed.remove(source) {
            for f in list {
                let _ = self.filter_set_enabled(source, &f, true);
            }
        }
        Ok(())
    }

    pub fn voice_bypassed(&self, source: &str) -> bool {
        self.bypassed.contains_key(source)
    }

    // -----------------------------------------------------------------------
    // Generic properties

    /// A destination with its secrets filled in from the vault.
    fn target(&self, id: &str) -> Result<crate::settings::StreamTarget> {
        let mut t = self.settings.stream.targets.iter().find(|t| t.id == id).cloned().ok_or("Destination not found")?;
        t.service_settings = crate::vault::with_service_secrets(&crate::vault::target_prefix(id), &t.service_settings);
        Ok(t)
    }

    pub fn properties(&mut self, target: &PropTarget) -> Result<Value> {
        let (props, settings) = match target {
            PropTarget::Source { name } => source_props(&self.find_source(name)?),
            PropTarget::Filter { source, filter } => source_props(&self.filter(source, filter)?.1),
            PropTarget::Transition => source_props(self.transition.as_ref().ok_or("No transition")?),
            PropTarget::Service => {
                let svc = self.outputs.service(&self.settings.with_secrets().stream)?;
                let props = Properties::from_raw(unsafe { sys::obs_service_properties(svc.as_ptr()) });
                (props, svc.settings())
            }
            PropTarget::TargetService { id } => {
                let t = self.target(id)?;
                let svc = self.outputs.target_service(&t)?;
                let props = Properties::from_raw(unsafe { sys::obs_service_properties(svc.as_ptr()) });
                (props, svc.settings())
            }
            PropTarget::VideoEncoder => {
                let id = outputs::resolve_video_encoder(&self.settings.output.video_encoder).ok_or("No encoder")?;
                let c = cstr(&id);
                let props = Properties::from_raw(unsafe { sys::obs_get_encoder_properties(c.as_ptr()) });
                let data = Data::from_raw(unsafe { sys::obs_encoder_defaults(c.as_ptr()) }).unwrap_or_else(Data::new);
                data.apply(&Data::from_json(&self.settings.output.video_encoder_settings));
                (props, data)
            }
        };
        let props = props.ok_or("This object has no properties")?;
        props.apply_settings(&settings);
        let mut list = props.to_json();
        // Stream keys / tokens are edited only in the capture-protected Secrets window.
        let service = matches!(target, PropTarget::Service | PropTarget::TargetService { .. });
        let hidden = |name: &str| HIDDEN_PROPS.contains(&name) || (service && crate::vault::SERVICE_SECRET_FIELDS.contains(&name));
        if let Some(arr) = list.as_array_mut() {
            arr.retain(|p| !hidden(p["name"].as_str().unwrap_or("")));
        }
        let mut values = settings.to_json_with_defaults();
        if service {
            if let Some(obj) = values.as_object_mut() {
                for f in crate::vault::SERVICE_SECRET_FIELDS {
                    obj.remove(f);
                }
            }
        }
        Ok(json!({ "properties": list, "settings": values }))
    }

    pub fn properties_update(&mut self, target: &PropTarget, patch: &Value) -> Result<Value> {
        let data = Data::from_json(patch);
        match target {
            PropTarget::Source { name } => self.find_source(name)?.update(&data),
            PropTarget::Filter { source, filter } => self.filter(source, filter)?.1.update(&data),
            PropTarget::Transition => self.transition.as_ref().ok_or("No transition")?.update(&data),
            PropTarget::Service => {
                let svc = self.outputs.service(&self.settings.with_secrets().stream)?;
                unsafe { sys::obs_service_update(svc.as_ptr(), data.as_ptr()) };
                let mut saved = svc.settings().to_json();
                crate::vault::take_service_secrets(&crate::vault::main_prefix(&self.settings.profile), &mut saved)?;
                self.settings.stream.service_settings = saved;
                self.save_settings()?;
            }
            PropTarget::TargetService { id } => {
                let t = self.target(id)?;
                let svc = self.outputs.target_service(&t)?;
                unsafe { sys::obs_service_update(svc.as_ptr(), data.as_ptr()) };
                let mut saved = svc.settings().to_json();
                crate::vault::take_service_secrets(&crate::vault::target_prefix(id), &mut saved)?;
                if let Some(t) = self.settings.stream.targets.iter_mut().find(|t| t.id == *id) {
                    t.service_settings = saved;
                }
                self.save_settings()?;
            }
            PropTarget::VideoEncoder => {
                let s = &mut self.settings.output.video_encoder_settings;
                if !s.is_object() {
                    *s = json!({});
                }
                if let (Some(dst), Some(src)) = (s.as_object_mut(), patch.as_object()) {
                    dst.extend(src.clone());
                }
                self.outputs.invalidate_encoders();
                self.save_settings()?;
            }
        }
        self.touch();
        self.properties(target)
    }

    pub fn properties_button(&mut self, target: &PropTarget, button: &str) -> Result<Value> {
        let (obj, props) = match target {
            PropTarget::Source { name } => {
                let s = self.find_source(name)?;
                (s.as_ptr() as *mut c_void, Properties::from_raw(unsafe { sys::obs_source_properties(s.as_ptr()) }))
            }
            PropTarget::Filter { source, filter } => {
                let f = self.filter(source, filter)?.1;
                (f.as_ptr() as *mut c_void, Properties::from_raw(unsafe { sys::obs_source_properties(f.as_ptr()) }))
            }
            PropTarget::Service => {
                let svc = self.outputs.service(&self.settings.with_secrets().stream)?;
                (svc.as_ptr() as *mut c_void, Properties::from_raw(unsafe { sys::obs_service_properties(svc.as_ptr()) }))
            }
            PropTarget::TargetService { id } => {
                let t = self.target(id)?;
                let svc = self.outputs.target_service(&t)?;
                (svc.as_ptr() as *mut c_void, Properties::from_raw(unsafe { sys::obs_service_properties(svc.as_ptr()) }))
            }
            _ => return self.properties(target),
        };
        if let Some(props) = props {
            unsafe { sys::obs_property_button_clicked(props.get(button), obj) };
        }
        self.properties(target)
    }

    // -----------------------------------------------------------------------
    // Outputs (shared by UI commands and the control API)

    /// Starts "stream" | "record" | "replay". Opens a live-only mic first.
    pub fn start_output(&mut self, kind: &str) -> Result<()> {
        if kind == "virtualcam" {
            return self.outputs.start_virtualcam();
        }
        self.sync_channels(true);
        let settings = self.settings.with_secrets();
        let r = match kind {
            "stream" => self.outputs.start_stream(&settings),
            "record" => self.outputs.start_record(&settings),
            "replay" => self.outputs.start_replay(&settings),
            _ => Err(format!("Unknown output '{kind}'")),
        };
        if r.is_err() {
            self.sync_channels(false);
        }
        r
    }

    // -----------------------------------------------------------------------
    // Settings

    pub fn save_settings(&self) -> Result<()> {
        self.settings.save(&self.paths.settings)
    }

    pub fn apply_settings(&mut self, mut new: AppSettings) -> Result<()> {
        // Owned by the collection/profile commands; a stale UI draft must not move them.
        new.collection = self.settings.collection.clone();
        new.profile = self.settings.profile.clone();
        // Secrets never live in settings.json (also covers older UIs sending them).
        if new.take_secrets()? {
            self.outputs.reset_service();
        }
        let video_changed = serde_json::to_value(&new.video).ok() != serde_json::to_value(&self.settings.video).ok();
        if video_changed {
            if self.outputs.any_active() {
                return Err("Stop streaming/recording before changing video settings".into());
            }
            runtime::reset_video(&new.video.to_config())?;
        }
        let transition_changed = new.transition.type_id != self.settings.transition.type_id;
        let monitoring_changed = new.audio.monitoring_device != self.settings.audio.monitoring_device;
        self.settings = new;
        if transition_changed {
            let id = self.settings.transition.type_id.clone();
            self.set_transition_type(&id);
        }
        if monitoring_changed {
            self.apply_monitoring_device();
        }
        self.sync_channels(false);
        self.outputs.invalidate_encoders();
        if self.outputs.is_running("stream") {
            let settings = self.settings.with_secrets();
            let errors = self.outputs.sync_destinations(&settings);
            if !errors.is_empty() {
                self.emit("app:error", json!(format!("Could not start: {}", errors.join("; "))));
            }
            self.emit("studio:changed", Value::Null);
        }
        self.touch();
        self.save_settings()
    }

    /// Releases everything that holds libobs references, in dependency order.
    pub fn shutdown(&mut self) {
        let _ = self.save_collection();
        let _ = self.save_settings();
        self.outputs.force_stop_all();
        self.previews.clear();
        unsafe { sys::obs_set_output_source(CH_TRANSITION, std::ptr::null_mut()) };
        // Remove every source (like switching collections) and wait for libobs to destroy them,
        // so Browser Sources close their CEF browsers before obs_shutdown stops CEF.
        self.clear_scene_data();
        self.transition = None;
    }
}

// ---------------------------------------------------------------------------
// Free helpers

/// New sources often default to "no device/monitor selected". Like Streamlabs,
/// pick the first available option for top-level lists whose value is not valid.
fn preselect_lists(src: &Source) {
    let (Some(props), settings) = source_props(src) else { return };
    props.apply_settings(&settings);
    let current = settings.to_json_with_defaults();
    let patch = serde_json::Map::from_iter(props.to_json().as_array().into_iter().flatten().filter_map(|p| {
        let items = p["items"].as_array().filter(|_| p["type"] == "list" && p["editable"] != true)?;
        let name = p["name"].as_str()?;
        let valid = items.iter().any(|i| i["value"] == current[name] && i["disabled"] != true);
        let first = items.iter().find(|i| i["disabled"] != true)?;
        (!valid).then(|| (name.to_string(), first["value"].clone()))
    }));
    if !patch.is_empty() {
        src.update(&Data::from_json(&Value::Object(patch)));
    }
}

fn source_props(src: &Source) -> (Option<Properties>, Data) {
    (Properties::from_raw(unsafe { sys::obs_source_properties(src.as_ptr()) }), src.settings())
}

fn audio_info(s: &Source, channel: bool, inputs: &[audio_devices::AudioDevice]) -> Value {
    let bluetooth = s.id() == "wasapi_input_capture" && {
        let dev = s.settings().to_json()["device_id"].as_str().unwrap_or("default").to_string();
        inputs.iter().any(|d| d.bluetooth && (d.id == dev || (dev == "default" && d.default)))
    };
    unsafe {
        let mul = sys::obs_source_get_volume(s.as_ptr());
        let db = if mul <= 0.0 { -60.0 } else { (20.0 * mul.log10()).max(-60.0) };
        json!({
            "name": s.name(),
            "volume_db": db,
            "muted": sys::obs_source_muted(s.as_ptr()),
            "monitoring": sys::obs_source_get_monitoring_type(s.as_ptr()),
            "type_id": s.id(),
            "channel": channel,
            "bluetooth_mic": bluetooth,
            "advanced": audio_adv::info(s),
        })
    }
}

/// Borrowed item pointers in bottom-to-top order; valid while the scene lives.
fn scene_items(scene: &Source) -> Vec<*mut sys::obs_sceneitem_t> {
    unsafe extern "C" fn cb(_s: *mut sys::obs_scene_t, item: *mut sys::obs_sceneitem_t, p: *mut c_void) -> bool {
        unsafe { (*(p as *mut Vec<*mut sys::obs_sceneitem_t>)).push(item) };
        true
    }
    let mut out = Vec::new();
    unsafe { sys::obs_scene_enum_items(scene.as_scene(), Some(cb), &mut out as *mut _ as *mut c_void) };
    out
}

fn item_source(item: *mut sys::obs_sceneitem_t) -> Source {
    Source::from_raw(unsafe { sys::obs_source_get_ref(sys::obs_sceneitem_get_source(item)) }).expect("item has a source")
}

fn enum_types(f: unsafe extern "C" fn(usize, *mut *const std::ffi::c_char) -> bool) -> Vec<TypeInfo> {
    let mut out = Vec::new();
    let mut id = std::ptr::null();
    let mut idx = 0;
    while unsafe { f(idx, &mut id) } {
        idx += 1;
        let flags = unsafe { sys::obs_get_source_output_flags(id) };
        out.push(TypeInfo {
            id: from_cstr(id),
            name: from_cstr(unsafe { sys::obs_source_get_display_name(id) }),
            video: flags & sys::OBS_SOURCE_VIDEO != 0,
            audio: flags & sys::OBS_SOURCE_AUDIO != 0,
        });
    }
    out
}

/// Source names are global in libobs; append " (n)" until free.
fn unique_name(wanted: &str, fallback: &str) -> String {
    let base = if wanted.is_empty() { fallback } else { wanted };
    let taken = |n: &str| Source::by_name(n).is_some() || reserved_name(n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap()
}

fn rename_source(source: &Source, new_name: &str) -> Result<()> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    if new_name == source.name() {
        return Ok(());
    }
    if Source::by_name(new_name).is_some() || reserved_name(new_name) {
        return Err(format!("'{new_name}' is already used"));
    }
    let c = cstr(new_name);
    unsafe { sys::obs_source_set_name(source.as_ptr(), c.as_ptr()) };
    Ok(())
}
