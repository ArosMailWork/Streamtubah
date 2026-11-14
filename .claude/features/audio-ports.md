# Audio safety, Voice Studio & plug-in ports

## Files
- `src-tauri/src/audio_devices.rs` — Core Audio endpoints; Bluetooth = enumerator `BTH*` or name contains "Hands-Free"
- `src-tauri/src/studio/channels.rs` — `AudioChannel`: open/close a global channel without losing filters/volume
- `src-tauri/src/studio/mod.rs` — `sync_channels`, `apply_monitoring_device`, `voice_set_bypass`, `modules()`, `HIDDEN_PROPS`
- `src-tauri/src/bridge.rs` — Audio Bridge filter (`streamtubah_audio_bridge`) registered via `obs_register_source_s`
- `src/components/VoiceDialog.tsx`, `src/components/SettingsDialog.tsx` (Audio, Plugins tabs), `src/components/Panels.tsx` (mixer badges/hints)
- `docs/audio-bridge.md`, `examples/audio-bridge/`

## Rules
- A Bluetooth headset mic is NEVER opened via "default": it forces the headset into hands-free mode
  (mono 16 kHz), degrading what the streamer hears and captured desktop audio. Explicit selection is allowed + warned.
- No input device -> mic channel stays closed (no WASAPI retry spam).
- `audio.mic_live_only`: mic opens in `Studio::start_output` and closes once no output is active.
- The stats thread re-runs `sync_channels` every 3 s (hot-plug / default-device changes) and every second when live-only.
- Voice preview toggles monitoring 0 <-> 2 (monitor + output) so the stream mix never changes; monitor-only (1) is left alone.
- Monitoring device comes from settings (`obs_set_audio_monitoring_device`).
- Bypass disables enabled filters and restores exactly those.

## Plug-in ports
- OBS plug-ins: `%APPDATA%/com.streamtubah.app/plugins/<name>/bin/64bit/<name>.dll` + `<name>/data/`,
  plus `%ProgramData%/obs-studio/plugins` when `plugins.load_obs_studio_plugins` (restart required).
- VST 2.x via obs-vst (restored by `scripts/setup-obs.sh`). Editor buttons are hidden: the obs-vst editor is a Qt
  widget and needs a QApplication, so opening it would abort the app.
- Audio Bridge: WebSocket `/audio/<channel>` on the API port, f32le interleaved stereo 48 kHz, latency buffer + underrun re-prime.
- Virtual-mic apps: choose their device as the Mic; per-app audio: "Application Audio Capture" source.
- WHIP/WebRTC service `whip_custom` (obs-webrtc); the Opus audio encoder is used when the service rejects AAC.
- DeckLink module restored (needs Blackmagic Desktop Video drivers).

## Mixer & advanced audio
- `src-tauri/src/studio/audio_adv.rs`: `AudioPatch` {sync_offset_ms (-950..20000), balance 0-1, mono
  (`OBS_SOURCE_FLAG_FORCE_MONO`), tracks bit mask 1-6 (`obs_source_set_audio_mixers`), hidden}. Hidden is stored in
  the source's private settings (`mixer_hidden`) so it persists with the source. Each mixer entry has `advanced`.
- UI (`Panels.tsx`): meter with dB scale, peak hold (1.5 s) and clip light (>= -0.5 dB); the track draws the full
  color scale and a dark cover hides the part above the level (colors stay at fixed dB). Horizontal/vertical mixer
  (localStorage `streamtubah.mixer.vertical`), "+N hidden" toggle, right-click menu, Advanced Audio Properties table.
- Extra global devices: Settings → Audio → Desktop Audio 2, Mic/Aux 2-4 (explicit devices only; extra mics follow
  the live-only gate). Verified: Desktop Audio 2 appears in the mixer and persists.
