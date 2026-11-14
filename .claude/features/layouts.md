# Layouts (Unity-style), modes & Studio Mode

## Files
- `src/layout/model.ts` — preset = free split tree (`split{dir,sizes,children}` / `leaf{widget}`), mode, chatOpen;
  built-ins (Editor, Sidebar left/right, Column, Live, Live compact, Studio, Studio stacked); localStorage
  `streamtubah.layouts.v2`; ops: `dock` (swap / split by identity), `closePanel`, `addPanel`, `setSizes`,
  `savePresetAs`, `duplicatePreset`, `deletePreset`, `resetPreset`, `isModified`
- `src/layout/dock.tsx` — pointer-based docking (HTML5 DnD is intercepted by Tauri's file drop on Windows):
  `useDockHandle` (panel header = drag handle + ✕ close), `DockLeaf` (drop zones: center = swap, 25% edges = split),
  `DragGhost`; displays are hidden while dragging
- `src/layout/LayoutView.tsx` — renders the tree with react-resizable-panels v4; remounts on `shape()` change
- `src/layout/LayoutDrawer.tsx` — gallery only (pick / save current / duplicate / rename / delete / reset);
  thumbnails generated from the tree (`cells()` absolute rects; small cells show only the icon)
- `src/App.tsx` — topbar mode switch, "＋ Panel" for closed panels, workspace, chat drawer
- `src/components/Panels.tsx` `Panel` — shared panel chrome; header wired to docking
- `src-tauri/src/preview.rs`, `src-tauri/src/studio/mod.rs` — displays `main`/`program`, Studio Mode

## Behaviour
- Users build layouts in the workspace (drag headers, resize borders), then save them in the gallery.
- Each widget appears at most once per layout ("main" display must stay unique).
- Modes are a preset property; Studio presets enable Studio Mode (main = editable preview scene, program = on air).
- Native displays cannot be covered by HTML: `holdDisplaysHidden()` while the gallery is open (released on the
  closing `transitionend`), while dragging a panel, and while modals are open.
- Animations: `--ease: cubic-bezier(0.65, 0, 0.35, 1)`, `--slide: 460ms`.
- Chat drawer tab is glued to the chat panel (shared background, no gap).
