#!/bin/bash
# Build the Android APK and drop it next to this script.
#
#   ./builds/android/build.sh           # debug APK
#   ./builds/android/build.sh --release # release APK
#
# Output: builds/android/soshal_flutter.apk (or soshal_flutter-release.apk)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

source "$SCRIPT_DIR/../common.sh"

MODE="debug"
OUT_NAME="soshal_flutter.apk"
if [[ "${1:-}" == "--release" ]]; then
  MODE="release"
  OUT_NAME="soshal_flutter-release.apk"
elif [[ $# -gt 0 ]]; then
  echo "Unknown argument: $1 (expected --release or nothing)" >&2
  exit 1
fi

ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
NDK_VERSION="${NDK_VERSION:-27.1.12297006}"
resolve_flutter_bin

NDK_ROOT="$ANDROID_HOME/ndk/$NDK_VERSION"
NDK_BIN="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"
SYSROOT="$NDK_BIN/../sysroot"
JNI_LIBS="$PROJECT_ROOT/soshal_flutter/android/app/src/main/jniLibs"
ASSETS_DIR="$PROJECT_ROOT/soshal_flutter/android/app/src/main/assets/daemons"
TARGETS_DIR="$SOSHAL_TARGETS_DIR"
NDK_ALIAS_DIR="$TARGETS_DIR/ndk-bin"
DAEMONS_CACHE="$TARGETS_DIR/daemons"
BRIDGE="soshal-flutter-bridge"

if [[ ! -x "$NDK_BIN/aarch64-linux-android21-clang" ]]; then
  echo "NDK $NDK_VERSION not found at $NDK_ROOT" >&2
  echo "Set ANDROID_HOME and/or NDK_VERSION if your SDK lives elsewhere." >&2
  exit 1
fi

# NDK 27 ships only versioned wrappers (<triple>21-clang) that set --target
# but NO --sysroot, and build scripts (ring, aws-lc-sys, secp256k1-sys,
# libsql-ffi) probe bare <triple>-clang on PATH. Create patched aliases in a
# scratch dir: bare name + explicit --target + --sysroot. They must be
# *copied* scripts (NDK wrappers resolve dirname $0, so symlinks break them)
# with clang/clang++/ld.lld symlinked in alongside.
setup_ndk_aliases() {
  mkdir -p "$NDK_ALIAS_DIR"
  for spec in aarch64-linux-android-clang:aarch64-linux-android21 \
              x86_64-linux-android-clang:x86_64-linux-android21 \
              arm-linux-androideabi-clang:armv7a-linux-androideabi21; do
    local alias="${spec%%:*}" target="${spec##*:}"
    cat > "$NDK_ALIAS_DIR/$alias" <<EOF
#!/usr/bin/env bash
bin_dir=\`dirname "\$0"\`
if [ "\$1" != "-cc1" ]; then
    "\$bin_dir/clang" --target=$target --sysroot=$SYSROOT "\$@"
else
    "\$bin_dir/clang" "\$@"
fi
EOF
    chmod +x "$NDK_ALIAS_DIR/$alias"
  done
  ln -sf "$NDK_BIN/clang"   "$NDK_ALIAS_DIR/clang"
  ln -sf "$NDK_BIN/clang++" "$NDK_ALIAS_DIR/clang++"
  ln -sf "$NDK_BIN/ld.lld"  "$NDK_ALIAS_DIR/ld.lld"
  ln -sf "$NDK_BIN/llvm-ar" "$NDK_ALIAS_DIR/llvm-ar" 2>/dev/null || true
  export PATH="$NDK_ALIAS_DIR:$PATH"   # bare aliases win; versioned names still resolve
}
setup_ndk_aliases

declare -A TRIPLE=( [arm64-v8a]=aarch64-linux-android [x86_64]=x86_64-linux-android [armeabi-v7a]=armv7-linux-androideabi )
declare -A CLANG=(  [arm64-v8a]=aarch64-linux-android21-clang [x86_64]=x86_64-linux-android21-clang [armeabi-v7a]=armv7a-linux-androideabi21-clang )
declare -A LIBDIR=( [arm64-v8a]=aarch64-linux-android [x86_64]=x86_64-linux-android [armeabi-v7a]=arm-linux-androideabi )
declare -A OPUSHOST=( [arm64-v8a]=aarch64-linux-android [x86_64]=x86_64-linux-android [armeabi-v7a]=armv7a-linux-androideabi )

# ---------------------------------------------------------------------------
# Build and bundle networking daemons
# ---------------------------------------------------------------------------

# Download + extract + verify a daemon binary into the cache
ensure_download() {
  local name="$1" url="$2" cache_file="$3" extract_dir="$4" binary="$5"
  mkdir -p "$DAEMONS_CACHE" "$extract_dir"
  if [[ ! -f "$cache_file" ]]; then
    echo "  $name: downloading Android binary"
    curl -L -f -o "$cache_file" "$url" || echo "  $name: download failed, will use stub"
  fi
  if [[ -f "$cache_file" ]]; then
    unzip -q -o "$cache_file" -d "$extract_dir" 2>/dev/null || true
    if [[ -f "$extract_dir/$binary" ]]; then
      chmod +x "$extract_dir/$binary"
      echo "  $name: binary prepared"
    fi
  fi
}

# Single stub fallback: writes a failing placeholder when a download fails
create_stub() {
  local path="$1" msg="$2"
  cat > "$path" << EOF
#!/system/bin/sh
echo "$msg"
exit 1
EOF
  chmod +x "$path"
}

# Download and prepare I2P router (i2pd) for Android
ensure_i2pd() {
  local version="${I2PD_VERSION:-2.50.0}"
  ensure_download "i2pd" \
    "https://github.com/PurpleI2P/i2pd/releases/download/${version}/i2pd_${version}_android_arm64.zip" \
    "$DAEMONS_CACHE/i2pd-arm64.zip" "$DAEMONS_CACHE/i2pd" "i2pd"
}

# Download and prepare Freenet reference node for Android
ensure_freenet() {
  local version="${FREENET_VERSION:-0.4.0}"
  ensure_download "freenet" \
    "https://github.com/freenet/freenet-core/releases/download/${version}/freenet-node-android.zip" \
    "$DAEMONS_CACHE/freenet-android.zip" "$DAEMONS_CACHE/freenet" "freenet"
}

# Build Reticulum daemon for Android (use Python-for-Android approach)
ensure_reticulum() {
  local cache_dir="$DAEMONS_CACHE/reticulum"
  mkdir -p "$cache_dir"
  
  # For now, create a stub script since Reticulum requires Python runtime
  # In production, this would use Python-for-Android to package rnsd
  create_stub "$cache_dir/rnsd" "Reticulum daemon requires Python runtime - not bundled in APK"
  echo "  reticulum: stub prepared (requires Python runtime)"
}

# Bundle all daemons into APK assets
bundle_daemons() {
  echo "  bundling: networking daemons"
  mkdir -p "$ASSETS_DIR"
  
  # Copy daemons to assets
  if [[ -f "$DAEMONS_CACHE/i2pd/i2pd" ]]; then
    cp "$DAEMONS_CACHE/i2pd/i2pd" "$ASSETS_DIR/i2pd"
  else
    # Create stub if download failed
    create_stub "$ASSETS_DIR/i2pd" "I2P daemon not available - download failed"
  fi
  
  if [[ -f "$DAEMONS_CACHE/freenet/freenet" ]]; then
    cp "$DAEMONS_CACHE/freenet/freenet" "$ASSETS_DIR/freenet"
  else
    # Create stub if download failed
    create_stub "$ASSETS_DIR/freenet" "Freenet daemon not available - download failed"
  fi
  
  # Always use Reticulum stub (requires Python runtime)
  cp "$DAEMONS_CACHE/reticulum/rnsd" "$ASSETS_DIR/rnsd"
  
  echo "  daemons: bundled to assets/daemons/"
}

# audiopus_sys has no Android cross-build support of its own: its build.rs
# runs host `configure && make` unless OPUS_LIB_DIR points at a prebuilt
# libopus. Build a static per-ABI opus once with the NDK and cache it.
OPUS_SRC_VERSION="${OPUS_SRC_VERSION:-1.5.2}"
OPUS_SRC_URL="${OPUS_SRC_URL:-https://downloads.xiph.org/releases/opus/opus-$OPUS_SRC_VERSION.tar.gz}"
OPUS_DIR="${OPUS_DIR:-$TARGETS_DIR/opus}"   # disk-backed — /tmp is tmpfs

ensure_opus() {
  local abi="$1" host="$2" cc="$3"
  local prefix="$OPUS_DIR/$abi"
  if [[ -f "$prefix/lib/libopus.a" ]]; then
    return 0
  fi
  local src="$OPUS_DIR/src/opus-$OPUS_SRC_VERSION"
  if [[ ! -f "$src/configure" ]]; then
    mkdir -p "$OPUS_DIR/src"
    local tarball="$OPUS_DIR/src/opus-$OPUS_SRC_VERSION.tar.gz"
    if [[ ! -f "$tarball" ]]; then
      echo "  opus: fetching source"
      curl -fsSL "$OPUS_SRC_URL" -o "$tarball"
    fi
    tar -xzf "$tarball" -C "$OPUS_DIR/src"
  fi
  echo "  opus: building $abi (static)"
  mkdir -p "$prefix"
  (
    cd "$src"
    make distclean >/dev/null 2>&1 || true
    if [[ ! -x ./configure ]]; then
      ./autogen.sh >/dev/null
    fi
    ./configure --host="$host" CC="$cc" --disable-shared --enable-static \
      --disable-doc --disable-extra-programs --with-pic \
      --prefix="$prefix" CFLAGS="-O2 -fPIC" >/dev/null
    make -j"$(nproc)" >/dev/null
    make install >/dev/null
  )
}

build_abi() {
  local abi="$1" cc="${CLANG[$abi]}" triple="${TRIPLE[$abi]}" libdir="${LIBDIR[$abi]}"
  local cache="$TARGETS_DIR/so-$abi"
  local so="$cache/$triple/release/libsoshal_flutter_bridge.so"
  echo "  bridge: $abi"
  ensure_opus "$abi" "${OPUSHOST[$abi]}" "$NDK_BIN/$cc"
  # audiopus_sys' build.rs doesn't rerun on env change — force a fresh link
  # against the opus we just ensured (see OPUS_LIB_DIR below).
  find "$cache" -path "*audiopus*" -exec rm -rf {} + 2>/dev/null || true
  env "CC_${triple}_linux_android=$NDK_BIN/$cc" \
  ${CC_GNU:+CC_arm-linux-androideabi_linux_android=$NDK_BIN/$cc} \
  "CC_${triple}=$NDK_BIN/$cc" \
  RUSTFLAGS="-C linker=$NDK_BIN/$cc -L native=$SYSROOT/usr/lib/$libdir/24" \
  OPUS_LIB_DIR="$OPUS_DIR/$abi/lib" LIBOPUS_STATIC=1 OPUS_NO_PKG=1 \
    cargo build -p "$BRIDGE" --release --target "$triple" --target-dir "$cache"
  cp "$so" "$JNI_LIBS/$abi/"
}

echo "== Soshal Android build ($MODE) =="
mkdir -p "$JNI_LIBS" "$TARGETS_DIR"

# Build and bundle networking daemons
echo "  preparing: networking daemons"
ensure_i2pd
ensure_freenet
ensure_reticulum
bundle_daemons

# Build Rust bridge for all ABIs
for abi in arm64-v8a x86_64 armeabi-v7a; do
  if [[ "$abi" == armeabi-v7a ]]; then
    export CC_GNU=1   # secp256k1-sys/aws-lc-sys probe arm-linux-androideabi-clang
  else
    unset CC_GNU
  fi
  build_abi "$abi"
done

cd "$PROJECT_ROOT/soshal_flutter"
if [[ "$MODE" == "release" ]]; then
  "$FLUTTER_BIN" build apk --release --split-per-abi
  OUT="$SCRIPT_DIR/$OUT_NAME"
  rm -f "$OUT"
  cp build/app/outputs/flutter-apk/app-arm64-v8a-release.apk "$OUT" || cp build/app/outputs/flutter-apk/app-release.apk "$OUT"
else
  "$FLUTTER_BIN" build apk --debug
  OUT="$SCRIPT_DIR/$OUT_NAME"
  rm -f "$OUT"
  cp "build/app/outputs/flutter-apk/app-debug.apk" "$OUT"
fi

echo "  bundling: $OUT"
echo "  verifying: bridge libraries (expect 3 ABIs)"
BRIDGE_COUNT="$(unzip -l "$OUT" | grep -c 'lib/arm64-v8a/\|lib/x86_64/\|lib/armeabi-v7a/' || true)"
BRIDGE_MATCHES="$(unzip -l "$OUT" | grep 'libsoshal_flutter_bridge.so' || true)"
if [[ -z "$BRIDGE_MATCHES" ]]; then
  echo "  ERROR: no libsoshal_flutter_bridge.so in APK" >&2
  exit 1
fi
if [[ "$BRIDGE_COUNT" -lt 3 ]]; then
  echo "  WARNING: expected 3 ABI lib dirs, found $BRIDGE_COUNT:" >&2
  unzip -l "$OUT" | grep 'libsoshal_flutter_bridge.so' >&2 || true
fi
echo "  verifying: networking daemons"
unzip -l "$OUT" | grep "assets/daemons/" || echo "  warning: daemons not found in APK"
echo "== Done: $OUT =="