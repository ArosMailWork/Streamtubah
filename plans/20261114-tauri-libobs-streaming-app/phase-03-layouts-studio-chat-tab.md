# Phase 3 — Layout editor, modes, Studio Mode, chat drawer, pop-out, platform login

Decisions (user):
- Layout Editor doubles as the mode switcher. Modes are presets: Editor (all panels), Live (fewer panels,
  chat open), Studio (Preview + Program + Transition).
- Left edge tab → full-width layout drawer slides in from the left; its tab then sits on the right edge;
  clicking it slides the drawer back out, revealing the chosen layout. Ease-in-out animations.
- Users save their own presets; everything resizable by dragging (react-resizable-panels v4).
- Chat is a right-side drawer that PUSHES the preview (native window cannot be overlaid), resizable.
- Chat pop-out: separate always-on-top window, opacity slider, click-through toggled by a global hotkey.
- Chat tab like Streamlabs: send, click name to @mention, scroll back, search by name/message,
  timeout / ban / unban / delete / mod / VIP. Login code complete; enabled once client IDs exist.

Status (2026-11-14): implemented. Verified in the running app: Classic/Live/Studio presets, layout drawer
slide + thumbnails, Studio Mode dual displays + LIVE badge, chat drawer pushing both displays, pop-out render +
Ctrl+Alt+C click-through toggle. Login/send/moderation not verifiable until client IDs are provided.

## Backend
| Item | Notes |
|---|---|
| Multiple displays | `preview.rs`: per-window state via GWLP_USERDATA; render Program (main texture) or a Scene source; editable flag |
| Studio Mode | `Studio::studio_mode`, `preview_scene` (inc/dec showing), `studio_transition`; items/edits target the editing scene |
| Auth | `auth/`: Twitch device code (public client), Google installed-app loopback + PKCE, Kick loopback + PKCE + secret; tokens in accounts.json; refresh |
| Chat actions | send, timeout, ban, unban, delete, mod, vip per platform (Kick: no mod/VIP API; YouTube: no unban without ban id) |
| Pop-out | async `popout_open`, transparent borderless window with UI background alpha (WS_EX_LAYERED blanks WebView2), click-through via set_ignore_cursor_events, global shortcut Ctrl+Alt+C |

## Frontend
Layout model (tree of splits → widgets), presets in localStorage, Layout drawer (preset list + template/slot editor
+ mode), chat drawer with resizable width, ChatView shared by drawer and pop-out, Accounts settings tab.

## Verify
cargo check/test, tsc; run app: switch presets, resize, studio mode transition, chat drawer push + preview bounds,
pop-out opacity/click-through. Login flows cannot be verified until client IDs are provided.

## Revision (user feedback)
Layout editor reworked Unity-style: the drawer is a large preset gallery with generated thumbnails; layouts are
built in the workspace by docking panels (drag header: center swaps, edges split) and resizing, then saved.
Chat tab glued to the chat panel. Verified: gallery, swap dock, edge dock, Edited badge + reset, chat tab flush.
