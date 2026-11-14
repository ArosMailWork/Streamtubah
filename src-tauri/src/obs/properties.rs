//! Serializes `obs_properties_t` into a JSON description the UI renders as a
//! generic form. This is how every source, filter, encoder and service gets a
//! settings dialog without per-plugin UI code.

use serde_json::{Value, json};

use super::{Data, cstr, from_cstr, sys};

/// Owned `obs_properties_t`.
pub struct Properties(*mut sys::obs_properties_t);

impl Properties {
    pub fn from_raw(ptr: *mut sys::obs_properties_t) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }

    /// Runs every property's "modified" callback so visibility/list contents
    /// reflect the given settings.
    pub fn apply_settings(&self, settings: &Data) {
        unsafe { sys::obs_properties_apply_settings(self.0, settings.as_ptr()) }
    }

    pub fn get(&self, name: &str) -> *mut sys::obs_property_t {
        let n = cstr(name);
        unsafe { sys::obs_properties_get(self.0, n.as_ptr()) }
    }

    pub fn to_json(&self) -> Value {
        describe_list(self.0)
    }
}

impl Drop for Properties {
    fn drop(&mut self) {
        unsafe { sys::obs_properties_destroy(self.0) }
    }
}

fn describe_list(props: *mut sys::obs_properties_t) -> Value {
    let mut out = Vec::new();
    let mut p = unsafe { sys::obs_properties_first(props) };
    while !p.is_null() {
        out.push(describe(p));
        if !unsafe { sys::obs_property_next(&mut p) } {
            break;
        }
    }
    Value::Array(out)
}

fn describe(p: *mut sys::obs_property_t) -> Value {
    unsafe {
        let ty = sys::obs_property_get_type(p);
        let mut v = json!({
            "name": from_cstr(sys::obs_property_name(p)),
            "description": from_cstr(sys::obs_property_description(p)),
            "long_description": from_cstr(sys::obs_property_long_description(p)),
            "visible": sys::obs_property_visible(p),
            "enabled": sys::obs_property_enabled(p),
        });
        let o = v.as_object_mut().unwrap();
        let kind = match ty {
            sys::obs_property_type_OBS_PROPERTY_BOOL => "bool",
            sys::obs_property_type_OBS_PROPERTY_INT => {
                o.insert("min".into(), json!(sys::obs_property_int_min(p)));
                o.insert("max".into(), json!(sys::obs_property_int_max(p)));
                o.insert("step".into(), json!(sys::obs_property_int_step(p)));
                o.insert("suffix".into(), json!(from_cstr(sys::obs_property_int_suffix(p))));
                o.insert(
                    "slider".into(),
                    json!(sys::obs_property_int_type(p) == sys::obs_number_type_OBS_NUMBER_SLIDER),
                );
                "int"
            }
            sys::obs_property_type_OBS_PROPERTY_FLOAT => {
                o.insert("min".into(), json!(sys::obs_property_float_min(p)));
                o.insert("max".into(), json!(sys::obs_property_float_max(p)));
                o.insert("step".into(), json!(sys::obs_property_float_step(p)));
                o.insert("suffix".into(), json!(from_cstr(sys::obs_property_float_suffix(p))));
                o.insert(
                    "slider".into(),
                    json!(sys::obs_property_float_type(p) == sys::obs_number_type_OBS_NUMBER_SLIDER),
                );
                "float"
            }
            sys::obs_property_type_OBS_PROPERTY_TEXT => {
                let t = match sys::obs_property_text_type(p) {
                    sys::obs_text_type_OBS_TEXT_PASSWORD => "password",
                    sys::obs_text_type_OBS_TEXT_MULTILINE => "multiline",
                    sys::obs_text_type_OBS_TEXT_INFO => "info",
                    _ => "default",
                };
                let info = match sys::obs_property_text_info_type(p) {
                    sys::obs_text_info_type_OBS_TEXT_INFO_WARNING => "warning",
                    sys::obs_text_info_type_OBS_TEXT_INFO_ERROR => "error",
                    _ => "normal",
                };
                o.insert("text_type".into(), json!(t));
                o.insert("info_type".into(), json!(info));
                "text"
            }
            sys::obs_property_type_OBS_PROPERTY_PATH => {
                let t = match sys::obs_property_path_type(p) {
                    sys::obs_path_type_OBS_PATH_FILE_SAVE => "file_save",
                    sys::obs_path_type_OBS_PATH_DIRECTORY => "directory",
                    _ => "file",
                };
                o.insert("path_type".into(), json!(t));
                o.insert("filter".into(), json!(from_cstr(sys::obs_property_path_filter(p))));
                o.insert("default_path".into(), json!(from_cstr(sys::obs_property_path_default_path(p))));
                "path"
            }
            sys::obs_property_type_OBS_PROPERTY_LIST => {
                let format = sys::obs_property_list_format(p);
                let editable = sys::obs_property_list_type(p) == sys::obs_combo_type_OBS_COMBO_TYPE_EDITABLE;
                let count = sys::obs_property_list_item_count(p);
                let items: Vec<Value> = (0..count)
                    .map(|i| {
                        let value = match format {
                            sys::obs_combo_format_OBS_COMBO_FORMAT_INT => json!(sys::obs_property_list_item_int(p, i)),
                            sys::obs_combo_format_OBS_COMBO_FORMAT_FLOAT => {
                                json!(sys::obs_property_list_item_float(p, i))
                            }
                            sys::obs_combo_format_OBS_COMBO_FORMAT_BOOL => {
                                json!(sys::obs_property_list_item_bool(p, i))
                            }
                            _ => json!(from_cstr(sys::obs_property_list_item_string(p, i))),
                        };
                        json!({
                            "name": from_cstr(sys::obs_property_list_item_name(p, i)),
                            "value": value,
                            "disabled": sys::obs_property_list_item_disabled(p, i),
                        })
                    })
                    .collect();
                let f = match format {
                    sys::obs_combo_format_OBS_COMBO_FORMAT_INT => "int",
                    sys::obs_combo_format_OBS_COMBO_FORMAT_FLOAT => "float",
                    sys::obs_combo_format_OBS_COMBO_FORMAT_BOOL => "bool",
                    _ => "string",
                };
                o.insert("format".into(), json!(f));
                o.insert("editable".into(), json!(editable));
                o.insert("items".into(), Value::Array(items));
                "list"
            }
            sys::obs_property_type_OBS_PROPERTY_COLOR => "color",
            sys::obs_property_type_OBS_PROPERTY_COLOR_ALPHA => "color_alpha",
            sys::obs_property_type_OBS_PROPERTY_BUTTON => "button",
            sys::obs_property_type_OBS_PROPERTY_FONT => "font",
            sys::obs_property_type_OBS_PROPERTY_EDITABLE_LIST => {
                let t = match sys::obs_property_editable_list_type(p) {
                    sys::obs_editable_list_type_OBS_EDITABLE_LIST_TYPE_FILES => "files",
                    sys::obs_editable_list_type_OBS_EDITABLE_LIST_TYPE_FILES_AND_URLS => "files_and_urls",
                    _ => "strings",
                };
                o.insert("list_type".into(), json!(t));
                o.insert("filter".into(), json!(from_cstr(sys::obs_property_editable_list_filter(p))));
                "editable_list"
            }
            sys::obs_property_type_OBS_PROPERTY_FRAME_RATE => "frame_rate",
            sys::obs_property_type_OBS_PROPERTY_GROUP => {
                let checkable = sys::obs_property_group_type(p) == sys::obs_group_type_OBS_GROUP_CHECKABLE;
                o.insert("checkable".into(), json!(checkable));
                o.insert("children".into(), describe_list(sys::obs_property_group_content(p)));
                "group"
            }
            _ => "unknown",
        };
        o.insert("type".into(), json!(kind));
        v
    }
}
