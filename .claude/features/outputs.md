# Outputs: stream, record, replay buffer

## Files
- `src-tauri/src/studio/outputs.rs`
- `src/components/Controls.tsx`, `src/components/SettingsDialog.tsx`

## Behavior
- One shared video + audio encoder pair (simple mode), created lazily on first start and invalidated when settings change while idle.
- Video encoder default: first hardware texture (zero-copy GPU) encoder, H.264 preferred (`resolve_video_encoder`); settings edited through the encoder's own properties form (`PropTarget::VideoEncoder`).
- Audio: `ffmpeg_aac` (record/replay/most services) or `ffmpeg_opus` when the service lists Opus but not AAC (WHIP); bitrate from settings, 48 kHz stereo fixed.
- All starts go through `Studio::start_output` (opens a live-only mic first). `Outputs::detail(kind)` feeds the control API.
- Stream: service `rtmp_common`/`rtmp_custom`; output type from `obs_service_get_preferred_output_type`; reconnect 20 x 2 s.
- Record: `ffmpeg_muxer`; formats mp4, fragmented mp4 (default, crash-safe), mkv, mov, flv; pause supported.
- Replay buffer: `replay_buffer`, `save` proc; saved path reported via `get_last_replay`.
- Virtual camera: `virtualcam_output` (raw program video, no encoders), kind `virtualcam`. `src-tauri/src/virtualcam.rs`
  checks the DirectShow CLSID `{A3FCE0F5-3493-419F-958A-ABA1250EC20B}` (64 + 32-bit views) and registers
  `data/obs-plugins/win-dshow/obs-virtualcam-module{64,32}.dll` with one elevated `cmd /s /c regsvr32 …` (one UAC prompt).
  `virtualcam::toggle` runs on its own thread (the UAC wait must not block the UI); errors -> `app:error` event (toast).
  An existing OBS Studio registration is reused.
- Mic "live only" gate uses `any_audio_active()` (stream/record/replay), so the virtual camera does not open the mic;
  `any_active()` (incl. virtual camera) still blocks video/profile/collection changes.
- Audio tracks: one AAC encoder per track (`aac_enc[0..6]`, mixer index = track). Record and replay buffer write
  `output.record_tracks` (bit mask, FLV keeps the first); the stream sends `output.stream_track`. Verified: a 2-track
  MKV has `audio_track1` + `audio_track2`.
- Signals -> `obs:output` event `{output, signal, code?, error?, path?}`. The UI refreshes on each signal and also
  when `obs:stats.outputs` (every second) differs from the shown state (`src/store.ts`): on `stop` the output can
  still report active, which used to leave the button on "Stop Rec".

## Stream-only / recording-only sources
- `src-tauri/src/split.rs`; UI: Sources context menu → "Show in" (video sources), badge "📡 only" / "⏺ only".
- Per **source** (not per item): a hidden video filter `streamtubah_output_gate` (settings `mode` = stream|record,
  first in the chain, `CAP_DISABLED` so it is not in "add filter", skipped by `filters()` / voice bypass, kept by
  "Paste Filters"). Saved with the collection like any filter. "both" removes it.
- Record canvas: `RecordView` = `obs_view` + `obs_view_add2` (canvas video info) whose only source is the private
  `streamtubah_record_program`; it renders output channel 0 (transition = program, so transitions are recorded) with a
  graphics-thread flag set. The gate draws (`obs_source_skip_video_filter`) or skips depending on that flag.
  Main mix (preview, program, stream, multistream, virtual camera) = stream version.
- `Outputs::record_video_encoder`: when record + replay are idle and `split::any_split()`, creates the view and its own
  video encoder `record_video` (same type/settings as main); recording and replay buffer share it. Otherwise they share
  `main_video`. `release_idle_record_view` (stats tick, 1 s) frees both once record + replay stop. Field order: encoder
  is dropped before the view.
- Changing "Show in" while a recording runs on the shared canvas returns a note (toast): applies to the next recording.
  While the record canvas is running, changes apply live. Audio is not split.
- Verified 2026-11-14 (NVENC, 1920x1080 → 1280x720): local RTMP receiver got the stream-only box and not the
  recording-only one; the MKV the opposite; replay buffer like the MKV; settings survived a collection reload; with
  every source on "both" the recording showed both boxes and no record canvas was created.

## Multistream
- `src-tauri/src/studio/outputs/multistream.rs`, settings `stream.main_enabled` + `stream.targets: Vec<StreamTarget>`
  {id, name, enabled, service_type, service_settings, own_encoder, bitrate_kbps} (part of the profile).
- Every destination is its own libobs stream output sent straight to its platform (no relay). Default: shares the
  main video encoder and the stream-track audio encoder (bytes identical to main, no extra GPU). `own_encoder`:
  separate video encoder (main encoder type + settings, CBR at `bitrate_kbps`) kept in `TargetOut` while running.
- `start_stream` starts main (if enabled) + enabled destinations; destinations that start keep running and failures
  are joined into the error. `stop("stream")` stops all. `is_running("stream")` / `any_*active` include destinations.
- While live, `apply_settings` calls `sync_destinations`: toggling main or a destination starts/stops it; removed ones
  stop. Encoder/bitrate changes apply the next time that destination starts.
- Signals use kind `stream:<id>` and a `label` ("Stream · <name>") shown in toasts. The control API reports only the
  aggregate stream (StreamStateChanged comes from main; `stream:*` events are not mapped). GetStreamStatus falls
  back to the first live destination when main is off.
- `stream_stats` = totals (kbps, dropped, total, max congestion) + `destinations` list; `state.destinations` =
  [{id, name, enabled, active, reconnecting}] (main first) for the Controls panel.
- Service forms: `PropTarget::TargetService { id }` (service object per destination in `target_services`); the
  Settings draft keeps backend-owned `service_settings` per destination on save.
- UI: `src/components/Destinations.tsx` (Settings → Stream editor, upload estimate vs last Auto-Optimize result
  in localStorage `streamtubah.upload_mbps`, own-encoder GPU note; Controls panel list with live toggles).
- Verified 2026-11-14 with two local RTMP listeners (PyAV): both received H.264 + AAC; shared mode delivered identical
  bytes (7.0 Mbit/s each), own-encoder destination at 2500 kbps delivered ~2.9 Mbit/s while main stayed ~7.0.
