//! Source list management: groups (folders), drag-and-drop ordering, duplicate / paste,
//! copying filters. Groups cannot be nested (libobs rule); item ids stay unique per scene
//! because grouping moves the same item between the scene and the group's private scene.

use std::ffi::c_void;

use serde_json::{Value, json};

use super::{Result, Studio, item_source, scene_items, unique_name};
use crate::obs::{Source, cstr, from_cstr, sys, vec2, vec2_get};

/// Every item of a scene, top-level and inside groups: (item, parent group item or null).
pub(super) fn all_items(scene: &Source) -> Vec<(*mut sys::obs_sceneitem_t, *mut sys::obs_sceneitem_t)> {
    let mut out = Vec::new();
    for item in scene_items(scene) {
        out.push((item, std::ptr::null_mut()));
        if unsafe { sys::obs_sceneitem_is_group(item) } {
            for child in group_items(item) {
                out.push((child, item));
            }
        }
    }
    out
}

/// Items of a group, bottom to top.
pub(super) fn group_items(group: *mut sys::obs_sceneitem_t) -> Vec<*mut sys::obs_sceneitem_t> {
    unsafe extern "C" fn cb(_s: *mut sys::obs_scene_t, item: *mut sys::obs_sceneitem_t, p: *mut c_void) -> bool {
        unsafe { (*(p as *mut Vec<*mut sys::obs_sceneitem_t>)).push(item) };
        true
    }
    let mut out = Vec::new();
    unsafe { sys::obs_sceneitem_group_enum_items(group, Some(cb), &mut out as *mut _ as *mut c_void) };
    out
}

/// Transform/crop/visibility snapshot used to place a pasted or duplicated item.
struct Placement {
    info: sys::obs_transform_info,
    crop: sys::obs_sceneitem_crop,
    visible: bool,
}

impl Placement {
    fn of(item: *mut sys::obs_sceneitem_t) -> Self {
        unsafe {
            let mut info: sys::obs_transform_info = std::mem::zeroed();
            sys::obs_sceneitem_get_info2(item, &mut info);
            let mut crop: sys::obs_sceneitem_crop = std::mem::zeroed();
            sys::obs_sceneitem_get_crop(item, &mut crop);
            Self { info, crop, visible: sys::obs_sceneitem_visible(item) }
        }
    }

    fn apply(&self, item: *mut sys::obs_sceneitem_t) {
        unsafe {
            sys::obs_sceneitem_defer_update_begin(item);
            sys::obs_sceneitem_set_info2(item, &self.info);
            sys::obs_sceneitem_set_crop(item, &self.crop);
            sys::obs_sceneitem_set_visible(item, self.visible);
            sys::obs_sceneitem_defer_update_end(item);
        }
    }
}

impl Studio {
    /// Moves an item into `parent` (a group id, or None for the scene itself) at
    /// `index` counted from the top of the list, like the UI shows it.
    pub fn item_move(&mut self, id: i64, parent: Option<i64>, index: usize) -> Result<()> {
        let mut item = self.item(id)?;
        let target_group = match parent {
            Some(g) => {
                let g = self.item(g)?;
                if !unsafe { sys::obs_sceneitem_is_group(g) } {
                    return Err("Target is not a folder".into());
                }
                if unsafe { sys::obs_sceneitem_is_group(item) } {
                    return Err("Folders cannot contain folders".into());
                }
                g
            }
            None => std::ptr::null_mut(),
        };
        let scene = self.current_scene()?.clone();
        let current_group = unsafe { sys::obs_sceneitem_get_group(scene.as_scene(), item) };
        if current_group != target_group {
            unsafe {
                if !current_group.is_null() {
                    sys::obs_sceneitem_group_remove_item(current_group, item);
                }
                if !target_group.is_null() {
                    sys::obs_sceneitem_group_add_item(target_group, item);
                }
            }
            item = self.item(id)?;
        }
        let siblings = if target_group.is_null() { scene_items(&scene) } else { group_items(target_group) };
        let others = siblings.len().saturating_sub(1);
        let position = others.saturating_sub(index.min(others));
        unsafe { sys::obs_sceneitem_set_order_position(item, position as i32) };
        self.touch();
        Ok(())
    }

    /// Puts the given top-level items into a new folder.
    pub fn group_create(&mut self, name: &str, ids: &[i64]) -> Result<i64> {
        let scene = self.current_scene()?.clone();
        let top: Vec<*mut sys::obs_sceneitem_t> = scene_items(&scene);
        let mut picked: Vec<*mut sys::obs_sceneitem_t> = Vec::new();
        for &id in ids {
            let item = self.item(id)?;
            if !top.contains(&item) || unsafe { sys::obs_sceneitem_is_group(item) } {
                return Err("Only items outside folders can be grouped".into());
            }
            picked.push(item);
        }
        let name = unique_name(name, "Folder");
        let n = cstr(&name);
        let group = unsafe { sys::obs_scene_insert_group(scene.as_scene(), n.as_ptr(), picked.as_mut_ptr(), picked.len()) };
        if group.is_null() {
            return Err("Could not create folder".into());
        }
        self.touch();
        Ok(unsafe { sys::obs_sceneitem_get_id(group) })
    }

    pub fn group_ungroup(&mut self, id: i64) -> Result<()> {
        let item = self.item(id)?;
        if !unsafe { sys::obs_sceneitem_is_group(item) } {
            return Err("Not a folder".into());
        }
        unsafe { sys::obs_sceneitem_group_ungroup(item) };
        self.touch();
        Ok(())
    }

    /// Adds the source of `from_id` in scene `from_scene` to the editing scene with the same
    /// transform. `duplicate` makes an independent copy instead of a reference.
    pub fn item_paste(&mut self, from_scene: &str, from_id: i64, duplicate: bool) -> Result<i64> {
        let src_scene = self.scenes.iter().find(|s| s.name() == from_scene).cloned()
            .ok_or_else(|| format!("Scene '{from_scene}' no longer exists"))?;
        let from = all_items(&src_scene).into_iter().map(|(i, _)| i)
            .find(|&i| unsafe { sys::obs_sceneitem_get_id(i) } == from_id)
            .ok_or("The copied source no longer exists")?;
        if unsafe { sys::obs_sceneitem_is_group(from) } {
            return Err("Folders cannot be copied".into());
        }
        let placement = Placement::of(from);
        let source = item_source(from);
        let source = if duplicate {
            let name = cstr(&unique_name(&source.name(), "Source"));
            Source::from_raw(unsafe { sys::obs_source_duplicate(source.as_ptr(), name.as_ptr(), false) })
                .ok_or("Could not duplicate the source")?
        } else {
            source
        };
        let scene = self.current_scene()?.clone();
        if scene.as_ptr() == source.as_ptr() {
            return Err("A scene cannot contain itself".into());
        }
        let item = unsafe { sys::obs_scene_add(scene.as_scene(), source.as_ptr()) };
        if item.is_null() {
            return Err("Failed to add source to scene".into());
        }
        placement.apply(item);
        unsafe {
            for (other, _) in all_items(&scene) {
                sys::obs_sceneitem_select(other, other == item);
            }
        }
        self.touch();
        Ok(unsafe { sys::obs_sceneitem_get_id(item) })
    }

    /// Replaces nothing: appends copies of `from`'s filters to `to` (OBS "Paste Filters").
    pub fn filters_copy(&mut self, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let (src, dst) = (self.find_source(from)?, self.find_source(to)?);
        // "Show in" belongs to the source, not to its filter chain: keep the target's own.
        let keep = crate::split::show_in(&dst);
        unsafe { sys::obs_source_copy_filters(dst.as_ptr(), src.as_ptr()) };
        if dst.output_flags() & sys::OBS_SOURCE_VIDEO != 0 {
            crate::split::set_show_in(&dst, keep)?;
        }
        self.touch();
        Ok(())
    }
}

/// Top-left corner of an item's bounding box in canvas pixels (OBS keeps this fixed when
/// rotating or flipping so the item does not jump).
fn top_left(item: *mut sys::obs_sceneitem_t) -> (f32, f32) {
    let mut m = crate::obs::Matrix4::default();
    unsafe { sys::obs_sceneitem_get_box_transform(item, &mut m as *mut _ as *mut sys::matrix4) };
    [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .iter()
        .map(|&(u, v)| m.apply(u, v))
        .fold((f32::MAX, f32::MAX), |(ax, ay), (x, y)| (ax.min(x), ay.min(y)))
}

fn transition_info(item: *mut sys::obs_sceneitem_t, show: bool) -> Value {
    unsafe {
        let tr = sys::obs_sceneitem_get_transition(item, show);
        json!({
            "type_id": if tr.is_null() { String::new() } else { from_cstr(sys::obs_source_get_id(tr)) },
            "duration_ms": sys::obs_sceneitem_get_transition_duration(item, show),
        })
    }
}

impl Studio {
    /// rotate_cw | rotate_ccw | rotate_180 | flip_h | flip_v, keeping the top-left corner in place.
    pub fn item_rotate_flip(&mut self, id: i64, action: &str) -> Result<()> {
        let item = self.item(id)?;
        let before = top_left(item);
        unsafe {
            let rot = sys::obs_sceneitem_get_rot(item);
            let mut scale: sys::vec2 = std::mem::zeroed();
            sys::obs_sceneitem_get_scale(item, &mut scale);
            let (sx, sy) = vec2_get(&scale);
            match action {
                "rotate_cw" => sys::obs_sceneitem_set_rot(item, (rot + 90.0).rem_euclid(360.0)),
                "rotate_ccw" => sys::obs_sceneitem_set_rot(item, (rot - 90.0).rem_euclid(360.0)),
                "rotate_180" => sys::obs_sceneitem_set_rot(item, (rot + 180.0).rem_euclid(360.0)),
                "flip_h" => sys::obs_sceneitem_set_scale(item, &vec2(-sx, sy)),
                "flip_v" => sys::obs_sceneitem_set_scale(item, &vec2(sx, -sy)),
                _ => return Err(format!("Unknown transform action '{action}'")),
            }
            sys::obs_sceneitem_force_update_transform(item);
            let after = top_left(item);
            let mut pos: sys::vec2 = std::mem::zeroed();
            sys::obs_sceneitem_get_pos(item, &mut pos);
            let (px, py) = vec2_get(&pos);
            sys::obs_sceneitem_set_pos(item, &vec2(px + before.0 - after.0, py + before.1 - after.1));
        }
        self.touch();
        Ok(())
    }

    /// Values for the Edit Transform dialog.
    pub fn item_transform_get(&self, id: i64) -> Result<Value> {
        let item = self.item(id)?;
        unsafe {
            let mut info: sys::obs_transform_info = std::mem::zeroed();
            sys::obs_sceneitem_get_info2(item, &mut info);
            let mut crop: sys::obs_sceneitem_crop = std::mem::zeroed();
            sys::obs_sceneitem_get_crop(item, &mut crop);
            let src = sys::obs_sceneitem_get_source(item);
            Ok(json!({
                "pos": vec2_get(&info.pos),
                "rot": info.rot,
                "scale": vec2_get(&info.scale),
                "alignment": info.alignment,
                "bounds_type": info.bounds_type,
                "bounds": vec2_get(&info.bounds),
                "bounds_alignment": info.bounds_alignment,
                "crop": [crop.left, crop.top, crop.right, crop.bottom],
                "source_size": [sys::obs_source_get_width(src), sys::obs_source_get_height(src)],
            }))
        }
    }

    pub fn item_transform_set(&mut self, id: i64, t: &Value) -> Result<()> {
        let item = self.item(id)?;
        let pair = |k: &str| -> Result<(f32, f32)> {
            let a = t[k].as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("'{k}' must be [x, y]"))?;
            Ok((a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32))
        };
        let num = |k: &str| t[k].as_f64().ok_or_else(|| format!("'{k}' must be a number"));
        let (pos, scale, bounds) = (pair("pos")?, pair("scale")?, pair("bounds")?);
        let crop: Vec<i32> = t["crop"].as_array().map(|a| a.iter().map(|v| v.as_i64().unwrap_or(0).max(0) as i32).collect()).unwrap_or_default();
        if crop.len() != 4 {
            return Err("'crop' must be [left, top, right, bottom]".into());
        }
        unsafe {
            let mut info: sys::obs_transform_info = std::mem::zeroed();
            sys::obs_sceneitem_get_info2(item, &mut info);
            info.pos = vec2(pos.0, pos.1);
            info.rot = num("rot")? as f32;
            info.scale = vec2(scale.0, scale.1);
            info.alignment = num("alignment")? as u32;
            info.bounds_type = num("bounds_type")? as sys::obs_bounds_type;
            info.bounds = vec2(bounds.0.max(1.0), bounds.1.max(1.0));
            info.bounds_alignment = num("bounds_alignment")? as u32;
            sys::obs_sceneitem_defer_update_begin(item);
            sys::obs_sceneitem_set_info2(item, &info);
            let c = sys::obs_sceneitem_crop { left: crop[0], top: crop[1], right: crop[2], bottom: crop[3] };
            sys::obs_sceneitem_set_crop(item, &c);
            sys::obs_sceneitem_defer_update_end(item);
        }
        self.touch();
        Ok(())
    }

    pub fn item_transitions(&self, id: i64) -> Result<Value> {
        let item = self.item(id)?;
        Ok(json!({ "show": transition_info(item, true), "hide": transition_info(item, false) }))
    }

    /// Show/hide transition of one item; empty `type_id` removes it.
    pub fn item_transition_set(&mut self, id: i64, show: bool, type_id: &str, duration_ms: u32) -> Result<()> {
        let item = self.item(id)?;
        let tr = if type_id.is_empty() {
            None
        } else {
            Some(Source::create_private(type_id, if show { "Show transition" } else { "Hide transition" }, None)
                .ok_or_else(|| format!("Unknown transition '{type_id}'"))?)
        };
        unsafe {
            sys::obs_sceneitem_set_transition(item, show, tr.as_ref().map_or(std::ptr::null_mut(), |t| t.as_ptr()));
            sys::obs_sceneitem_set_transition_duration(item, show, duration_ms.clamp(50, 20_000));
        }
        self.touch();
        Ok(())
    }
}
