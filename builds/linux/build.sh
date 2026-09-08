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
# Pinned release (AppImage/appimagetool 1.9.1, built 2025-12-04) + sha256:
# the rolling `continuous` release is unpinned supply-chain risk.
APPIMAGE_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
APPIMAGE_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"

echo "== Soshal Linux dev build =="

# Build Reticulum daemon
echo "  rnsd: building daemon"
"$PROJECT_ROOT/scripts/build-reticulum.sh" || true  # non-fatal if build fails
RNSD_BIN="$PROJECT_ROOT/target/release/rnsd"

echo "  bridge: cargo build (debug profile)"
cargo build -p "$BRIDGE" --target "$HOST_TRIPLE" --target-dir "$CACHE_DIR"

if [[ ! -x "$APPIMAGETOOL" ]]; then
  echo "  tool: downloading appimagetool (pinned 1.9.1)"
  mkdir -p "$TOOLS_DIR"
  curl -L -f -o "$APPIMAGETOOL" "$APPIMAGE_URL"
  chmod +x "$APPIMAGETOOL"
fi
# Always verify the pinned checksum (detects tampered cache or bad download).
if ! echo "$APPIMAGE_SHA256  $APPIMAGETOOL" | sha256sum -c - >/dev/null 2>&1; then
  echo "  tool: checksum mismatch, re-downloading"
  rm -f "$APPIMAGETOOL"
  curl -L -f -o "$APPIMAGETOOL" "$APPIMAGE_URL"
  chmod +x "$APPIMAGETOOL"
  echo "$APPIMAGE_SHA256  $APPIMAGETOOL" | sha256sum -c - || {
    echo "FATAL: appimagetool checksum verification failed" >&2
    exit 1
  }
fi

cd "$PROJECT_ROOT/soshal_flutter"
echo "  flutter: build linux --debug"
"$FLUTTER_BIN" build linux --debug

# Stage the AppDir. The Flutter binary expects lib*/data next to itself
# ($ORIGIN/lib rpath), so the whole bundle layout is preserved under
# usr/bin/<app>/.
BUNDLE="build/linux/x64/debug/bundle"
STAGE="$SOSHAL_TARGETS_DIR/appimage-stage"
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

# Bundle rnsd daemon if available. Real binaries are >100KB; anything smaller
# is a stub/error placeholder and is never bundled as a real daemon.
if [[ -x "$RNSD_BIN" ]] && [[ $(stat -c%s "$RNSD_BIN") -gt 100000 ]]; then
  mkdir -p "$STAGE/usr/bin/daemons"
  cp "$RNSD_BIN" "$STAGE/usr/bin/daemons/rnsd"
  echo "  rnsd: bundled to usr/bin/daemons/ ($(stat -c%s "$RNSD_BIN")B)"
elif [[ -x "$RNSD_BIN" ]]; then
  echo "  WARNING: rnsd is $(stat -c%s "$RNSD_BIN")B (stub, not a real binary) — not bundled" >&2
fi

# Bundle i2pd + freenet from the host if real binaries are available (same
# >100KB stub guard as rnsd). usr/bin/daemons/ is also the runtime daemons
# dir: daemon.rs falls back to the AppImage's exe-sibling usr/bin/daemons.
I2PD_BIN="$(command -v i2pd || true)"
FREENET_BIN="$(command -v freenet || true)"
for daemon in "i2pd:$I2PD_BIN" "freenet:$FREENET_BIN"; do
  name="${daemon%%:*}"
  bin="${daemon#*:}"
  if [[ -x "$bin" ]] && [[ -f "$bin" ]] && [[ $(stat -c%s "$bin") -gt 100000 ]]; then
    mkdir -p "$STAGE/usr/bin/daemons"
    cp "$bin" "$STAGE/usr/bin/daemons/$name"
    echo "  $name: bundled to usr/bin/daemons/ ($(stat -c%s "$bin")B)"
  else
    echo "  WARNING: $name host binary not found — $name transport unavailable in AppImage" >&2
  fi
done

# Bundle libmpv.so.2 (media_kit video backend) if the host provides it.
# media_kit_libs_video does NOT bundle mpv on Linux — it dlopens system
# libmpv.so.2 at runtime; without it every video surfaces the old
# "not supported" error. Soname-stable, co-located with the other .sos so
# the $ORIGIN/lib rpath resolves it.
if [[ -f /usr/lib/libmpv.so.2 ]] || [[ -f /usr/lib/x86_64-linux-gnu/libmpv.so.2 ]]; then
  MPV_LIB=$( [[ -f /usr/lib/libmpv.so.2 ]] && echo /usr/lib/libmpv.so.2 \
          || echo /usr/lib/x86_64-linux-gnu/libmpv.so.2 )
  mkdir -p "$STAGE/usr/bin/soshal_flutter/lib"
  cp "$MPV_LIB" "$STAGE/usr/bin/soshal_flutter/lib/libmpv.so.2"
  echo "  mpv: bundled libmpv.so.2 for media_kit video"
else
  echo "  WARNING: system libmpv.so.2 not found — Linux video playback needs mpv" >&2
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