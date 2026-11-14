# Phase 6 — StreamElements/Streamlabs events, stream-only/record-only items, JS plugins

Status: 6.1-6.3 done · 2026-11-14 (6.1 not yet tested with real SE/SL tokens; 6.3 not tested with a real microphone)

## Decisions (user)
- SE/SL: take all events; duplicates of events already read directly (Twitch IRC, Kick, YouTube) are dropped.
- 5.7: stream and recording are different outputs. Program (live) = stream view (virtual cam too);
  recording + replay buffer get their own render with "recording only" items and without "stream only" items.
  Preview behaviour stays the Studio Mode one (preview = next scene, program = on air).
- User plugins belong to Streamtubah (not Agent-Desk). Written in JavaScript, embedded. Plugins handle their own
  UI but may use the Streamtubah API. Enabled per plugin after a red warning.
- Agent-Desk 6b stays pending.

## Work items
| # | Item | Verify |
|---|------|--------|
| 6.1 | StreamElements Astro WS (`channel.activities`, JWT in vault) + Streamlabs socket.io (socket token in vault) → alerts; cross-source dedupe (kind + platform + name + count, 30 s) | parser unit tests, mock WS servers fire alerts, duplicates dropped |
| 6.2 | Per-source "Show in: both / stream / recording" (chosen over per-item: no scene duplication, eye toggle untouched); second video mix for recording when any item is split; recording forces its own encoder | MP4 contains record-only item, stream/program does not |
| 6.3 | JS plugins: `<app data>/plugins-js/<id>/manifest.json` + `main.html`/`main.js`, hidden WebView per plugin served from the API port, scoped API token by declared permissions, Plugin Audio Source (PCM in over WS), Plugins tab (enable with warning, show window, reload, log) | sample voice-changer plugin pipes mic through Web Audio into the stream |

## Results
- 6.1 `alerts/relay.rs`; parser + mock-socket + dedupe tests; real endpoints answer a wrong token as expected
  (SE `err_unauthorized`, SL `44"Authentication error"`). Details: `.claude/features/alerts.md`.
- 6.2 `split.rs` (gate filter per source + record canvas with its own encoder, only while needed). Stream (local RTMP)
  and MKV/replay verified with stream-only / recording-only boxes. Details: `.claude/features/outputs.md`.
- 6.3 `jsplugins.rs`, `jsplugins_sdk.js`, `plugin_audio.rs`, Settings → Plugins, `docs/js-plugins.md`, sample
  `examples/js-plugins/voice-fx`. Permission, isolation and audio checks: `.claude/features/js-plugins.md`.
- Fixed after review (user request): the Record button could stay on "Stop Rec" after a stop (the UI refreshed once on
  the `stop` signal while the output still reported active). The UI now resyncs when the per-second `obs:stats`
  output state differs; 5/5 start/stop cycles showed the right button.

## Risks
- SE/SL payload fields are partly undocumented: parse defensively, ignore unknown types.
- Second render costs GPU; only active while a split item exists and recording/replay runs.
- Plugin WebViews share the WebView2 browser process; a runaway plugin can still use CPU. Plugin pages have no Tauri IPC.
