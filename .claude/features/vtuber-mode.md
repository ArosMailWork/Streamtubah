# VTuber mode

Keeps the real camera (face) off preview, stream, recordings and virtual camera. Other apps are never closed or
blocked (user decision: cover + warn only).

## Files
- `src-tauri/src/vtuber.rs` — settings `vtuber {enabled, allowed_apps, cover_image}`, camera-user polling,
  cover filter `streamtubah_vtuber_cover`, `refresh_sources`, `tick`, `configure`, `status` (`vtuber:status` event)
- `src-tauri/assets/vtuber-cover.gif` — default cover: the blonde sample character of pngtubah
  (`pngchuba/pngtubah/src/assets/sample/gif-{closed,open}.png`), 4 frames (mouth closed/open), "Camera hidden" caption,
  edges flattened on the background colour #1E2130 (= `BG` in vtuber.rs). Written to `<app data>/vtuber-cover-default.gif`.
- `src/components/VtuberTab.tsx` — Settings → VTuber mode (toggle, cover import/default, apps using the camera now with
  "Allow this app", allow list chips) and `VtuberToggle` in the top bar (off / on / ⚠ n).
- Commands `vtuber_status`, `vtuber_import_cover` (PNG/GIF/JPG/WebP → alert-media via `alerts::import_media`);
  `settings_set` calls `vtuber::configure` when `vtuber` changed.

## Behaviour
- Camera users: HKCU `...\CapabilityAccessManager\ConsentStore\webcam` (+ `\NonPackaged` for desktop apps, key = path
  with `#` for `\`); in use = `LastUsedTimeStart != 0 && LastUsedTimeStop == 0`. Polled each second in the stats timer
  (Studio lock held). Streamtubah's own exe is ignored (its webcam sources are always covered).
- Allowed = file/package name contains an allow-list entry (case-insensitive). Default list: VTube Studio, VSeeFace,
  Warudo, VNyan, Animaze, 3tene, XR Animator, Webcam Motion Capture, PNGTuber.
- Covered sources (`should_cover`): `dshow_input` always; `monitor_capture` while any disallowed app uses the camera;
  `window_capture` / `game_capture` whose `window` setting's exe is such an app; `browser_source` when
  `obs-browser-page.exe` uses the camera. Other types untouched.
- Each covered-type source gets one cover filter (private, last in chain = MOVE_TOP, hidden from the filter list, extra
  copies removed). `covered` is an AtomicBool in the filter data (`obs_obj_get_data`), true from creation. Render:
  solid `BG` fill + cover image (private `image_source`, `inc_showing` so GIFs animate) fitted to 80 % and centred;
  the image is read with `try_lock` so the graphics thread never waits.
- `refresh_sources` runs on every `Studio::touch` (new/changed sources are covered immediately) and each tick;
  turning the mode off removes all cover filters. New disallowed app → `app:error` toast once per use.
- Exit: `vtuber::shutdown` releases the image before sources are removed.

## Verified 2026-11-16
Unit tests (allow list, cover rules incl. escaped window strings). In the app with a simulated camera user
(temporary HKCU key for `C:\FakeTest\Discord.exe`, removed afterwards; this PC has no camera): Display Capture shown
normally → covered (background 29,32,47 ≈ #1E2130, character centred) while "Discord" used the camera → shown again
when released → not covered once "discord" was allowed; imported PNG cover shown; non-image import refused; Settings
tab + top bar "⚠ 1" rendered; clean exit (no leaks). Not verified with a real webcam source (no device).
