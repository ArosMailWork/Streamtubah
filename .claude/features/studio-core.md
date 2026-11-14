# Studio core (libobs integration)

## Purpose
Boot libobs, own every libobs object safely, and expose scenes/sources/items/filters/
transitions/audio as Tauri commands.

## Files
- `src-tauri/src/obs/mod.rs` — RAII refs (`Source`, `Output`, `Encoder`, `Service`), `Matrix4`, extra `os_*` externs
- `src-tauri/src/obs/data.rs` — `obs_data` <-> JSON
- `src-tauri/src/obs/properties.rs` — `obs_properties` -> JSON form description
- `src-tauri/src/obs/runtime.rs` — startup, log handler (`%APPDATA%/com.streamtubah.app/obs.log`), video/audio reset, shutdown
- `src-tauri/src/obs/signals.rs` — calldata parser (inline accessors are not exported), proc calls
- `src-tauri/src/studio/mod.rs` — `Studio` model + persistence (`scenes.json`)
- `src-tauri/src/studio/mixer.rs` — volmeters -> atomics -> `obs:levels` (~30 Hz)
- `src-tauri/src/settings.rs` — `settings.json`
- `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs` — command surface, boot, timers

## Key behavior
- Output channels: 0 = transition (program), 1 = Desktop Audio, 2 = Desktop Audio 2, 3 = Mic/Aux, 4-6 = Mic/Aux 2-4 (extras in `EXTRA_CHANNELS`, saved as `channels.desktop2/mic2/mic3/mic4`; names reserved via `reserved_name`) — private sources managed by `AudioChannel` (`studio/channels.rs`), saved under `channels`; device ids come from `settings.audio` (see audio-ports.md).
- Scene switch goes through the transition (`obs_transition_start`, duration from settings).
- New sources: list properties with an invalid value auto-select the first option (`preselect_lists`), e.g. Display Capture picks a monitor.
- Global audio channels cannot be renamed (Settings > Audio finds them by name).
- Autosave: mutating commands set `dirty`; the 1 s stats thread saves `scenes.json`. Also saved on exit.
- Events: `studio:changed`, `preview:delete`, `obs:levels`, `obs:stats`, `obs:output`, `studio:scene`, `studio:scenes`, `studio:mute`, `studio:volume`.
- `input_create` / `input_remove` add or remove inputs in any scene (used by the control API).

## Scene collections & profiles
- `src-tauri/src/studio/collections.rs` (impl Studio). Files: `collections/<name>.json` (same format as the old
  `scenes.json`, which `lib.rs::migrate_collection` moves to `collections/<settings.collection>.json` on first boot),
  `profiles/<name>.json` = `settings::Profile { video, output, stream }`.
- `settings.collection` / `settings.profile` are owned by these commands; `apply_settings` keeps the current values
  so a stale Settings draft cannot move them. Names go through `settings::valid_name` (file-name safe, 1-64 chars).
- Switching collection: save current -> `clear_scene_data` (dec preview showing, clear displays/mixer/transition,
  `obs_source_remove` every scene + input, drop refs, then `obs_wait_for_destroy_queue` — libobs destroys on a
  background queue and names stay taken until it drains, otherwise reloaded sources get renamed "X 2") -> load or
  create default -> `studio:scenes` + `studio:changed`.
- Switching profile: write current profile file, `apply_settings` with the new video/output/stream, `reset_service`.
- Both refuse while any output (incl. virtual camera) is active.
- UI: `src/components/NamedSets.tsx` (picker + New/Duplicate/Rename/Delete) used in the Scenes panel (collections)
  and at the top of Settings → Stream/Output/Video (profiles).

## Source list (folders, ordering, copy/paste)
- `src-tauri/src/studio/items.rs`: `all_items` (top-level + folder children), `item_move(id, parent, index)` (index
  counted from the top, moves in/out of folders with `obs_sceneitem_group_add/remove_item`), `group_create`
  (`obs_scene_insert_group`, may be empty), `group_ungroup`, `item_paste(from_scene, from_id, duplicate)` (reference
  or `obs_source_duplicate`, copies transform/crop/visibility via `get/set_info2`), `filters_copy`
  (`obs_source_copy_filters`, appends). Folders cannot nest; item ids stay unique per scene.
- `Studio::item` / `item_select` / `item_remove_selected` search folders too. `ItemInfo` has `group` + `parent`;
  `state.items` lists top-most first with children right after their folder.
- Transforms: `item_rotate_flip` (rotate ±90/180, flip H/V) keeps the bounding box's top-left fixed like OBS;
  `item_transform_get/set` back the Edit Transform dialog (pos, rot, scale, alignment, bounds type/size/alignment,
  crop); `item_transitions` / `item_transition_set` = per-item show/hide transitions (private transition source,
  duration 50-20000 ms; saved by libobs with the scene).
- UI: `Panels.tsx` SourcesPanel (drag-and-drop with before/into hints, collapse, right-click `ContextMenu.tsx`,
  Ctrl+C / Ctrl+V / Ctrl+D, app-wide item and filter clipboards), `TransformDialogs.tsx`.
- Add Source dialog (`Dialogs.tsx` + `sourceCatalog.ts`): categories, search, description + tips per type keyed by
  type id without version suffix; unknown (plug-in) types fall under "Other".

## Threading
- Sync Tauri commands + preview window proc run on the main thread; `Studio` is behind a Mutex.
- Volmeter/signal callbacks never take the Studio lock (atomics / emit only).
