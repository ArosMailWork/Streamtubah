# Phase 5 — Sources & mixer parity with Streamlabs, Auto-Optimize

Status: 5.1-5.7 done · 2026-11-14 (5.7 implemented in phase 6.2) · Runs before multistream (phase 4.5+), at user request.

## Decisions (user)
- All four groups: add-source + list management, transforms, advanced mixer, multiple devices + multi-track.
- "Notes when adding" = built-in description per source type (what it is for, tips, pitfalls), like Streamlabs.
- Auto-Optimize wizard: everything measured on the streamer's machine.
- UI language stays English.
- App stays personal/local: no server, no account.

## Assumptions (stated to user)
- Upload speed is measured against Cloudflare's public speed test endpoint (no account / stream key needed).
- Stream-only / recording-only sources need a second render of the scene (extra GPU); off by default, built last.

## Work items
| # | Item | Verify |
|---|------|--------|
| 5.1 | Add Source dialog: categories (Capture / Media / Audio / Web & Text / Other), search, description + tips panel per type, "add existing" | every featured type has text; search filters |
| 5.2 | Source list: drag-and-drop reorder, groups (folders), context menu (properties, filters, rename, duplicate, copy/paste, copy/paste filters, transform, order, hide in mixer) | reorder persists; group collapse; paste in another scene |
| 5.3 | Transforms: rotate 90°, flip H/V, Edit Transform dialog (position, size, rotation, crop, bounds); show/hide transitions per item; projector window | values round-trip; rotated items resize in preview |
| 5.4 | Advanced audio: sync offset, balance, mono, tracks 1-6, volume % / dB input; hide in mixer; dB scale + peak hold + clip indicator; vertical mixer option | settings saved per source; meter shows clip |
| 5.5 | More global devices: Desktop Audio 2, Mic/Aux 2-4 (OBS channels 2, 4-6); multi-track recording (per-track AAC encoders); stream track choice | 2-track MKV has 2 audio streams |
| 5.6 | Auto-Optimize: detect GPU encoders, CPU, canvas; upload test; pick encoder/bitrate/resolution/FPS; preview and apply to current profile | applies sane values on this machine |
| 5.7 | Stream-only / record-only items (second render, off by default) | item visible in recording only |

## Order
5.1 → 5.2 → 5.3 → 5.4 → 5.5 → 5.6 → 5.7

## Risks
- Groups in libobs (`obs_scene_add_group`) change item ids/transform math in preview editing.
- Multi-track: record output needs one audio encoder per track; stream keeps one.
- 5.7 doubles scene rendering when enabled.

## Results
- 5.1 Add Source dialog with categories/search/descriptions — screenshot verified.
- 5.2 Folders, drag-and-drop ordering, paste reference/duplicate, copy filters — backend verified through temporary
  API hooks (removed); UI drag-and-drop not clicked through (no mouse automation on the user's desktop).
- 5.3 Rotate/flip keep the top-left corner (hand-checked numbers), Edit Transform round-trips, item show/hide
  transitions set/clear; preview resize now supports 90° steps and flips. Projector: built, not opened during tests.
- 5.4 Advanced audio values persist across restart; meter/vertical mixer/hidden badge screenshot verified.
- 5.5 Extra devices + multi-track: 2-track MKV verified with PyAV.
- 5.6 Auto-Optimize verified on the dev PC.
- 5.7 deferred (user decision): needs a second video mix + second encoder for recording (GPU cost).
