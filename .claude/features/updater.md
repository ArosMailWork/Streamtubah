# Portable packaging & self-update

## Files
- `scripts/package.sh` — release build → `release/<v>/`: `Streamtubah-<v>-win64.zip` (portable folder, CEF locales
  trimmed to en-US + vi, LICENSE + THIRD-PARTY with OBS source link), `Streamtubah-<v>-app.zip` (exe only),
  `latest.json` (version, notes, obs_runtime, app {url, sha256, signature, size}, full {url, sha256, size}).
  Refuses mismatched versions (Cargo.toml / tauri.conf.json / package.json) or a missing OBS runtime.
- `src-tauri/examples/release_tool.rs` — keygen / pubkey / sign (Ed25519; key outside the repo in
  `%USERPROFILE%\.streamtubah\update-signing.key`).
- `src-tauri/src/updater.rs` — check, install (sha256 + signature, extract exe, rename running exe to
  `streamtubah.old.exe`, rollback on failure), cleanup at boot, relaunch with `--after-update <pid>` (new instance
  waits up to 15 s for the old one), `OBS_RUNTIME` must match `scripts/setup-obs.sh`.
- Commands `update_check` / `update_install` (async), `update_restart` (refused while any output is active).
- UI `src/components/Updates.tsx`: startup check (settings `updates.auto_check`, 4 s after start, silent on errors),
  top-bar "⬆ Update x" button, dialog (notes, install, restart, full-package link), Settings → About & Updates.

## Browser component (CEF) on demand
- `src-tauri/src/cef.rs`, `src/components/CefDialog.tsx` (dialog opened by `act()` on errors starting with
  `BROWSER_COMPONENT_MISSING`, plus "Browser component" row in Settings → About), commands `cef_status`,
  `cef_install(path?)` (async, spawn_blocking), event `cef:status`.
- `package.sh` moves obs-browser + CEF (`obs-browser.dll`, `obs-browser-page.exe`, `libcef.dll`, `chrome_elf.dll`,
  `*.pak`, `icudtl.dat`, `v8_context_snapshot.bin`, `libEGL`/`libGLESv2`, `locales/`, `data/obs-plugins/obs-browser`)
  out of the portable folder into `Streamtubah-cef-<OBS>-win64.zip` (layout `obs-browser/{bin/64bit,data}`) and
  writes `cef-<OBS>.json` {obs_runtime, url, sha256, signature, size}. Published once per OBS runtime on the GitHub
  release `runtime-cef-<OBS>` (not "latest").
- App: manifest from `https://github.com/ArosMailWork/Streamtubah/releases/download/runtime-cef-<OBS>/cef-<OBS>.json`
  (`STREAMTUBAH_CEF_URL` overrides the base; "Install from file…" uses the `.json` next to a local zip, else the
  online one); download ≤ 400 MB with progress; `updater::verify` (sha256 + Ed25519, same key as updates);
  zip entries must be under `obs-browser/` (`enclosed_name`); extracted to `cef-<OBS>.tmp`, checked for
  obs-browser.dll + libcef.dll, `.complete` marker, renamed to `<app data>/components/cef-<OBS>/`. Restart (same
  `update_restart`) loads it: `cef::plugin_root` adds that folder as an OBS plugin root unless the exe folder still
  has obs-browser (dev builds); components of other OBS versions are deleted at boot.
- `cef::require` refuses `browser_source` in `source_create` / `input_create` while not loaded (libobs would
  otherwise create a dead placeholder). Add Source lists "Browser (download needed)" when the type is missing.
  Saved Browser Sources survive (libobs keeps unknown sources) and work after the install.
- Sizes 0.1.1: portable folder 98 MB / zip 40 MB; CEF zip 110 MB, 237 MB installed.
- Verified 2026-11-16 with the release exe + local server: type missing → MISSING error for Browser Source and Alert Box;
  tampered zip rejected (checksum) with nothing installed; genuine pack installed in ~2 s, "Restart now" relaunched,
  obs-browser 2.26.8 / CEF 127 loaded from components, `obs-browser-page.exe` ran from there, a test donation alert
  rendered in a recording.

## Behavior
- Manifest URL: GitHub `releases/latest/download/latest.json`; `STREAMTUBAH_UPDATE_URL` overrides it (tests or a
  self-hosted mirror). Downloads capped at 200 MB.
- Sizes (0.1.0): portable folder 334 MB, full zip 149 MB, app zip 3.4 MB. 0.1.1 (CEF split off): 98 MB / 40 MB / 3.7 MB.

## Verified 2026-11-14 (portable 0.1.0 install + local server serving 0.1.1)
Update detected; forged signature, wrong checksum and OBS-runtime change all refused with the exe untouched; signed
update installed (exe hash = new build, old exe kept until restart, running app still 0.1.0); restart → old instance
exits, new one reports 0.1.1 and up to date, old exe deleted. Found during the test: the app crashed on exit
(0xC0000005) when closed while Auto-Optimize was encoding; fixed (see auto-optimize.md / studio-core.md exit sequence).
