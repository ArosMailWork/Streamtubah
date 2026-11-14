mod alerts;
mod api;
mod audio_devices;
mod auth;
mod bridge;
mod split;
mod jsplugins;
mod plugin_audio;
mod chat;
mod commands;
mod hotkeys;
mod obs;
mod optimize;
mod popout;
mod preview;
mod projector;
mod secrets;
mod settings;
mod studio;
mod updater;
mod vault;
mod virtualcam;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use commands::AppState;
use settings::AppSettings;
use studio::{Studio, StudioPaths};

/// Set while the app exits: background threads must not call into libobs any more.
pub static EXITING: AtomicBool = AtomicBool::new(false);

fn boot(app: &AppHandle) -> Result<Arc<Mutex<Studio>>, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(data_dir.join("plugin_config"))?;
    let bin_dir = std::env::current_exe()?.parent().ok_or("no exe dir")?.to_path_buf();
    updater::cleanup();
    let settings_path = data_dir.join("settings.json");
    let mut settings = AppSettings::load(&settings_path);
    vault::init(data_dir.join("secrets.bin"));
    auth::init(data_dir.join("accounts.json"));
    migrate_secrets(&data_dir, &mut settings)?;
    // The API always has a password (generated on first run, kept in the vault).
    if vault::get(settings::API_PASSWORD).is_none() {
        vault::set(settings::API_PASSWORD, &settings::random_token(16))?;
    }

    let mut plugin_roots = vec![plugins_dir(app)?];
    if settings.plugins.load_obs_studio_plugins {
        if let Some(pd) = std::env::var_os("ProgramData") {
            plugin_roots.push(std::path::PathBuf::from(pd).join("obs-studio/plugins"));
        }
    }
    let paths = obs::runtime::RuntimePaths { bin_dir, config_dir: data_dir.join("plugin_config"), plugin_roots };
    let version = obs::runtime::startup(&paths, &data_dir.join("obs.log"))?;
    obs::runtime::reset_audio()?;

    if let Err(e) = obs::runtime::reset_video(&settings.video.to_config()) {
        // A bad saved canvas must not brick the app: fall back to defaults.
        eprintln!("video reset failed ({e}), using defaults");
        obs::runtime::reset_video(&settings::VideoSettings::default().to_config())?;
    }
    bridge::register();
    split::register();
    plugin_audio::register();
    obs::runtime::load_modules();
    eprintln!("libobs {version} ready");

    let collection = migrate_collection(&data_dir, &settings.collection)?;
    let studio_paths = StudioPaths { settings: settings_path, data_dir: data_dir.clone(), collection };
    let mut studio = Studio::new(app.clone(), studio_paths, settings);

    let window = app.get_webview_window("main").ok_or("main window missing")?;
    let hwnd = window.hwnd()?.0 as isize;
    for id in ["main", "program"] {
        let display = preview::Preview::create(hwnd, app.clone(), studio.dirty.clone())?;
        // Displays stay hidden until the UI places them.
        display.set_visible(false);
        studio.attach_preview(id, display);
    }
    // Persist the generated API password and any first-run defaults.
    studio.save_settings()?;
    Ok(Arc::new(Mutex::new(studio)))
}

/// Path of the active scene collection. Moves the single-collection `scenes.json`
/// of older versions into `collections/`.
fn migrate_collection(data_dir: &std::path::Path, name: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = data_dir.join("collections");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.json"));
    let legacy = data_dir.join("scenes.json");
    if !path.exists() && legacy.exists() {
        std::fs::rename(&legacy, &path)?;
    }
    Ok(path)
}

/// Moves plaintext stream keys / client secrets of older versions out of settings.json and
/// every profile file into the encrypted vault.
fn migrate_secrets(data_dir: &std::path::Path, settings: &mut AppSettings) -> Result<(), String> {
    settings.take_secrets()?;
    let dir = data_dir.join("profiles");
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let (Some(name), Some("json")) = (path.file_stem().and_then(|n| n.to_str()), path.extension().and_then(|e| e.to_str())) else {
            continue;
        };
        let Some(mut p) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<settings::Profile>(&t).ok()) else {
            continue;
        };
        let mut moved = vault::take_service_secrets(&vault::main_prefix(name), &mut p.stream.service_settings)?;
        for t in &mut p.stream.targets {
            moved |= vault::take_service_secrets(&vault::target_prefix(&t.id), &mut t.service_settings)?;
        }
        if moved {
            let text = serde_json::to_string_pretty(&p).map_err(|e| e.to_string())?;
            settings::write_atomic(&path, &text)?;
        }
    }
    Ok(())
}

/// Per-user plugin folder (OBS layout). Created on first run.
pub fn plugins_dir(app: &AppHandle) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?.join("plugins");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Audio meters at ~30 Hz.
fn spawn_level_timer(app: AppHandle, studio: &Arc<Mutex<Studio>>) {
    let mixer = studio.lock().unwrap().mixer.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(33));
        let levels = match mixer.lock() {
            Ok(m) => m.levels(),
            Err(_) => return,
        };
        let _ = app.emit("obs:levels", levels);
    });
}

/// Performance stats + debounced autosave, once per second.
fn spawn_stats_timer(app: AppHandle, studio: Arc<Mutex<Studio>>) {
    struct Cpu(*mut std::ffi::c_void);
    // SAFETY: only used from this thread.
    unsafe impl Send for Cpu {}
    std::thread::spawn(move || {
        let cpu = Cpu(unsafe { obs::os_cpu_usage_info_start() });
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            tick += 1;
            let Ok(mut s) = studio.lock() else { return };
            if EXITING.load(Ordering::SeqCst) {
                return;
            }
            // Re-evaluate audio channels: closes a live-only mic after outputs stop and
            // reacts to device hot-plug / default-device changes (e.g. a Bluetooth headset
            // becoming the default mic is closed before it degrades the headset).
            if s.settings.audio.mic_live_only || tick % 3 == 0 {
                s.sync_channels(false);
            }
            if s.dirty.swap(false, Ordering::Relaxed) {
                if let Err(e) = s.save_collection() {
                    eprintln!("autosave failed: {e}");
                }
            }
            let cpu_now = unsafe { obs::os_cpu_usage_info_query(cpu.0) };
            api::CPU_USAGE.store(cpu_now.to_bits(), Ordering::Relaxed);
            s.outputs.release_idle_record_view();
            let stats = unsafe {
                let video = obs::sys::obs_get_video();
                json!({
                    "fps": obs::sys::obs_get_active_fps(),
                    "cpu": cpu_now,
                    "render_lagged": obs::sys::obs_get_lagged_frames(),
                    "render_total": obs::sys::obs_get_total_frames(),
                    "encode_skipped": obs::sys::video_output_get_skipped_frames(video),
                    "encode_total": obs::sys::video_output_get_total_frames(video),
                    "stream": s.outputs.stream_stats(),
                    "record_split": s.outputs.record_split_active(),
                    "outputs": s.outputs.status(),
                })
            };
            drop(s);
            let _ = app.emit("obs:stats", stats);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Low-memory mode: WebView2 reads extra browser arguments from the environment when it
/// starts, so this must run before any window is created. Settings are read straight from disk.
fn apply_ui_startup_options() {
    let Some(appdata) = std::env::var_os("APPDATA") else { return };
    let path = std::path::PathBuf::from(appdata).join("com.streamtubah.app").join("settings.json");
    if !AppSettings::load(&path).ui.low_memory {
        return;
    }
    const VAR: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
    let existing = std::env::var(VAR).unwrap_or_default();
    // SAFETY: called at startup before any other thread exists.
    // Software rendering inside the browser process: measured 189 MB -> 114 MB for WebView2.
    unsafe { std::env::set_var(VAR, format!("{existing} --disable-gpu --disable-gpu-compositing --in-process-gpu").trim()) };
}

pub fn run() {
    updater::wait_for_previous_instance();
    apply_ui_startup_options();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([popout::HOTKEY])
                .expect("valid hotkey")
                .with_handler(|app, shortcut, event| {
                    if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    match hotkeys::action_for(shortcut) {
                        Some(action) => hotkeys::dispatch(app, &action),
                        None => popout::toggle_click_through(app),
                    }
                })
                .build(),
        )
        .setup(|app| {
            let studio = boot(app.handle())?;
            spawn_level_timer(app.handle().clone(), &studio);
            spawn_stats_timer(app.handle().clone(), studio.clone());
            let (api_cfg, chat_cfg, hotkey_cfg, donate_cfg, js_plugins) = {
                let s = studio.lock().unwrap();
                let st = &s.settings;
                (st.with_secrets().api, st.chat.clone(), st.hotkeys.clone(), st.donate.clone(), st.plugins.js.clone())
            };
            let data_dir = app.path().app_data_dir()?;
            alerts::init(app.handle(), &data_dir);
            alerts::donate::init(app.handle(), data_dir.clone(), &donate_cfg);
            for e in hotkeys::apply(app.handle(), &hotkey_cfg) {
                eprintln!("hotkey: {e}");
            }
            api::init(app.handle(), studio.clone(), &api_cfg);
            jsplugins::init(app.handle(), &data_dir, &js_plugins);
            chat::init(app.handle(), &chat_cfg);
            app.manage(AppState(studio));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::scene_create,
            commands::scene_remove,
            commands::scene_rename,
            commands::scene_move,
            commands::scene_set_current,
            commands::source_types,
            commands::existing_sources,
            commands::source_create,
            commands::source_add_existing,
            commands::source_rename,
            commands::item_remove,
            commands::item_remove_selected,
            commands::item_set_visible,
            commands::item_set_locked,
            commands::item_select,
            commands::item_order,
            commands::item_transform,
            commands::filter_types,
            commands::filters,
            commands::filter_add,
            commands::filter_remove,
            commands::source_set_show_in,
            commands::filter_set_enabled,
            commands::filter_move,
            commands::audio_set_volume,
            commands::audio_set_muted,
            commands::audio_set_monitoring,
            commands::properties_get,
            commands::properties_update,
            commands::properties_button,
            commands::stream_start,
            commands::record_start,
            commands::replay_start,
            commands::replay_save,
            commands::output_stop,
            commands::record_pause,
            commands::settings_get,
            commands::settings_set,
            commands::video_encoders,
            commands::transition_types,
            commands::preview_set_bounds,
            commands::preview_set_visible,
            commands::studio_mode_set,
            commands::studio_transition,
            commands::audio_devices,
            commands::voice_set_bypass,
            commands::voice_bypassed,
            commands::plugins_info,
            commands::plugins_open_folder,
            commands::api_info,
            commands::chat_history,
            commands::chat_status,
            commands::chat_send,
            commands::chat_action,
            commands::auth_accounts,
            commands::auth_login,
            commands::auth_logout,
            commands::popout_open,
            commands::popout_state,
            commands::popout_set_opacity,
            commands::popout_set_click_through,
            commands::virtualcam_toggle,
            commands::hotkey_actions,
            commands::collections,
            commands::collection_switch,
            commands::collection_create,
            commands::collection_duplicate,
            commands::collection_rename,
            commands::collection_remove,
            commands::profiles,
            commands::profile_switch,
            commands::profile_create,
            commands::profile_rename,
            commands::profile_remove,
            commands::item_move,
            commands::group_create,
            commands::group_ungroup,
            commands::item_paste,
            commands::filters_copy,
            commands::item_transform_get,
            commands::item_transform_set,
            commands::item_transitions,
            commands::item_transition_set,
            commands::projector_monitors,
            commands::projector_open,
            commands::audio_set_advanced,
            commands::optimize_run,
            commands::optimize_apply,
            commands::optimize_machine_check,
            commands::optimize_dismiss,
            commands::secrets_open,
            commands::secrets_list,
            commands::secret_get,
            commands::secret_set,
            commands::update_check,
            commands::update_install,
            commands::update_restart,
            commands::alerts_kinds,
            commands::alerts_test,
            commands::alerts_recent,
            commands::alerts_replay,
            commands::alerts_skip,
            commands::alerts_import_media,
            commands::alerts_add_to_scene,
            commands::donate_status,
            commands::js_plugins_list,
            commands::js_plugin_set_enabled,
            commands::js_plugin_reload,
            commands::js_plugin_show,
            commands::js_plugin_logs,
            commands::js_plugins_open_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // A running Auto-Optimize test owns libobs encoders/outputs: stop it first.
            optimize::cancel_and_wait();
            alerts::donate::shutdown();
            // Hold the Studio lock through obs_shutdown so no other thread enters libobs, and
            // flag the exit so threads that get the lock afterwards return immediately.
            let state = app.try_state::<AppState>();
            let guard = state.as_ref().and_then(|s| s.0.lock().ok());
            EXITING.store(true, Ordering::SeqCst);
            if let Some(mut s) = guard {
                // Removes every source first so Browser Sources close their CEF browsers;
                // CEF aborts the process (0x80000003) if it stops while a browser is alive.
                s.shutdown();
                obs::runtime::shutdown();
            } else {
                obs::runtime::shutdown();
            }
        }
    });
}

