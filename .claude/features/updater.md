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

## Behavior
- Manifest URL: GitHub `releases/latest/download/latest.json`; `STREAMTUBAH_UPDATE_URL` overrides it (tests or a
  self-hosted mirror). Downloads capped at 200 MB.
- Sizes (0.1.0): portable folder 334 MB, full zip 149 MB, app zip 3.4 MB.

## Verified 2026-11-14 (portable 0.1.0 install + local server serving 0.1.1)
Update detected; forged signature, wrong checksum and OBS-runtime change all refused with the exe untouched; signed
update installed (exe hash = new build, old exe kept until restart, running app still 0.1.0); restart → old instance
exits, new one reports 0.1.1 and up to date, old exe deleted. Found during the test: the app crashed on exit
(0xC0000005) when closed while Auto-Optimize was encoding; fixed (see auto-optimize.md / studio-core.md exit sequence).
