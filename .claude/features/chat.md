# Unified chat (read-only, no login)

## Files
- `src-tauri/src/chat/mod.rs` — hub: history (300), status, overlay subscribers, supervisor with backoff, `ws_connect`, `http`
- `src-tauri/src/chat/twitch.rs` — anonymous IRC over `wss://irc-ws.chat.twitch.tv` (`justinfan*`), PRIVMSG + USERNOTICE (subs, gifts, bits)
- `src-tauri/src/chat/kick.rs` — Pusher `chatrooms.<id>.v2`; app key negotiated each connect via
  `POST web.kick.com/api/v1/realtime/connection` (fallback `34bf7a0ff419a2a775b9`/`us2`); chatroom id from `kick.com/api/v2/channels/<slug>`
- `src-tauri/src/chat/youtube.rs` — InnerTube `youtubei/v1/live_chat/get_live_chat` (unofficial), "Live chat" (all) continuation
- `src-tauri/src/chat/overlay.html` — Browser Source overlay, served at `http://127.0.0.1:<api port>/overlay/chat`
- `src-tauri/src/api.rs` — `/chat` WebSocket (history replay + live), plain-HTTP routing via `peek`
- `src-tauri/src/chat/raw_http.rs` — hand-written HTTPS (Title-Case headers) for Kick
- `src-tauri/src/chat/actions.rs` — send / timeout / ban / unban / delete / mod / VIP per platform
- `src-tauri/src/auth.rs` — Twitch device code, Google loopback+PKCE, Kick loopback (port 17563)+PKCE+secret; `accounts.json`
- `src-tauri/src/popout.rs` — pop-out window (label `chat-popout`), click-through, global hotkey Ctrl+Alt+C
- `src/components/ChatView.tsx` — list, search (`from:name`), user card with moderation, input (shared)
- `src/components/ChatDrawer.tsx` — right drawer that pushes the workspace, resizable width (localStorage)
- `src/PopoutApp.tsx` — pop-out UI (chosen in `main.tsx` by window label), background-alpha slider
- `src/store.ts` (`chatStore`, `chatStatusStore`, `accountsStore`, `authPendingStore`, `popoutStore`), Settings → Chat / Accounts

## Gotchas
- `ChatMessage.msg_id` / `author_id` are the platform's raw ids (needed for delete/ban); channel ids live in
  `ChannelContext` (Twitch room-id from ROOMSTATE, Kick broadcaster `user_id`, YouTube video id -> activeLiveChatId).
- Platform limits: Kick has no mod/VIP API; YouTube cannot unban (needs ban id) and has no VIP.
- Pop-out: never use WS_EX_LAYERED on it (WebView2 renders blank); opacity is the UI background alpha.
  `popout_open` must be async (building a window in a sync command deadlocks on Windows); do not call
  `restore_state` manually (the window-state plugin restores new windows itself; the manual call hung the UI thread).
- `WebviewUrl::App` percent-encodes `#`, so the pop-out loads `index.html` and picks its UI by window label.
- Kick's Cloudflare returns 403 for lower-case header names (ureq/`http` crate) or any Accept-Encoding header.
  `chat::raw_http` writes HTTP/1.0 requests by hand over native-tls with Title-Case headers (also used for the Kick API).
- Kick emotes arrive as `[emote:<id>:<name>]` -> `strip_emotes`.
- YouTube channel input only resolves while the channel is live (`/@handle/live` canonical URL).
- `ChatSettings` change -> `chat::configure` restarts all connectors.
- Events: `chat:message` (ChatMessage), `chat:status` ({platform, state, detail}).
- Message kinds: message | paid (bits, Super Chat/Sticker) | membership (subs, gifts, YouTube members) | system.

## Not yet
Login/send/moderation code is complete but untested until developer client IDs exist (docs/platform-apps.md).
Follow/raid/sub alerts, TikTok, Facebook, donations.
