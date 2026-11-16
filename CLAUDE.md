# Streamtubah

Tauri v2 + React UI over official libobs (OBS 32.1.0). GPL-3.0. Windows first.
Setup: see README.md. Plan: `plans/20261114-tauri-libobs-streaming-app/plan.md`.

## Features
| Feature | Doc |
|---|---|
| Studio core (libobs wrapper, scenes, sources, folders, transforms, filters, persistence, scene collections, profiles, Add Source catalog) | `.claude/features/studio-core.md` |
| Native preview & item editing, projector | `.claude/features/preview-editing.md` |
| Outputs (stream, multistream, record, replay buffer, virtual camera, encoders, audio tracks, stream-only / recording-only sources) | `.claude/features/outputs.md` |
| Audio safety (Bluetooth mic), Voice Studio, mixer & advanced audio, extra devices, plug-in ports, Audio Bridge | `.claude/features/audio-ports.md` |
| Remote control API (obs-websocket v5 compatible) | `.claude/features/control-api.md` |
| Unified chat (Twitch, Kick, YouTube): drawer, pop-out, send/moderate, login, overlay | `.claude/features/chat.md` |
| Global hotkeys | `.claude/features/hotkeys.md` |
| Secrets: encrypted vault (stream keys, client secrets, API password) + capture-protected Secrets window | `.claude/features/secrets.md` |
| Auto-Optimize (upload test, encoder/bitrate/resolution recommendation) | `.claude/features/auto-optimize.md` |
| Portable packaging, release signing, self-update, browser component (CEF) downloaded on first use | `.claude/features/updater.md` |
| Alerts (Alert Box overlay, Twitch/Kick/YouTube events) and donations (Ko-fi tunnel, Patreon, StreamElements, Streamlabs) | `.claude/features/alerts.md` |
| Layout editor, modes (Editor/Live/Studio), Studio Mode, resizable panels | `.claude/features/layouts.md` |
| JavaScript plugins (hidden WebView per plugin, permissions, SDK) and Plugin Audio source | `.claude/features/js-plugins.md` |
| VTuber mode (cover real cameras, camera-use detection, allow list, cover image) | `.claude/features/vtuber-mode.md` |
