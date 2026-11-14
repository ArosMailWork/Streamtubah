# Native preview & item editing

## Files
- `src-tauri/src/preview.rs` — Win32 child window (`StreamtubahPreview`) + `obs_display`
- `src/components/Preview.tsx` — sends physical-pixel bounds (rect * devicePixelRatio) via `preview_set_bounds`
- `src/store.ts` — `useModalGate`: hides preview while any modal is open

## Displays
- Two native displays: `main` (editable; program output normally, preview scene in Studio Mode) and `program`
  (Studio Mode only, not editable). State per window via GWLP_USERDATA; commands take a `display` id.

## Behavior
- Draw callback (graphics thread): letterboxed `obs_render_main_texture`, then selection boxes + 8 handles (solid effect).
- Selection state is libobs' own (`obs_sceneitem_select`), shared with the Sources panel.
- Mouse: click selects (Ctrl toggles), drag moves with snapping to canvas edges/center (Ctrl disables snap),
  handles resize unrotated & unflipped items (corners keep aspect; Shift = free). Locked items cannot move.
- Keys: arrows nudge 1px (Shift 10px); Delete emits `preview:delete`.
- Resize works for axis-aligned items (rotation a multiple of 90°, flips kept): the drag sets the scale/bounds
  magnitude in item space (axes swap at 90°/270°), then `force_update_transform` and the position is shifted so the
  box's top-left lands on the dragged rect. Items at other angles are move-only (use Edit Transform).
- Limitation: HTML cannot overlay the preview; context menus/modals hide the displays while open.

## Projector
- `src-tauri/src/projector.rs`, `src/ProjectorApp.tsx`, "⧉ Projector" select in the Preview header (`LayoutView.tsx`).
- Window label `projector` (one at a time; reopening replaces it), program display id `projector` in `Studio.previews`,
  windowed or fullscreen on a chosen monitor. Resize -> `set_bounds`; CloseRequested/Destroyed -> display dropped
  while the parent still exists. The empty page underneath keeps focus: F11 toggles fullscreen, Esc leaves
  fullscreen or closes. Capability `default.json` includes the window + set/is-fullscreen permissions.
- Opened from an async command (creating windows in sync commands deadlocks on Windows).
