# Alerts & donations

## Files
- `src-tauri/src/alerts/mod.rs` — `AlertEvent` (`source` = relay name, empty when read directly),
  `AlertStyle`/`AlertSettings` (settings.alerts), event bus (`emit` dedupes by id and across sources, keeps 50 recent, `alert:event` to the UI, `{"type":"alert"|"config"|"skip"}` to overlays), `test`,
  `replay`, `skip`, media import (`<app data>/alert-media/`, random names, PNG/GIF/JPG/WebP/APNG/MP3/WAV/OGG/WebM/MP4,
  ≤ 50 MB) and `read_media` (name-only, no paths)
- `src-tauri/src/alerts/overlay.html` — Alert Box page: queue (one at a time, `gap_ms` between), per-type style,
  template placeholders {name} {amount} {count} {message} {platform} (HTML-escaped), built-in emoji icons and Web Audio
  chimes (no bundled assets), custom image/GIF/video and sound, animations bounce/zoom/slide/fade, min-amount filter,
  live config reload, skip, auto-reconnect
- `src-tauri/src/alerts/donate.rs` — Ko-fi and Patreon (below), settings.donate, starts/stops the relays
- `src-tauri/src/alerts/relay.rs` — StreamElements + Streamlabs sockets and their parsers (below)
- `src-tauri/src/api.rs` — `/overlay/alerts`, `/alerts/config`, `/alerts/media/<name>`, WebSocket `/alerts`
- `src-tauri/src/chat/{twitch,kick,youtube}.rs` — parsers attach `ChatMessage.alert` (not serialized); `chat::publish`
  forwards it to `alerts::emit`
- `src/components/AlertsTab.tsx`, `src/components/DonationsTab.tsx` (Settings → Alerts / Donations)

## Sources
| Event | Source |
|---|---|
| sub (months), gift (count; a mass gift = one alert, its single gifts are skipped), bits (amount, cheermotes stripped from the message), raid (viewers) | Twitch IRC USERNOTICE / PRIVMSG tags (no login) |
| sub (months), gift (count of gifted users), raid (StreamHostEvent viewers) | Kick Pusher events (no login) |
| superchat (Super Chat/Sticker, amount parsed from "$5.00" / "₫50,000"), member, gift (gifted memberships count) | YouTube live chat (no login) |
| donation (Ko-fi Donation/Shop Order), patron (Ko-fi Subscription) | Ko-fi webhook |
| patron (new active patron, $/month) | Patreon polling |
| everything the relay delivers (tip/merch → donation, follow, sub, gifts, cheer → bits, raid/host, superchat, sponsor/member) | StreamElements, Streamlabs sockets |
| follow (direct) | Twitch EventSub — needs Twitch sign-in (client ID); not implemented yet. Available through SE/SL meanwhile |

## Ko-fi
Local listener `127.0.0.1:<donate.kofi_port, 4460>`, only `POST /kofi` (everything else 404) — separate from the API
port, so the tunnel never exposes the control API or overlays. Body `data=<url-encoded JSON>`; `verification_token`
must equal the vault secret `donate.kofi_token` (Secrets window) → else 403. Public URL: `cloudflared tunnel --url`
(quick tunnel, downloaded once to `<app data>/tools/cloudflared.exe`, ~53 MB, started hidden), URL parsed from its
output; changes every start. Disabling or app exit kills it. Measured: ~65 MB private RAM, 0 % CPU idle.

## Patreon
Every 60 s with the creator access token (vault `donate.patreon_token`): `/api/oauth2/v2/campaigns` →
`/campaigns/{id}/members` (pages followed). First successful poll only remembers active patrons; later new
`active_patron` members raise one alert each. `STREAMTUBAH_PATREON_API` overrides the base URL (tests).

## StreamElements / Streamlabs
Settings `donate.streamelements_enabled` / `donate.streamlabs_enabled`; tokens in the vault (`donate.streamelements_token`
= channel JWT, `donate.streamlabs_token` = Socket API token), edited in the Secrets window; saving a token reconnects
(`donate::restart_relay`). One thread per service, reconnect with backoff 2 → 60 s, state in `donate:status`.
- SE: Astro `wss://astro.streamelements.com`, `subscribe` `channel.activities` with `token_type: jwt`; `response` with
  `error` = token refused (real server: `err_unauthorized`); `reconnect` → reconnect. Platform = activity `provider`,
  tips/merch use platform `streamelements`. Gifted `subscriber` → gift ×1 from `sender`.
- SL: socket.io, Engine.IO v3 (`EIO=3`): client sends `2` every `pingInterval`; `44…` = bad token (real server:
  `44"Authentication error"`); `42["event",{type, for, message:[…]}]`. `for` → platform; YouTube `subscription` → member,
  Super Chat amount in micros. Donations/merch use platform `streamlabs`.
- Cross-source dedupe (`is_relay_duplicate`, 30 s): same platform + kind + name (case-insensitive) + count from a
  different source is dropped; a relayed gift ×1 after a mass gift by the same gifter is dropped. Same-source events
  are never merged. Test env: `STREAMTUBAH_SE_WS`, `STREAMTUBAH_SL_WS` override the base URLs.

## Add to scene
`alerts_add_to_scene` creates Browser Source "Alert Box" (canvas size, `reroute_audio` so sounds reach the mixer).

## Verified 2026-11-14 (SE/SL)
Parser tests for every mapped type; mock sockets (subscribe/auth error, EIO3 ping, event, close); dedupe test. Real
endpoints probed with a wrong token (error texts above). Not yet tested with a real token (user has none yet).

## Verified 2026-11-14
Overlay suite 16/16 in headless Edge (render, message, disabled type, min amount, live style change incl. Vietnamese,
custom image, path traversal refused, non-media refused, queue order without overlap, skip, recent + replay, add to
scene); a real Twitch sub from the configured channel arrived as an alert; Ko-fi 11/11 (listener, cloudflared download
+ tunnel, valid/invalid token, duplicate transaction, 404 elsewhere, donation through the public tunnel, tunnel exposes
nothing else, disable/re-enable, exit stops cloudflared); Patreon 4/4 against a mock API; unit tests for every parser.
Found and fixed: API HTTP responses could be cut off (connection reset: request was only peeked); the app aborted on
exit (0x80000003) whenever a Browser Source existed (sources are now removed before obs_shutdown).
