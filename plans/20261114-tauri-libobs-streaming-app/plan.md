# Streamtubah — Tauri v2 + libobs streaming app (Streamlabs-like, lighter)

Status: MVP done (phases 1-8), verified on Windows 11 + GTX 1050 Ti · Started 2026-11-14 · License: GPL-3.0 (required by libobs)

## Decisions (accepted by user)
- Core: official libobs (OBS 32.1.0) via raw FFI crate `libobs` 6.0.0; our own safe wrapper layer.
- UI: Tauri v2 + React + TypeScript. Windows first; keep platform code isolated.
- OBS runtime binaries: official prebuilt, placed next to exe by `cargo-obs-build`.
- Preview: native Win32 child window over the WebView; libobs `obs_display` renders into it.

## Architecture
```
React UI (WebView2) --invoke/events--> Rust (src-tauri)
                                        ├─ obs/      safe wrapper over libobs (RAII refs, data, properties, signals)
                                        ├─ studio/   app model: scenes, sources, mixer, outputs, persistence
                                        ├─ preview/  Win32 child HWND + obs_display + item editing (drag/resize)
                                        └─ commands  Tauri command surface
```

## Phases
| # | Phase | Verify |
|---|-------|--------|
| 1 | Scaffold Tauri v2 + React TS, fetch OBS binaries, boot libobs | app opens, log shows libobs 32.1.0 + modules loaded |
| 2 | Safe wrapper: Data, Source, Scene, SceneItem, Properties, Output/Encoder/Service | cargo check + unit smoke |
| 3 | Preview child window + display render + resize sync | preview shows scene |
| 4 | Scenes / sources CRUD, generic properties form, add-source dialog | add display/window/game/webcam/image/text/browser |
| 5 | Preview item editing: select, move, resize, fit, order, visibility, lock | drag item in preview |
| 6 | Audio mixer: volmeter events, fader, mute, default desktop/mic sources | meters move |
| 7 | Outputs: record (mp4/mkv), stream RTMP, encoder selection (x264/NVENC/QSV/AMF), stats | file plays, RTMP connects |
| 8 | Settings + persistence (scene collection JSON, app settings) | restart restores state |

## Acceptance
- Feature set of Streamlabs core editor (preview, scenes, sources, mixer, record, stream, settings) on Windows.
- Idle RAM / CPU noticeably below Streamlabs Desktop.

## Verification (2026-11-14)
- Boot: libobs 32.1.0, 18 modules incl. obs-browser (CEF 127), NVENC detected.
- Display capture + text source + fit transform rendered in native preview with selection handles.
- 5 s NVENC recording -> 3.85 MB MP4; stream to unreachable server -> "Could not connect to server".
- Idle: ~0.2-0.9 % CPU at 30 fps.

## Next
- Release bundling of the OBS runtime (Tauri resources / NSIS), hotkeys, studio mode, virtual camera, scene collections/profiles, macOS.

## Phase 2
See `phase-02-audio-ports-integrations.md`. Part A (audio safety, ports, control API) done 2026-11-14;
B (chat) done; C (OAuth platforms: waiting for developer app client IDs) and D (donations) pending.

## Phase 6
See `phase-06-se-sl-split-output-plugins.md`: StreamElements/Streamlabs events, stream-only / recording-only sources
(phase 5.7), JavaScript plugins + Plugin Audio source. Done 2026-11-14.

## Risks
- WebView2 vs native child z-order and DPI scaling → compute physical px from devicePixelRatio.
- HTML popups cannot overlay preview → hide preview while modals open.
- libobs thread rules: obs_display draw callback runs on graphics thread; keep callbacks lock-free.
