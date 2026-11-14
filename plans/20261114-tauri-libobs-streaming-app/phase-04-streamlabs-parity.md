# Phase 4 — Streamlabs parity (hotkeys, virtual cam, profiles, multistream, login, alerts, donations, update)

Status: 4.1-4.6, 4.8 (except direct Twitch follows), 4.9 (Ko-fi, Patreon, StreamElements, Streamlabs), 4.11 done · 2026-11-14

## Decisions (user)
- Scope: everything except installer and macOS. Ship as a portable zip; auto-update from GitHub Releases
  (`ArosMailWork/Streamtubah`).
- Login like Streamlabs (one click): owner registers Twitch / Google apps, client IDs embedded.
- Streamer must be able to chat on their own channel from the app (send already coded, needs login).
- No relay server. Secrets stay on the streamer's machine, entered in a dedicated window that is excluded
  from screen capture (Windows display affinity / Tauri `content_protected`), inputs masked `*` with an eye toggle.
- Release package strips CEF locales except en-US + vi.

## Work items
| # | Item | Notes | Verify |
|---|------|-------|--------|
| 4.1 | Measure release size + idle RAM | `tauri build --no-bundle` + setup-obs release | numbers in report |
| 4.2 | Global hotkeys | stream/record/replay save/scene switch/mute/studio transition; Settings → Hotkeys; global-shortcut plugin | press while app unfocused |
| 4.3 | Virtual camera | libobs `virtualcam_output`; register `obs-virtualcam-module64.dll` once (UAC) | camera visible in Discord/Camera app |
| 4.4 | Scene collections + profiles | multiple `scenes.json` / settings sets, switch/rename/duplicate | switch restores state |
| 4.5 | Multistream | N stream outputs sharing the encoders, per-target service/key/enable | 2 RTMP targets connect |
| 4.6 | Secrets window | content-protected window, masked inputs, DPAPI-encrypted storage | invisible in display capture + Snipping Tool |
| 4.7 | Platform login + own-channel chat | Twitch device code, Google loopback PKCE (embedded IDs); Kick and Meta credentials via secrets window | login + send message on own channel |
| 4.8 | Alerts | event bus (follow/sub/raid/bits/superchat/member/donation) → `/overlay/alerts`, per-type image/sound/text, queue, test buttons; Twitch EventSub WS | test alert renders in Browser Source |
| 4.9 | Donations | StreamElements + Streamlabs sockets; SePay/Casso + Patreon polling; Ko-fi webhook via on-demand tunnel; PlayerDuo/Gank research first | each source fires an alert |
| 4.10 | TikTok + Facebook chat | TikTok via signing service (unofficial, fragile); Facebook Graph live comments (Meta app review) | messages in chat drawer |
| 4.11 | Auto-update | check GitHub latest release, download app-only zip, verify ed25519 signature, swap exe (rename running exe) on restart; OBS runtime only when its version changes | old build updates to new |

## Deferred by user (2026-11-14)
- 4.10 TikTok + Facebook chat: postponed (TikTok only via an unofficial signing service; Facebook needs Meta App Review).
- 4.7 / direct Twitch follows: waiting for developer app client IDs (docs/platform-apps.md).
- 4.3: virtual camera registration needs one manual UAC confirmation by the user.

## Order
4.1 → 4.2–4.4 → 4.5 → 4.6 → 4.7 → 4.8 → 4.9 → 4.10 → 4.11

## Risks
- Virtual cam registration needs admin once.
- Display affinity does not stop a phone filming the monitor.
- Ko-fi tunnel URL changes per session unless the streamer uses a named tunnel.
- TikTok unofficial API can break any time; Facebook needs Meta app review.
- YouTube Data API quota is shared across all users of the embedded client ID.

## Results
- 4.1 Release (`tauri build --no-bundle`): exe 7.0 MB; portable folder 330 MB / zip 149 MB with CEF locales trimmed
  to en-US + vi (CEF = 205 MB of it). Idle: ~300 MB private RAM (124 core + 177 WebView2), ~0.4 % CPU, no browser source.
- 4.2 Hotkeys: verified mute toggle + scene switch from injected keys; capture field; save.
- 4.3 Virtual camera: button + API status/"not installed" path verified; registration (UAC) and the camera
  appearing in Discord/Camera app still need one manual run by the user.
- 4.4 Collections/profiles: verified via API incl. same-named sources in two collections (needed
  `obs_wait_for_destroy_queue`), invalid names rejected, legacy `scenes.json` migrated.
- Extra: obs-websocket requests for virtual cam / scene collections / profiles (Stream Deck uses them).
- 4.5 Multistream ("combined" encoder mode chosen by user: shared by default, optional own encoder per destination):
  verified with two local RTMP receivers. Vertical (dual) output deferred, like phase 5.7.
- 4.6 Secrets: DPAPI vault + capture-protected Secrets window; full test matrix in `.claude/features/secrets.md`.
  Found and fixed on the way: API restarted with a blank password after saving settings; API listener rebind race.
- App icon replaced (was the default Tauri icon shared with pngtuber); source `src-tauri/icons/app-icon.png`.
- 4.11 Portable packaging + signed self-update (scripts/package.sh, updater.rs); end-to-end tested with a local server.
- OAuth tokens moved into the encrypted vault. Exit crash during Auto-Optimize found and fixed.
- 4.8/4.9 Alerts (Alert Box overlay, Twitch/Kick/YouTube events without login) + Ko-fi (cloudflared quick tunnel) +
  Patreon (polling). Twitch follows wait for Twitch sign-in. User chose Ko-fi + Patreon first (no VietQR /
  StreamElements / TTS for now). Low-memory UI option added (WebView2 189 → 114 MB).
