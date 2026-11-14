# Streamtubah

Lightweight streaming/recording studio for Windows — Streamlabs-style UI built with
**Tauri v2 + React**, powered by the official **libobs** (OBS Studio 32.1.0) core.

License: GPL-3.0 (required because the app links libobs).

## Requirements
- Windows 10/11 x64, Rust (stable), Node.js 20+
- `cargo install cargo-obs-build`

## Setup
```bash
npm install
bash scripts/setup-obs.sh debug     # downloads OBS 32.1.0 runtime next to the debug exe
npm run tauri dev
```
For release builds run `bash scripts/setup-obs.sh release` before `npm run tauri build`.

## Architecture
```
React UI (WebView2) ──invoke/events──► Rust (src-tauri/src)
                                        ├─ obs/       safe RAII wrapper over raw libobs FFI
                                        ├─ studio/    scenes, sources, filters, mixer, outputs, persistence
                                        ├─ preview.rs native Win32 child window rendered by obs_display
                                        └─ commands.rs Tauri command surface
```
- Preview is a native window layered over the WebView; HTML cannot draw over it, so it
  is hidden while dialogs are open.
- Every source/filter/encoder/service settings dialog is generated from libobs
  `obs_properties` — no per-plugin UI code.
- Data lives in `%APPDATA%/com.streamtubah.app/` (`scenes.json`, `settings.json`, `obs.log`).

## Extending
- **OBS plug-ins**: put them in `%APPDATA%/com.streamtubah.app/plugins` (Settings → Plugins → Open).
- **VST 2.x** voice plug-ins: Voice Studio → "VST 2.x plug-in".
- **External apps** (voice changers, AI voice, scripts): [docs/audio-bridge.md](docs/audio-bridge.md).
- **Remote control** (Stream Deck, Touch Portal, Streamer.bot…): [docs/control-api.md](docs/control-api.md).
- **Chat**: Twitch, Kick and YouTube chat without login (Settings → Chat); overlay at
  `http://127.0.0.1:4455/overlay/chat`. Platform login setup: [docs/platform-apps.md](docs/platform-apps.md).

## OBS runtime
`cargo-obs-build` strips Qt, `obs-frontend-api.dll` and several modules. `scripts/setup-obs.sh`
restores what this app uses from the official OBS zip: Qt + frontend-api (needed by Browser
Source), obs-vst (VST 2.x), obs-webrtc (WHIP) and decklink.
When distributing, ship the OBS source link with the binaries to satisfy GPL.
