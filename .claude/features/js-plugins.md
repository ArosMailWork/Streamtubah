# JavaScript plugins & Plugin Audio source

User-written plugins (JS), each in a hidden WebView window, talking to the app only through the local API
with a per-run token limited to approved permissions. User guide: `docs/js-plugins.md`.

## Files
- `src-tauri/src/jsplugins.rs` — discovery (`<app data>/plugins-js/<id>/manifest.json`), approval check, window
  lifecycle, tokens, logs (300 lines/plugin, `jsplugins:log`), file serving, request → permission map
  (`permission_for`), `jsplugins:changed` event
- `src-tauri/src/jsplugins_sdk.js` — `window.streamtubah` (injected with `initialization_script` after
  `window.__STREAMTUBAH__ = {id, name, port, token, permissions}`)
- `src-tauri/src/plugin_audio.rs` — libobs input `streamtubah_plugin_audio` (setting `channel`), registry
  channel → source pointers, `push` (obs_source_output_audio, continuous timestamps via `Clock`, resync > 200 ms)
- `src-tauri/src/api.rs` — `/plugin?id=&token=` (control session with `PluginAuth`), `/plugins/<id>/<path>` (HTTP),
  `/audio-source/<ch>`, `audio_authorized` (password or plugin token with `audio`) for `/audio/` too.
  `/plugin`, `/plugins/`, plugin-token audio auth: loopback peers only.
- `src-tauri/src/commands.rs` — `js_plugins_list`, `js_plugin_set_enabled` (async; approves the manifest's
  current permissions, saves `settings.plugins.js`), `js_plugin_reload`, `js_plugin_show`, `js_plugin_logs`,
  `js_plugins_open_folder`; `settings_set` keeps `plugins.js` from the backend (stale drafts cannot disable plugins)
  and restarts plugins (thread) when API settings change
- `src/components/JsPlugins.tsx` (Settings → Plugins, top), `src/components/sourceCatalog.ts` (Plugin Audio entry)
- `examples/js-plugins/voice-fx/` — sample (mic → robot/radio/echo/cave → creates "Voice FX" source, channel `voice-fx`)

## Behaviour
- Settings `plugins.js: { id: [approved permissions] }`. `configure` starts enabled plugins whose manifest
  permissions ⊆ approved; otherwise error "asks for a new permission …" until enabled again. Manifest errors
  (missing file, bad `main` path, unknown permission) show in the list; such plugins cannot be enabled.
- Permissions: scenes, inputs, outputs, collections, audio, mic. Requests: `GetVersion`/`GetStats`/
  `BroadcastCustomEvent` free; unknown request types need permission "all" (never granted). Event subscriptions are
  masked (General always; Scenes/Inputs/Outputs by permission). Plugin-only requests: `PluginLog {level,text}`,
  `PluginSetWindowVisible {visible}` (spawned thread), `PluginInfo`.
- Window `plugin-<id>`: `WebviewUrl::External(http://127.0.0.1:<port>/plugins/<id>/<main>)`, hidden, close button
  hides. No capability lists `plugin-*`, so Tauri IPC is refused ("not allowed. Plugin not found").
  `configure_webview`: `controller.SetIsVisible(true)` (no background throttling while hidden) and a WebView2
  PermissionRequested handler: microphone allowed only with `mic`, everything else denied (no hanging prompts).
- `main` ending in `.js` → generated `__main.html`. Files: `safe_relative` (normal components only, no `\` or `:`),
  served only while the plugin runs, MIME by extension.
- Requires the API listener (`api::is_listening`); otherwise the plugin shows an error.
- Windows are created/destroyed from async commands or spawned threads, never from sync commands.

## Verified 2026-11-14
Unit tests (paths, permission map, manifest checks, clock). In the app with a tester plugin: timer 30 ticks/3 s while
hidden; allowed request ok; `inputs` request denied with message; Tauri invoke denied; notification/geolocation
denied immediately; 440 Hz from an OscillatorNode via `connectNode` → Plugin Audio source → MKV peak 439.9 Hz,
RMS 0.212; wrong token closes `/plugin` and `/audio-source`; path traversal variants 404; new permission in manifest
stops the plugin until re-approved; disable destroys the window. Voice FX sample with Chromium's fake microphone:
mic granted, "Voice FX" source created, effected audio in the recording; window screenshot OK.
Not verified with a real microphone (none connected during testing).
