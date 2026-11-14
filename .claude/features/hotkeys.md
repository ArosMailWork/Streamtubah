# Global hotkeys

## Files
- `src-tauri/src/hotkeys.rs` — `ACTIONS`, `apply` (re-registers all), `action_for`, `dispatch`
- `src-tauri/src/lib.rs` — global-shortcut plugin handler: user action if bound, else the chat pop-out hotkey (Ctrl+Alt+C)
- `src-tauri/src/settings.rs` — `AppSettings.hotkeys: BTreeMap<action, accelerator>`
- `src/components/SettingsDialog.tsx` — Settings → Hotkeys (`HotkeyInput`)

## Behavior
- Actions: stream.toggle, record.toggle, record.pause, replay.toggle, replay.save, virtualcam.toggle,
  studio.transition, mute.desktop, mute.mic, `scene:<name>` (Studio Mode: goes to preview like a click).
- Accelerator format: modifiers `Ctrl/Alt/Shift/Super` + `KeyboardEvent.code` ("Ctrl+Shift+KeyK"), parsed by
  global-hotkey. The UI falls back to `e.key` when `e.code` is empty (injected input). Backspace/Delete/Esc clears.
- Registered with RegisterHotKey (system-wide, works in games); the combination is consumed, so other apps and the
  input field itself never see it. Registration failures/duplicates/the reserved Ctrl+Alt+C are returned by `apply`;
  `settings_set` saves first, then returns them as an error toast.
- Applied at boot and whenever `settings.hotkeys` changes. Default: none bound.
- `dispatch` runs on the main thread, locks Studio, then emits `studio:changed`; errors -> `app:error`.
- Scene hotkeys are keyed by scene name and are not per collection; renaming/removing a scene leaves an unused entry.
- Verified 2026-11-14: mute toggle and scene switch via injected keys. Note: on the dev machine another program
  swallows Ctrl+Shift+F9 system-wide (not an app bug).
