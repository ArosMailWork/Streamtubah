//! Tauri command surface. Each command locks the studio, does one operation,
//! and returns either data or an error string shown by the UI.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::State;

use crate::settings::AppSettings;
use crate::studio::{PropTarget, Studio, TypeInfo, outputs};

pub struct AppState(pub Arc<Mutex<Studio>>);

type R<T> = Result<T, String>;

fn with<T>(state: &State<AppState>, f: impl FnOnce(&mut Studio) -> R<T>) -> R<T> {
    let mut s = state.0.lock().map_err(|_| "Studio state poisoned".to_string())?;
    f(&mut s)
}

#[tauri::command]
pub fn get_state(state: State<AppState>) -> R<Value> {
    with(&state, |s| Ok(s.state()))
}

// Scenes ---------------------------------------------------------------------

#[tauri::command]
pub fn scene_create(state: State<AppState>, name: String) -> R<String> {
    with(&state, |s| s.scene_create(&name))
}

#[tauri::command]
pub fn scene_remove(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.scene_remove(&name))
}

#[tauri::command]
pub fn scene_rename(state: State<AppState>, name: String, new_name: String) -> R<()> {
    with(&state, |s| s.scene_rename(&name, &new_name))
}

#[tauri::command]
pub fn scene_move(state: State<AppState>, name: String, delta: i32) -> R<()> {
    with(&state, |s| s.scene_move(&name, delta))
}

#[tauri::command]
pub fn scene_set_current(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.scene_set_current(&name))
}

// Sources / items --------------------------------------------------------------

#[tauri::command]
pub fn source_types() -> Vec<TypeInfo> {
    Studio::input_types()
}

#[tauri::command]
pub fn existing_sources(state: State<AppState>) -> R<Vec<String>> {
    with(&state, |s| Ok(s.existing_sources()))
}

#[tauri::command]
pub fn source_create(state: State<AppState>, type_id: String, name: String) -> R<String> {
    with(&state, |s| s.source_create(&type_id, &name))
}

#[tauri::command]
pub fn source_add_existing(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.source_add_existing(&name))
}

#[tauri::command]
pub fn source_rename(state: State<AppState>, name: String, new_name: String) -> R<()> {
    with(&state, |s| s.source_rename(&name, &new_name))
}

#[tauri::command]
pub fn item_remove(state: State<AppState>, id: i64) -> R<()> {
    with(&state, |s| s.item_remove(id))
}

#[tauri::command]
pub fn item_remove_selected(state: State<AppState>) -> R<()> {
    with(&state, |s| s.item_remove_selected())
}

#[tauri::command]
pub fn item_set_visible(state: State<AppState>, id: i64, visible: bool) -> R<()> {
    with(&state, |s| s.item_set_visible(id, visible))
}

#[tauri::command]
pub fn item_set_locked(state: State<AppState>, id: i64, locked: bool) -> R<()> {
    with(&state, |s| s.item_set_locked(id, locked))
}

#[tauri::command]
pub fn item_select(state: State<AppState>, id: Option<i64>) -> R<()> {
    with(&state, |s| s.item_select(id))
}

#[tauri::command]
pub fn item_order(state: State<AppState>, id: i64, movement: String) -> R<()> {
    with(&state, |s| s.item_order(id, &movement))
}

#[tauri::command]
pub fn item_transform(state: State<AppState>, id: i64, action: String) -> R<()> {
    with(&state, |s| s.item_transform(id, &action))
}

// Filters ----------------------------------------------------------------------

#[tauri::command]
pub fn filter_types(state: State<AppState>, source: String) -> R<Vec<TypeInfo>> {
    with(&state, |s| s.filter_types(&source))
}

#[tauri::command]
pub fn filters(state: State<AppState>, source: String) -> R<Vec<Value>> {
    with(&state, |s| s.filters(&source))
}

#[tauri::command]
pub fn filter_add(state: State<AppState>, source: String, type_id: String, name: String) -> R<String> {
    with(&state, |s| s.filter_add(&source, &type_id, &name))
}

#[tauri::command]
pub fn source_set_show_in(state: State<AppState>, source: String, mode: String) -> R<Option<String>> {
    with(&state, |s| s.source_set_show_in(&source, &mode))
}

#[tauri::command]
pub fn filter_remove(state: State<AppState>, source: String, filter: String) -> R<()> {
    with(&state, |s| s.filter_remove(&source, &filter))
}

#[tauri::command]
pub fn filter_set_enabled(state: State<AppState>, source: String, filter: String, enabled: bool) -> R<()> {
    with(&state, |s| s.filter_set_enabled(&source, &filter, enabled))
}

#[tauri::command]
pub fn filter_move(state: State<AppState>, source: String, filter: String, up: bool) -> R<()> {
    with(&state, |s| s.filter_move(&source, &filter, up))
}

// Audio ------------------------------------------------------------------------

#[tauri::command]
pub fn audio_set_volume(state: State<AppState>, name: String, db: f32) -> R<()> {
    with(&state, |s| s.audio_set_volume(&name, db))
}

#[tauri::command]
pub fn audio_set_muted(state: State<AppState>, name: String, muted: bool) -> R<()> {
    with(&state, |s| s.audio_set_muted(&name, muted))
}

#[tauri::command]
pub fn audio_set_monitoring(state: State<AppState>, name: String, mode: i32) -> R<()> {
    with(&state, |s| s.audio_set_monitoring(&name, mode))
}

// Properties -------------------------------------------------------------------

#[tauri::command]
pub fn properties_get(state: State<AppState>, target: PropTarget) -> R<Value> {
    with(&state, |s| s.properties(&target))
}

#[tauri::command]
pub fn properties_update(state: State<AppState>, target: PropTarget, patch: Value) -> R<Value> {
    with(&state, |s| s.properties_update(&target, &patch))
}

#[tauri::command]
pub fn properties_button(state: State<AppState>, target: PropTarget, button: String) -> R<Value> {
    with(&state, |s| s.properties_button(&target, &button))
}

// Outputs ----------------------------------------------------------------------

#[tauri::command]
pub fn stream_start(state: State<AppState>) -> R<()> {
    with(&state, |s| s.start_output("stream"))
}

#[tauri::command]
pub fn record_start(state: State<AppState>) -> R<()> {
    with(&state, |s| s.start_output("record"))
}

#[tauri::command]
pub fn replay_start(state: State<AppState>) -> R<()> {
    with(&state, |s| s.start_output("replay"))
}

#[tauri::command]
pub fn replay_save(state: State<AppState>) -> R<()> {
    with(&state, |s| s.outputs.save_replay())
}

#[tauri::command]
pub fn output_stop(state: State<AppState>, kind: String) -> R<()> {
    with(&state, |s| {
        s.outputs.stop(&kind);
        Ok(())
    })
}

#[tauri::command]
pub fn record_pause(state: State<AppState>, paused: bool) -> R<()> {
    with(&state, |s| s.outputs.set_record_paused(paused))
}

// Settings ---------------------------------------------------------------------

/// Settings for the main window. Secrets are never included (see `secrets.rs`).
#[tauri::command]
pub fn settings_get(state: State<AppState>) -> R<AppSettings> {
    with(&state, |s| {
        Ok(s.settings.clone())
    })
}

#[tauri::command]
pub fn settings_set(app: tauri::AppHandle, state: State<AppState>, mut settings: AppSettings) -> R<()> {
    // Compare and reconfigure with the settings as actually applied: the UI never sends secrets
    // (e.g. the API password arrives blank and is kept by apply_settings).
    let (api, chat, hotkeys, alerts_changed, donate) = with(&state, |s| {
        let before = (s.settings.api.clone(), s.settings.chat.clone(), s.settings.hotkeys.clone());
        let (alerts_before, donate_before) = (s.settings.alerts.clone(), s.settings.donate.clone());
        // Enabled plugins change only through js_plugin_set_enabled (a stale draft must not disable them).
        settings.plugins.js = s.settings.plugins.js.clone();
        s.apply_settings(settings)?;
        let api_changed = serde_json::to_value(&before.0).ok() != serde_json::to_value(&s.settings.api).ok();
        Ok((
            api_changed.then(|| s.settings.with_secrets().api),
            (before.1 != s.settings.chat).then(|| s.settings.chat.clone()),
            (before.2 != s.settings.hotkeys).then(|| s.settings.hotkeys.clone()),
            alerts_before != s.settings.alerts,
            (donate_before != s.settings.donate).then(|| s.settings.donate.clone()),
        ))
    })?;
    if alerts_changed {
        crate::alerts::config_changed();
    }
    if let Some(cfg) = donate {
        crate::alerts::donate::configure(&cfg);
    }
    if let Some(cfg) = api {
        crate::api::configure(&cfg);
        // Plugins carry the API port and must reconnect; windows are rebuilt off the main thread.
        std::thread::spawn(crate::jsplugins::restart_all);
    }
    if let Some(cfg) = chat {
        crate::chat::configure(&cfg);
    }
    if let Some(map) = hotkeys {
        let errors = crate::hotkeys::apply(&app, &map);
        if !errors.is_empty() {
            return Err(format!("Settings saved, but some hotkeys are unavailable:\n{}", errors.join("\n")));
        }
    }
    Ok(())
}

// Chat --------------------------------------------------------------------------

#[tauri::command]
pub fn chat_history() -> Vec<crate::chat::ChatMessage> {
    crate::chat::history()
}

#[tauri::command]
pub fn chat_status() -> Value {
    serde_json::to_value(crate::chat::status()).unwrap_or_default()
}

fn platform_apps(state: &State<AppState>) -> R<crate::settings::PlatformApps> {
    with(state, |s| Ok(s.settings.with_secrets().platform_apps))
}

/// Sends to one platform, or to every signed-in connected platform when `platform` is None.
/// Network calls run off the main thread.
#[tauri::command]
pub async fn chat_send(state: State<'_, AppState>, text: String, platform: Option<String>) -> R<()> {
    let apps = platform_apps(&state)?;
    tauri::async_runtime::spawn_blocking(move || match platform {
        Some(p) => crate::chat::actions::perform(&p, &crate::chat::actions::Action::Send { text }, &apps),
        None => {
            let errors = crate::chat::actions::send_all(&text, &apps);
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.into_iter().map(|(p, e)| format!("{p}: {e}")).collect::<Vec<_>>().join("; "))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn chat_action(state: State<'_, AppState>, platform: String, action: crate::chat::actions::Action) -> R<()> {
    let apps = platform_apps(&state)?;
    tauri::async_runtime::spawn_blocking(move || crate::chat::actions::perform(&platform, &action, &apps))
        .await
        .map_err(|e| e.to_string())?
}

// Accounts ---------------------------------------------------------------------

#[tauri::command]
pub fn auth_accounts() -> Vec<crate::auth::AccountInfo> {
    crate::auth::list()
}

#[tauri::command]
pub fn auth_login(app: tauri::AppHandle, state: State<AppState>, platform: String) -> R<()> {
    let apps = platform_apps(&state)?;
    crate::auth::login(app, platform, apps)
}

#[tauri::command]
pub fn auth_logout(platform: String) {
    crate::auth::logout(&platform);
}

// Chat pop-out -------------------------------------------------------------------

/// Async on purpose: building a window inside a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn popout_open(app: tauri::AppHandle) -> R<()> {
    crate::popout::open(&app)
}

#[tauri::command]
pub fn popout_state() -> Value {
    crate::popout::state()
}

#[tauri::command]
pub fn popout_set_opacity(app: tauri::AppHandle, opacity: f64) {
    crate::popout::set_opacity(&app, opacity);
}

#[tauri::command]
pub fn popout_set_click_through(app: tauri::AppHandle, enabled: bool) {
    crate::popout::set_click_through(&app, enabled);
}

// Audio devices / voice ---------------------------------------------------------

#[tauri::command]
pub fn audio_devices() -> Value {
    serde_json::json!({ "inputs": crate::audio_devices::inputs(), "outputs": crate::audio_devices::outputs() })
}

#[tauri::command]
pub fn voice_set_bypass(state: State<AppState>, source: String, bypass: bool) -> R<()> {
    with(&state, |s| s.voice_set_bypass(&source, bypass))
}

#[tauri::command]
pub fn voice_bypassed(state: State<AppState>, source: String) -> R<bool> {
    with(&state, |s| Ok(s.voice_bypassed(&source)))
}

// Plugins / API -------------------------------------------------------------------

#[tauri::command]
pub fn plugins_info(app: tauri::AppHandle) -> R<Value> {
    let dir = crate::plugins_dir(&app).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "folder": dir,
        "modules": crate::studio::modules(),
        "bridges": crate::bridge::channels(),
    }))
}

#[tauri::command]
pub fn plugins_open_folder(app: tauri::AppHandle) -> R<()> {
    let dir = crate::plugins_dir(&app).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn api_info(state: State<AppState>) -> R<Value> {
    with(&state, |s| {
        let a = &s.settings.api;
        let host = if a.allow_lan { "0.0.0.0" } else { "127.0.0.1" };
        Ok(serde_json::json!({
            "enabled": a.enabled, "port": a.port, "allow_lan": a.allow_lan,
            "url": format!("ws://{host}:{}", a.port),
        }))
    })
}

#[tauri::command]
pub fn video_encoders() -> Vec<outputs::EncoderInfo> {
    outputs::video_encoders()
}

#[tauri::command]
pub fn transition_types() -> Vec<TypeInfo> {
    Studio::transition_types()
}

// Preview ----------------------------------------------------------------------

#[tauri::command]
pub fn preview_set_bounds(state: State<AppState>, display: String, x: i32, y: i32, width: u32, height: u32) -> R<()> {
    with(&state, |s| {
        if let Some(p) = s.previews.get(&display) {
            p.set_bounds(x, y, width, height);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn preview_set_visible(state: State<AppState>, display: String, visible: bool) -> R<()> {
    with(&state, |s| {
        if let Some(p) = s.previews.get(&display) {
            p.set_visible(visible);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn studio_mode_set(state: State<AppState>, enabled: bool) -> R<()> {
    with(&state, |s| {
        s.studio_mode_set(enabled);
        Ok(())
    })
}

#[tauri::command]
pub fn studio_transition(state: State<AppState>) -> R<()> {
    with(&state, |s| s.studio_transition())
}

// Virtual camera / hotkeys -------------------------------------------------------

#[tauri::command]
pub fn virtualcam_toggle(app: tauri::AppHandle) {
    crate::virtualcam::toggle(app);
}

#[tauri::command]
pub fn hotkey_actions() -> Vec<&'static str> {
    crate::hotkeys::ACTIONS.to_vec()
}

// Scene collections / profiles -----------------------------------------------------

#[tauri::command]
pub fn collections(state: State<AppState>) -> R<Value> {
    with(&state, |s| Ok(s.collections()))
}

#[tauri::command]
pub fn collection_switch(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.collection_switch(&name))
}

#[tauri::command]
pub fn collection_create(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.collection_create(&name))
}

#[tauri::command]
pub fn collection_duplicate(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.collection_duplicate(&name))
}

#[tauri::command]
pub fn collection_rename(state: State<AppState>, name: String, new_name: String) -> R<()> {
    with(&state, |s| s.collection_rename(&name, &new_name))
}

#[tauri::command]
pub fn collection_remove(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.collection_remove(&name))
}

#[tauri::command]
pub fn profiles(state: State<AppState>) -> R<Value> {
    with(&state, |s| Ok(s.profiles()))
}

#[tauri::command]
pub fn profile_switch(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.profile_switch(&name))
}

#[tauri::command]
pub fn profile_create(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.profile_create(&name))
}

#[tauri::command]
pub fn profile_rename(state: State<AppState>, name: String, new_name: String) -> R<()> {
    with(&state, |s| s.profile_rename(&name, &new_name))
}

#[tauri::command]
pub fn profile_remove(state: State<AppState>, name: String) -> R<()> {
    with(&state, |s| s.profile_remove(&name))
}

// Source list: folders, ordering, copy/paste ------------------------------------

#[tauri::command]
pub fn item_move(state: State<AppState>, id: i64, parent: Option<i64>, index: usize) -> R<()> {
    with(&state, |s| s.item_move(id, parent, index))
}

#[tauri::command]
pub fn group_create(state: State<AppState>, name: String, ids: Vec<i64>) -> R<i64> {
    with(&state, |s| s.group_create(&name, &ids))
}

#[tauri::command]
pub fn group_ungroup(state: State<AppState>, id: i64) -> R<()> {
    with(&state, |s| s.group_ungroup(id))
}

#[tauri::command]
pub fn item_paste(state: State<AppState>, from_scene: String, from_id: i64, duplicate: bool) -> R<i64> {
    with(&state, |s| s.item_paste(&from_scene, from_id, duplicate))
}

#[tauri::command]
pub fn filters_copy(state: State<AppState>, from: String, to: String) -> R<()> {
    with(&state, |s| s.filters_copy(&from, &to))
}

#[tauri::command]
pub fn item_transform_get(state: State<AppState>, id: i64) -> R<Value> {
    with(&state, |s| s.item_transform_get(id))
}

#[tauri::command]
pub fn item_transform_set(state: State<AppState>, id: i64, transform: Value) -> R<()> {
    with(&state, |s| s.item_transform_set(id, &transform))
}

#[tauri::command]
pub fn item_transitions(state: State<AppState>, id: i64) -> R<Value> {
    with(&state, |s| s.item_transitions(id))
}

#[tauri::command]
pub fn item_transition_set(state: State<AppState>, id: i64, show: bool, type_id: String, duration_ms: u32) -> R<()> {
    with(&state, |s| s.item_transition_set(id, show, &type_id, duration_ms))
}

// Projector ----------------------------------------------------------------------

#[tauri::command]
pub fn projector_monitors(app: tauri::AppHandle) -> Vec<String> {
    crate::projector::monitors(&app)
}

/// Async: creating a window from a sync command deadlocks on Windows.
#[tauri::command]
pub async fn projector_open(app: tauri::AppHandle, monitor: Option<usize>) -> R<()> {
    crate::projector::open(&app, monitor)
}

#[tauri::command]
pub fn audio_set_advanced(state: State<AppState>, name: String, patch: crate::studio::AudioPatch) -> R<()> {
    with(&state, |s| s.audio_set_advanced(&name, &patch))
}

// Auto-Optimize --------------------------------------------------------------------

/// Async: the upload and encode tests take 10-25 s and must not block the UI thread.
#[tauri::command]
pub async fn optimize_run(app: tauri::AppHandle, state: State<'_, AppState>, level: u8) -> R<Value> {
    let studio = state.0.clone();
    let (m, rec) = tauri::async_runtime::spawn_blocking(move || crate::optimize::run(&app, &studio, level))
        .await
        .map_err(|e| e.to_string())??;
    Ok(serde_json::json!({ "measurement": m, "recommendation": rec }))
}

#[tauri::command]
pub fn optimize_apply(state: State<AppState>, rec: crate::optimize::Recommendation) -> R<()> {
    with(&state, |s| {
        let mut next = s.settings.clone();
        next.video.output_width = rec.width;
        next.video.output_height = rec.height;
        next.video.fps_num = rec.fps;
        next.video.fps_den = 1;
        if next.output.video_encoder != rec.encoder_id || !next.output.video_encoder_settings.is_object() {
            next.output.video_encoder_settings = serde_json::json!({});
        }
        next.output.video_encoder = rec.encoder_id.clone();
        if let (Some(dst), Some(src)) = (next.output.video_encoder_settings.as_object_mut(), rec.encoder_settings.as_object()) {
            dst.extend(src.clone());
        }
        next.optimize.level = rec.level;
        next.optimize.machine = crate::optimize::machine_id();
        s.apply_settings(next)
    })
}

/// Whether this PC differs from the one last optimized (first run counts as new).
#[tauri::command]
pub fn optimize_machine_check(state: State<AppState>) -> R<Value> {
    let machine = crate::optimize::machine_id();
    with(&state, |s| {
        Ok(serde_json::json!({ "new_machine": s.settings.optimize.machine != machine, "machine": machine, "level": s.settings.optimize.level }))
    })
}

/// "Not now" on the new-PC prompt: remember this PC without changing settings.
#[tauri::command]
pub fn optimize_dismiss(state: State<AppState>, level: u8) -> R<()> {
    with(&state, |s| {
        s.settings.optimize.machine = crate::optimize::machine_id();
        s.settings.optimize.level = level.clamp(1, 5);
        s.save_settings()
    })
}

// Secrets window -------------------------------------------------------------------

fn require_secrets_window(window: &tauri::WebviewWindow) -> R<()> {
    if window.label() == crate::secrets::LABEL { Ok(()) } else { Err("Secrets can only be read in the Secrets window".into()) }
}

/// Async: creating a window from a sync command deadlocks on Windows.
#[tauri::command]
pub async fn secrets_open(app: tauri::AppHandle, focus: Option<String>) -> R<()> {
    crate::secrets::open(&app, focus)
}

/// Names and "is set" flags only; never values.
#[tauri::command]
pub fn secrets_list(state: State<AppState>) -> R<Value> {
    with(&state, |s| Ok(serde_json::json!({ "items": crate::secrets::list(&s.settings), "lost": crate::vault::lost() })))
}

#[tauri::command]
pub fn secret_get(window: tauri::WebviewWindow, state: State<AppState>, id: String) -> R<String> {
    require_secrets_window(&window)?;
    with(&state, |s| crate::secrets::get(&s.settings, &id))
}

#[tauri::command]
pub fn secret_set(app: tauri::AppHandle, window: tauri::WebviewWindow, state: State<AppState>, id: String, value: String) -> R<()> {
    require_secrets_window(&window)?;
    let api_cfg = with(&state, |s| {
        let api_changed = crate::secrets::set(&s.settings, &id, value.trim())?;
        // Services are rebuilt with the new key on the next start (running streams keep theirs).
        s.outputs.reset_service();
        s.save_settings()?;
        Ok(api_changed.then(|| s.settings.with_secrets().api))
    })?;
    if let Some(cfg) = api_cfg {
        crate::api::configure(&cfg);
    }
    // Relay secret ids equal their vault keys; other ids are ignored.
    crate::alerts::donate::restart_relay(&id);
    use tauri::Emitter;
    let _ = app.emit("secrets:changed", ());
    Ok(())
}

// Updates ----------------------------------------------------------------------

/// Async: network access must not block the UI thread.
#[tauri::command]
pub async fn update_check() -> R<crate::updater::UpdateInfo> {
    tauri::async_runtime::spawn_blocking(crate::updater::check).await.map_err(|e| e.to_string())?
}

/// Downloads, verifies and installs the update; returns the installed version.
#[tauri::command]
pub async fn update_install() -> R<String> {
    tauri::async_runtime::spawn_blocking(crate::updater::install).await.map_err(|e| e.to_string())?
}

/// Restarts into the installed update; refused while live so a stream is never cut.
#[tauri::command]
pub fn update_restart(app: tauri::AppHandle, state: State<AppState>) -> R<()> {
    with(&state, |s| {
        if s.outputs.any_active() {
            return Err("Stop streaming, recording, replay buffer and virtual camera before restarting".into());
        }
        Ok(())
    })?;
    crate::updater::relaunch()?;
    app.exit(0);
    Ok(())
}

// Alerts / donations ------------------------------------------------------------

#[tauri::command]
pub fn alerts_kinds() -> Vec<(&'static str, &'static str)> {
    crate::alerts::KINDS.to_vec()
}

#[tauri::command]
pub fn alerts_test(kind: String) -> R<()> {
    crate::alerts::test(&kind)
}

#[tauri::command]
pub fn alerts_recent() -> Vec<crate::alerts::AlertEvent> {
    crate::alerts::recent()
}

#[tauri::command]
pub fn alerts_replay(id: String) -> R<()> {
    crate::alerts::replay(&id)
}

#[tauri::command]
pub fn alerts_skip() {
    crate::alerts::skip();
}

/// Copies a chosen image/sound into the app's alert-media folder; returns its name.
#[tauri::command]
pub fn alerts_import_media(path: String) -> R<String> {
    crate::alerts::import_media(&path)
}

/// Adds the Alert Box overlay (full canvas, audio through the mixer) to the current scene.
#[tauri::command]
pub fn alerts_add_to_scene(state: State<AppState>) -> R<i64> {
    with(&state, |s| {
        let scene = s.current_scene_name().ok_or("No scene")?;
        let (w, h) = crate::obs::runtime::canvas_size();
        let url = format!("http://127.0.0.1:{}/overlay/alerts", s.settings.api.port);
        let settings = serde_json::json!({ "url": url, "width": w, "height": h, "reroute_audio": true, "shutdown": false });
        s.input_create(&scene, "browser_source", "Alert Box", &settings, true)
    })
}

#[tauri::command]
pub fn donate_status() -> Value {
    crate::alerts::donate::status()
}

// ---------------------------------------------------------------------------
// JavaScript plugins (windows are created/destroyed here: async keeps them off the main thread)

#[tauri::command]
pub fn js_plugins_list() -> Value {
    serde_json::json!({ "folder": crate::jsplugins::folder(), "plugins": crate::jsplugins::list() })
}

/// Enabling approves the permissions currently listed in the plugin's manifest.
#[tauri::command]
pub async fn js_plugin_set_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> R<()> {
    let approved = with(&state, |s| {
        if enabled {
            let perms = crate::jsplugins::manifest_permissions(&id)?;
            s.settings.plugins.js.insert(id.clone(), perms);
        } else {
            s.settings.plugins.js.remove(&id);
        }
        s.save_settings()?;
        Ok(s.settings.plugins.js.clone())
    })?;
    crate::jsplugins::configure(&approved);
    Ok(())
}

#[tauri::command]
pub async fn js_plugin_reload(id: String) -> R<()> {
    crate::jsplugins::reload(&id)
}

#[tauri::command]
pub async fn js_plugin_show(id: String, visible: bool) -> R<()> {
    crate::jsplugins::show(&id, visible)
}

#[tauri::command]
pub fn js_plugin_logs(id: String) -> Vec<Value> {
    crate::jsplugins::logs(&id)
}

#[tauri::command]
pub fn js_plugins_open_folder() -> R<()> {
    tauri_plugin_opener::open_path(crate::jsplugins::folder(), None::<&str>).map_err(|e| e.to_string())
}
