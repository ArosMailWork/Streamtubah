//! Scene collections (`collections/<name>.json`) and profiles (`profiles/<name>.json`).
//! A collection is a full scene/source set; a profile is the video/output/stream settings.

use std::ffi::c_void;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Result, Studio};
use crate::obs::{Source, sys};
use crate::settings::{Profile, valid_name, write_atomic};

fn json_names(dir: &PathBuf) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "json" {
                return None;
            }
            Some(p.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    out.sort_by_key(|n| n.to_lowercase());
    out
}

fn ensure_free(dir: &PathBuf, name: &str) -> Result<()> {
    if json_names(dir).iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return Err(format!("'{name}' already exists"));
    }
    Ok(())
}

impl Studio {
    fn collections_dir(&self) -> PathBuf {
        self.paths.data_dir.join("collections")
    }

    fn profiles_dir(&self) -> PathBuf {
        self.paths.data_dir.join("profiles")
    }

    fn ensure_idle(&self) -> Result<()> {
        if self.outputs.any_active() {
            return Err("Stop streaming, recording, replay buffer and virtual camera first".into());
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Scene collections

    pub fn collections(&self) -> Value {
        let mut list = json_names(&self.collections_dir());
        if !list.contains(&self.settings.collection) {
            list.push(self.settings.collection.clone());
        }
        json!({ "current": self.settings.collection, "list": list })
    }

    /// Releases every scene and source of the current collection so a new one can load
    /// with the same source names.
    fn clear_scene_data(&mut self) {
        if let Some(p) = self.preview_scene.take() {
            unsafe { sys::obs_source_dec_showing(p.as_ptr()) };
        }
        for p in self.previews.values() {
            p.set_scene(None);
        }
        if let Ok(mut m) = self.mixer.lock() {
            m.clear();
        }
        if let Some(tr) = &self.transition {
            unsafe { sys::obs_transition_clear(tr.as_ptr()) };
        }
        self.current = None;

        unsafe extern "C" fn collect(p: *mut c_void, src: *mut sys::obs_source_t) -> bool {
            let out = unsafe { &mut *(p as *mut Vec<Source>) };
            if let Some(s) = Source::from_raw(unsafe { sys::obs_source_get_ref(src) }) {
                out.push(s);
            }
            true
        }
        let mut inputs: Vec<Source> = Vec::new();
        unsafe { sys::obs_enum_sources(Some(collect), &mut inputs as *mut _ as *mut c_void) };
        for s in self.scenes.iter().chain(inputs.iter()) {
            unsafe { sys::obs_source_remove(s.as_ptr()) };
        }
        self.scenes.clear();
        drop(inputs);
        // libobs destroys sources on a background queue; their names stay taken until it drains.
        unsafe { while sys::obs_wait_for_destroy_queue() {} }

        self.bypassed.clear();
        self.desktop.set_device(None);
        self.mic.set_device(None);
        self.desktop.load(Value::Null);
        self.mic.load(Value::Null);
        for ch in &mut self.extra {
            ch.set_device(None);
            ch.load(Value::Null);
        }
    }

    fn open_collection(&mut self, name: &str) -> Result<()> {
        self.ensure_idle()?;
        self.save_collection()?;
        self.clear_scene_data();
        self.paths.collection = self.collections_dir().join(format!("{name}.json"));
        self.settings.collection = name.to_string();
        if !self.load_collection() {
            self.create_default_collection();
            self.save_collection()?;
        }
        self.sync_displays();
        self.save_settings()?;
        self.emit("studio:scenes", json!(self.scene_names()));
        self.emit("studio:changed", Value::Null);
        Ok(())
    }

    pub fn collection_switch(&mut self, name: &str) -> Result<()> {
        if name == self.settings.collection {
            return Ok(());
        }
        if !json_names(&self.collections_dir()).iter().any(|n| n == name) {
            return Err(format!("Scene collection '{name}' not found"));
        }
        self.open_collection(name)
    }

    /// New empty collection (one blank scene), made active.
    pub fn collection_create(&mut self, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        ensure_free(&self.collections_dir(), &name)?;
        self.open_collection(&name)
    }

    /// Copy of the active collection under a new name, made active.
    pub fn collection_duplicate(&mut self, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        let dir = self.collections_dir();
        ensure_free(&dir, &name)?;
        self.save_collection()?;
        std::fs::copy(&self.paths.collection, dir.join(format!("{name}.json"))).map_err(|e| e.to_string())?;
        self.open_collection(&name)
    }

    pub fn collection_rename(&mut self, name: &str, new_name: &str) -> Result<()> {
        let new_name = valid_name(new_name)?;
        let dir = self.collections_dir();
        if !new_name.eq_ignore_ascii_case(name) {
            ensure_free(&dir, &new_name)?;
        }
        if name == self.settings.collection {
            self.save_collection()?;
        }
        let to = dir.join(format!("{new_name}.json"));
        std::fs::rename(dir.join(format!("{name}.json")), &to).map_err(|e| e.to_string())?;
        if name == self.settings.collection {
            self.paths.collection = to;
            self.settings.collection = new_name;
            self.save_settings()?;
        }
        Ok(())
    }

    /// Deletes a collection; removing the active one switches to another first.
    pub fn collection_remove(&mut self, name: &str) -> Result<()> {
        let dir = self.collections_dir();
        let others: Vec<String> = json_names(&dir).into_iter().filter(|n| n != name).collect();
        let Some(fallback) = others.first() else {
            return Err("At least one scene collection must remain".into());
        };
        if name == self.settings.collection {
            self.open_collection(&fallback.clone())?;
        }
        std::fs::remove_file(dir.join(format!("{name}.json"))).map_err(|e| e.to_string())
    }

    // -----------------------------------------------------------------------
    // Profiles

    pub fn profiles(&self) -> Value {
        let mut list = json_names(&self.profiles_dir());
        if !list.contains(&self.settings.profile) {
            list.push(self.settings.profile.clone());
        }
        json!({ "current": self.settings.profile, "list": list })
    }

    fn save_profile(&self) -> Result<()> {
        let dir = self.profiles_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let doc = serde_json::to_string_pretty(&self.settings.profile()).map_err(|e| e.to_string())?;
        write_atomic(&dir.join(format!("{}.json", self.settings.profile)), &doc)
    }

    fn activate_profile(&mut self, name: &str, profile: Profile) -> Result<()> {
        self.ensure_idle()?;
        self.save_profile()?;
        let mut next = self.settings.clone();
        next.video = profile.video;
        next.output = profile.output;
        next.stream = profile.stream;
        // Switch the name first: settings (and their vault secrets) now belong to the new profile.
        let previous = std::mem::replace(&mut self.settings.profile, name.to_string());
        if let Err(e) = self.apply_settings(next) {
            self.settings.profile = previous;
            return Err(e);
        }
        self.outputs.reset_service();
        self.save_profile()?;
        self.save_settings()?;
        self.emit("studio:changed", Value::Null);
        Ok(())
    }

    pub fn profile_switch(&mut self, name: &str) -> Result<()> {
        if name == self.settings.profile {
            return Ok(());
        }
        let text = std::fs::read_to_string(self.profiles_dir().join(format!("{name}.json")))
            .map_err(|_| format!("Profile '{name}' not found"))?;
        let profile: Profile = serde_json::from_str(&text).map_err(|e| format!("Profile '{name}': {e}"))?;
        self.activate_profile(name, profile)
    }

    /// New profile starting from a copy of the current settings, made active.
    pub fn profile_create(&mut self, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        ensure_free(&self.profiles_dir(), &name)?;
        let mut copy = self.settings.profile();
        crate::vault::copy_prefix(&crate::vault::main_prefix(&self.settings.profile), &crate::vault::main_prefix(&name))?;
        // Destinations get new ids so the two profiles keep separate keys.
        for t in &mut copy.stream.targets {
            let id = crate::settings::random_token(6);
            crate::vault::copy_prefix(&crate::vault::target_prefix(&t.id), &crate::vault::target_prefix(&id))?;
            t.id = id;
        }
        self.activate_profile(&name, copy)
    }

    pub fn profile_rename(&mut self, name: &str, new_name: &str) -> Result<()> {
        let new_name = valid_name(new_name)?;
        let dir = self.profiles_dir();
        if !new_name.eq_ignore_ascii_case(name) {
            ensure_free(&dir, &new_name)?;
        }
        let from = dir.join(format!("{name}.json"));
        if name == self.settings.profile {
            self.save_profile()?;
        }
        std::fs::rename(&from, dir.join(format!("{new_name}.json"))).map_err(|e| e.to_string())?;
        if name != new_name {
            crate::vault::copy_prefix(&crate::vault::main_prefix(name), &crate::vault::main_prefix(&new_name))?;
            crate::vault::remove_prefix(&crate::vault::main_prefix(name))?;
        }
        if name == self.settings.profile {
            self.settings.profile = new_name;
            self.save_settings()?;
        }
        Ok(())
    }

    pub fn profile_remove(&mut self, name: &str) -> Result<()> {
        if name == self.settings.profile {
            return Err("Switch to another profile before deleting this one".into());
        }
        let path = self.profiles_dir().join(format!("{name}.json"));
        if let Some(p) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Profile>(&t).ok()) {
            for t in &p.stream.targets {
                crate::vault::remove_prefix(&crate::vault::target_prefix(&t.id))?;
            }
        }
        crate::vault::remove_prefix(&crate::vault::main_prefix(name))?;
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}
