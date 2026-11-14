#!/usr/bin/env bash
# Places the official OBS Studio runtime next to the Tauri executable.
# Usage: scripts/setup-obs.sh [debug|release]
set -euo pipefail
PROFILE="${1:-debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/target/$PROFILE"
VERSION="32.1.0"
CACHE="$ROOT/obs-build"

mkdir -p "$OUT"
(cd "$ROOT/src-tauri" && "C:\Users\DA\AppData\Roaming\kazuo-ai\ai-chat\bin\rtk.exe" cargo obs-build build --out-dir "./target/$PROFILE" --browser --remove-pdbs)

# cargo-obs-build strips Qt and obs-frontend-api, but obs-browser.dll imports them.
ZIP="$CACHE/OBS-Studio-$VERSION-Windows-x64.zip"
EXTRACT="$CACHE/official-$VERSION"
if [ ! -d "$EXTRACT" ]; then
  [ -f "$ZIP" ] || curl -fL -o "$ZIP" "https://github.com/obsproject/obs-studio/releases/download/$VERSION/OBS-Studio-$VERSION-Windows-x64.zip"
  mkdir -p "$EXTRACT" && unzip -q "$ZIP" -d "$EXTRACT"
fi
BIN="$(dirname "$(find "$EXTRACT" -name obs.dll -path '*64bit*' | head -1)")"
for f in obs-frontend-api.dll Qt6Core.dll Qt6Gui.dll Qt6Widgets.dll Qt6Svg.dll Qt6Network.dll Qt6Xml.dll; do
  [ -f "$BIN/$f" ] && cp -u "$BIN/$f" "$OUT/"
done

# Modules cargo-obs-build drops that this app supports:
# obs-vst (VST 2 filter slot), obs-webrtc (WHIP streaming), decklink (capture cards).
PLUG="$(dirname "$BIN")/../obs-plugins/64bit"
DATA="$(dirname "$BIN")/../data/obs-plugins"
for m in obs-vst obs-webrtc decklink; do
  cp -u "$PLUG/$m.dll" "$OUT/obs-plugins/64bit/"
  [ -d "$DATA/$m" ] && mkdir -p "$OUT/data/obs-plugins/$m" && cp -ru "$DATA/$m/." "$OUT/data/obs-plugins/$m/"
done
echo "OBS $VERSION runtime ready in $OUT"
