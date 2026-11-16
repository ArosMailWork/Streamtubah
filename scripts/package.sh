#!/usr/bin/env bash
# Builds the portable release and the self-update assets into release/<version>/:
#   Streamtubah-<v>-win64.zip   full portable folder (first install / OBS runtime changes)
#   Streamtubah-<v>-app.zip     streamtubah.exe only (what the in-app updater installs)
#   latest.json                 update manifest (version, sha256, Ed25519 signature)
#   Streamtubah-cef-<obs>-win64.zip + cef-<obs>.json
#                               browser component (CEF + obs-browser), downloaded by the app on first
#                               use from the GitHub release `runtime-cef-<obs>` (see src-tauri/src/cef.rs)
#
# Usage: scripts/package.sh ["release notes"]
# Needs: `bash scripts/setup-obs.sh release` done once, and the signing key
#        (cargo run --example release_tool -- keygen; see src-tauri/examples/release_tool.rs).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
REPO="ArosMailWork/Streamtubah"
NOTES="${1:-}"
WINTAR="/c/Windows/System32/tar.exe"   # bsdtar: writes .zip with -a

VERSION=$(grep -m1 '^version' src-tauri/Cargo.toml | cut -d'"' -f2)
CONF_VERSION=$(grep -m1 '"version"' src-tauri/tauri.conf.json | cut -d'"' -f4)
PKG_VERSION=$(grep -m1 '"version"' package.json | cut -d'"' -f4)
if [ "$VERSION" != "$CONF_VERSION" ] || [ "$VERSION" != "$PKG_VERSION" ]; then
  echo "Version mismatch: Cargo.toml=$VERSION tauri.conf.json=$CONF_VERSION package.json=$PKG_VERSION" >&2
  exit 1
fi
OBS=$(grep -o 'OBS_RUNTIME: &str = "[^"]*"' src-tauri/src/updater.rs | cut -d'"' -f2)
SRC="$ROOT/src-tauri/target/release"
[ -f "$SRC/obs.dll" ] || { echo "OBS runtime missing: run 'bash scripts/setup-obs.sh release' first" >&2; exit 1; }

OUT="$ROOT/release/$VERSION"
STAGE="$OUT/Streamtubah"
rm -rf "$OUT"
mkdir -p "$STAGE"

echo "== Building $VERSION"
npx tauri build --no-bundle

echo "== Staging portable folder"
cp "$SRC"/*.exe "$SRC"/*.dll "$STAGE/"
cp -r "$SRC/data" "$SRC/obs-plugins" "$STAGE/"
# CEF ships ~40 MB of UI translations; keep English and Vietnamese.
find "$STAGE/obs-plugins/64bit/locales" -type f ! -name 'en-US.pak' ! -name 'vi.pak' -delete
find "$STAGE" -name '*.pdb' -delete
rm -f "$STAGE/streamtubah.old.exe" "$STAGE/streamtubah.new.exe"
cp LICENSE "$STAGE/LICENSE.txt"
cat > "$STAGE/THIRD-PARTY.txt" <<EOF
Streamtubah $VERSION is free software under the GNU GPL v3 (LICENSE.txt).
Source code: https://github.com/$REPO

It includes the OBS Studio $OBS runtime (libobs, plug-ins, CEF), also under the GPL v2+.
OBS Studio source code: https://github.com/obsproject/obs-studio/tree/$OBS
EOF

echo "== Splitting off the browser component (CEF)"
CEF_STAGE="$OUT/cef/obs-browser"
mkdir -p "$CEF_STAGE/bin/64bit" "$CEF_STAGE/data"
P64="$STAGE/obs-plugins/64bit"
for f in obs-browser.dll obs-browser-page.exe libcef.dll chrome_elf.dll icudtl.dat v8_context_snapshot.bin snapshot_blob.bin          libEGL.dll libGLESv2.dll vk_swiftshader.dll vk_swiftshader_icd.json vulkan-1.dll; do
  [ -e "$P64/$f" ] && mv "$P64/$f" "$CEF_STAGE/bin/64bit/"
done
mv "$P64"/*.pak "$CEF_STAGE/bin/64bit/"
mv "$P64/locales" "$CEF_STAGE/bin/64bit/"
mv "$STAGE/data/obs-plugins/obs-browser/"* "$CEF_STAGE/data/"
rmdir "$STAGE/data/obs-plugins/obs-browser"
[ -f "$CEF_STAGE/bin/64bit/libcef.dll" ] || { echo "libcef.dll not found" >&2; exit 1; }
CEF="Streamtubah-cef-$OBS-win64.zip"
(cd "$OUT/cef" && "$WINTAR" -a -cf "../$CEF" obs-browser)

FULL="Streamtubah-$VERSION-win64.zip"
APP="Streamtubah-$VERSION-app.zip"
echo "== Zipping"
(cd "$OUT" && "$WINTAR" -a -cf "$FULL" Streamtubah)
(cd "$STAGE" && "$WINTAR" -a -cf "../$APP" streamtubah.exe)

echo "== Signing"
SIG=$(cd src-tauri && cargo run -q --example release_tool -- sign "$OUT/$APP")
CEF_SIG=$(cd src-tauri && cargo run -q --example release_tool -- sign "$OUT/$CEF")
CEF_SHA=$(sha256sum "$OUT/$CEF" | cut -d' ' -f1)
python - "$OUT/cef-$OBS.json" <<EOF
import json, sys
json.dump({
    "obs_runtime": "$OBS",
    "url": "https://github.com/$REPO/releases/download/runtime-cef-$OBS/$CEF",
    "sha256": "$CEF_SHA", "signature": "$CEF_SIG", "size": $(stat -c%s "$OUT/$CEF"),
}, open(sys.argv[1], "w"), indent=2)
EOF
SHA_APP=$(sha256sum "$OUT/$APP" | cut -d' ' -f1)
SHA_FULL=$(sha256sum "$OUT/$FULL" | cut -d' ' -f1)
BASE="https://github.com/$REPO/releases/download/v$VERSION"
python - "$OUT/latest.json" <<EOF
import json, sys
json.dump({
    "version": "$VERSION",
    "notes": """$NOTES""",
    "obs_runtime": "$OBS",
    "app": {"url": "$BASE/$APP", "sha256": "$SHA_APP", "signature": "$SIG", "size": $(stat -c%s "$OUT/$APP")},
    "full": {"url": "$BASE/$FULL", "sha256": "$SHA_FULL", "size": $(stat -c%s "$OUT/$FULL")},
}, open(sys.argv[1], "w"), indent=2)
EOF

echo
du -sh "$STAGE" "$OUT/$FULL" "$OUT/$APP" "$OUT/cef" "$OUT/$CEF"
echo
echo "Browser component (once per OBS runtime; skip if runtime-cef-$OBS already exists):"
echo "  gh release create runtime-cef-$OBS \"release/$VERSION/$CEF\" \"release/$VERSION/cef-$OBS.json\" --title \"Browser component for OBS $OBS\" --notes \"Downloaded by Streamtubah on first use of a Browser Source.\" --latest=false"
echo "Publish (installed copies look at the 'latest' release):"
echo "  gh release create v$VERSION \"release/$VERSION/$FULL\" \"release/$VERSION/$APP\" \"release/$VERSION/latest.json\" --title \"Streamtubah $VERSION\" --notes \"$NOTES\""
