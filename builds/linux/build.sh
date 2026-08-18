#!/bin/bash
# Build the Linux desktop app in dev (debug) mode and package it as an
# AppImage next to this script.
#
#   ./builds/linux/build.sh
#
# Everything runs on the terminal with no output redirection, so compile
# errors surface immediately. Output: builds/linux/soshal_flutter-linux-x64.AppImage

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"

resolve_script_dir
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

resolve_flutter_bin

CACHE_DIR="$SOSHAL_TARGETS_DIR/so-linux"
TOOLS_DIR="$SOSHAL_TARGETS_DIR/tools"
BRIDGE="soshal-flutter-bridge"
HOST_TRIPLE="${HOST_TRIPLE:-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' || echo x86_64-unknown-linux-gnu)}"

# Bundled .so location; linux/CMakeLists.txt installs it into the bundle's
# lib/ dir (rpath $ORIGIN/lib is already set there).
OUT_SO="$CACHE_DIR/$HOST_TRIPLE/debug/libsoshal_flutter_bridge.so"
export SOSHAL_BRIDGE_SO="$OUT_SO"

APPIMAGETOOL="$TOOLS_DIR/appimagetool-x86_64.AppImage"
APPIMAGE_URL="https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"

echo "== Soshal Linux dev build =="

# Build Reticulum daemon
echo "  rnsd: building daemon"
"$PROJECT_ROOT/scripts/build-reticulum.sh" || true  # non-fatal if build fails
RNSD_BIN="$PROJECT_ROOT/target/release/rnsd"

echo "  bridge: cargo build (debug profile)"
cargo build -p "$BRIDGE" --target "$HOST_TRIPLE" --target-dir "$CACHE_DIR"

if [[ ! -x "$APPIMAGETOOL" ]]; then
  echo "  tool: downloading appimagetool"
  mkdir -p "$TOOLS_DIR"
  curl -L -f -o "$APPIMAGETOOL" "$APPIMAGE_URL"
  chmod +x "$APPIMAGETOOL"
fi

cd "$PROJECT_ROOT/soshal_flutter"
echo "  flutter: build linux --debug"
"$FLUTTER_BIN" build linux --debug

# Stage the AppDir. The Flutter binary expects lib*/data next to itself
# ($ORIGIN/lib rpath), so the whole bundle layout is preserved under
# usr/bin/<app>/.
BUNDLE="build/linux/x64/debug/bundle"
STAGE="/tmp/opencode/appimage-debug"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -r "$BUNDLE" "$STAGE/app"
mv "$STAGE/app/soshal_flutter" "$STAGE/app/soshal_flutter.bin"

cat > "$STAGE/AppRun" <<'EOF'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/soshal_flutter/soshal_flutter.bin" "$@"
EOF
chmod +x "$STAGE/AppRun"

INSTALL_BIN="$STAGE/usr/bin"
mkdir -p "$INSTALL_BIN"
mv "$STAGE/app" "$INSTALL_BIN/soshal_flutter"

# Bundle rnsd daemon if available
if [[ -x "$RNSD_BIN" ]]; then
  mkdir -p "$STAGE/usr/bin/daemons"
  cp "$RNSD_BIN" "$STAGE/usr/bin/daemons/rnsd"
  echo "  rnsd: bundled to usr/bin/daemons/"
fi

cat > "$STAGE/soshal_flutter.desktop" <<EOF
[Desktop Entry]
Name=Soshal
Comment=Native social media app
Exec=soshal_flutter
Icon=soshal_flutter
Type=Application
Categories=Network;Chat;
EOF
if command -v magick >/dev/null 2>&1; then
  magick -size 256x256 xc:none \
    -fill "#1a73e8" -draw "roundrectangle 16,16 240,240 48,48" \
    -fill white -gravity center -pointsize 120 -annotate 0 "S" \
    "$STAGE/soshal_flutter.png" 2>/dev/null || true
fi

OUT="$SCRIPT_DIR/soshal_flutter-linux-x64.AppImage"
rm -f "$OUT"
"$APPIMAGETOOL" --appimage-extract-and-run "$STAGE" "$OUT" >/dev/null 2>&1
if [[ ! -f "$OUT" ]]; then
  echo "  ERROR: appimagetool failed to produce $OUT" >&2
  exit 1
fi

echo "  bundling: $OUT"
VERIFY_DIR="$(mktemp -d)"
( cd "$VERIFY_DIR" \
    && APPIMAGE_EXTRACT_AND_RUN=1 "$OUT" --appimage-extract 'usr/bin/soshal_flutter/lib/libsoshal_flutter_bridge.so' >/dev/null 2>&1 )
if [[ -f "$VERIFY_DIR/squashfs-root/usr/bin/soshal_flutter/lib/libsoshal_flutter_bridge.so" ]]; then
  echo "  bridge .so verified inside AppImage"
else
  echo "  WARNING: bridge .so not found in AppImage" >&2
fi
rm -rf "$VERIFY_DIR"
echo "== Done: $OUT =="
echo "  Run it: $OUT (may need --appimage-extract-and-run if FUSE is unavailable)"