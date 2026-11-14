# Auto-Optimize

## Files
- `src-tauri/src/optimize.rs` — hardware info + machine fingerprint, upload test, 5 levels, candidate plan, real
  encode test, `run` (guarded so only one run at a time)
- `src-tauri/src/commands.rs` — `optimize_run(level)` (async, `spawn_blocking`), `optimize_apply(rec)`,
  `optimize_machine_check`, `optimize_dismiss(level)`
- `src-tauri/src/settings.rs` — `OptimizeSettings { level (1-5, default 3), machine }`
- `src/components/AutoOptimize.tsx` — Settings → Auto-Optimize tab (5-level slider, one button, progress, results)
- `src/components/NewMachineDialog.tsx` — shown at startup when this PC differs from the last optimized one

## Behavior
- One click: upload test → hardware → real encode test → recommendation. Nothing is applied until "Apply".
- Upload: 1 MB warm-up POST to `https://speed.cloudflare.com/__up`, then one POST sized for ~3 s (2-40 MB); faster
  result wins. Measures speed to Cloudflare's edge, which can exceed the speed to the streaming ingest.
- Machine fingerprint `machine_id()` = CPU name (registry) | threads | first hardware DXGI adapter. First run or a
  different PC (portable folder moved) → `NewMachineDialog` auto-runs the test and asks; Apply or "Not now" stores the
  fingerprint so it is not asked again on that PC. Never applies on its own.
- Levels (1 = smoother PC … 5 = best stream): max height/FPS 720p30, 720p60, 1080p60, 1080p60, 1440p60; bitrate share
  60/75/100/100/100 % of min(75 % upload − 160, platform cap); presets NVENC p1/p3/p5/p6/p7 (+ multipass
  disabled/disabled/qres/qres/fullres, tune hq), x264 ultrafast/superfast/veryfast/faster/fast, QSV TU7/TU7/TU5/TU3/TU1,
  AMF speed/speed/balanced/balanced/balanced. Above 1440p is left to the user.
- Platform caps: Twitch 6000 (1080p max), YouTube 16000 (1440p ok), Kick 8000, Facebook 6000, custom 12000 (1440p ok).
- Candidates: ladder 1440p60/1440p30/1080p60/1080p30/720p60/720p30/480p30 filtered by level, bitrate minimum,
  canvas (never upscale), platform 1440p support, CPU encoders ≤ 1080p.
- Encode test (`run_trial`): reset video to the candidate output size/FPS, ffmpeg_muxer to a temp MKV with the chosen
  encoder + preset + bitrate and AAC, 1 s warm-up, then 3 s measuring encoder-skipped and render-lagged frames; pass
  at ≤ 0.5 % each. Best-first, stops at the first pass; all failing → lightest. Video settings are restored after each
  trial. Content-light scenes understate x264 load (GPU encoders are mostly content-independent).
- `optimize_apply`: output resolution/FPS, encoder id, merged encoder settings, `optimize.level` and `machine`; refused
  while live. Canvas, scenes and stream keys untouched. Upload result kept in localStorage for the multistream estimate.
- Verified 2026-11-14 (GTX 1050 Ti, i5-14400F, ~425 Mbit/s, Twitch): level 1 → 720p30 3600 kbps p1; level 3 → 1080p60
  6000 kbps p5; level 5 → 1080p60 p7 fullres (Twitch blocks 1440p). New-PC dialog auto-ran and rendered results.

## Exit safety
`cancel_and_wait()` runs first in the app's Exit handler: a running test notices `CANCEL` within 50 ms, force-stops
and releases its output/encoders, skips restoring video, and `RUNNING` drops; then libobs shuts down. Before this,
closing the app during a test crashed it (0xC0000005 inside obs_shutdown). Verified: closing 3/7/12 s into a test exits 0.
