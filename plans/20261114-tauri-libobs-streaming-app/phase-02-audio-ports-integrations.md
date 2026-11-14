# Phase 2 — Audio safety, plugin ports, control API, platform integrations

Decisions (user, 2026-11-14):
- Bluetooth mic: never auto-open a Bluetooth hands-free mic + warning + "mic only while live/recording" toggle.
- Voice: no built-in effects; provide ports/lib so third parties plug in (VST slot, app audio, audio bridge, WebSocket API).
- Ports: VST slot, audio from other apps, local WebSocket API, WHIP/WebRTC + DeckLink.
- Platforms: Twitch, YouTube, TikTok, Kick, Facebook + donation platforms (PlayerDuo, Gank, Patreon, memberships…).

Root cause (verified in obs.log): opening `Headset (OPPO Enco Air3i)` mic at 16 kHz forces Windows to switch the
earbuds from A2DP (stereo) to HFP (mono 16 kHz) — degrades both headphone audio and captured desktop audio.

## A — DONE 2026-11-14
Verified: API handshake/auth/requests/events (examples/control-api/client.py); Audio Bridge real-time round trip
(4096-frame blocks every ~85 ms, signal preserved); voice_fx.py connects; obs-vst + obs-webrtc load; mic stays closed
with no input device. NOT yet verified on hardware: Bluetooth detection (earbuds were disconnected during testing).

### Items
| # | Item | Verify |
|---|------|--------|
| A1 | Core Audio device enumeration with Bluetooth detection; mic channel optional; BT mic never auto-opened; warning badge | first run with BT-only mic opens no mic; log shows no Headset init |
| A2 | Mic gate: "only while live/recording" | mic created on start, released when all outputs stop |
| A3 | Monitoring device setting + voice Preview (monitor on/off, stream unchanged) + Bypass | |
| A4 | Restore obs-vst, obs-webrtc, decklink modules; WHIP service; Opus audio for services that require it | modules listed as loaded |
| A5 | Plugin folders: app-data plugins + ProgramData/obs-studio/plugins; Plugins tab listing modules | |
| A6 | Audio Bridge filter (Rust-registered libobs filter) + WS protocol + Python example client | example client round-trips audio |
| A7 | Local control API, obs-websocket v5-compatible subset (Stream Deck, Touch Portal, Streamer.bot) | handshake + GetSceneList via test client |
| A8 | Voice panel: chain editor for a chosen audio source | |

## B — DONE 2026-11-14: chat (no developer credentials needed)
Verified live: Twitch #xqc (260 messages + 2 sub notices), Kick xqc (133 messages), YouTube @LofiGirl (50+),
overlay rendered in headless Edge. Bluetooth guard verified on real hardware (OPPO Enco Air3i connected:
mixer shows "Mic not opened", desktop audio unaffected).
Next for C: user registers developer apps (docs/platform-apps.md).

Twitch IRC (anonymous read), Kick (public Pusher socket), YouTube live chat (unofficial innertube), unified chat dock + chat overlay URL.

## C — needs developer apps / credentials from the user
Twitch/YouTube/Facebook OAuth (login, stream key, alerts), TikTok (unofficial signing service).

## D — donations
StreamElements / Streamlabs socket tokens, Ko-fi / Patreon webhooks (need public URL or relay), PlayerDuo / Gank (research API availability).

## Known limitation
obs-vst's editor window is a Qt widget and needs a QApplication; this app has no Qt event loop, so the
"Open Plug-in Interface" button is hidden to avoid a crash. VST plugins run with saved/default parameters.
