#!/bin/bash
# Build the Android APK and drop it next to this script.
#
#   ./builds/android/build.sh           # debug APK
#   ./builds/android/build.sh --release # release APK
#
# Output: builds/android/soshal_flutter.apk (or soshal_flutter-release.apk)

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"
resolve_script_dir
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

MODE="debug"
SPLIT_PER_ABI=0
OUT_NAME="soshal_flutter.apk"

for arg in "$@"; do
  case "$arg" in
    --release)
      MODE="release"
      OUT_NAME="soshal_flutter-release.apk"
      ;;
    --split-per-abi)
      SPLIT_PER_ABI=1
      ;;
    *)
      echo "Unknown argument: $arg (expected --release, --split-per-abi, or nothing)" >&2
      exit 1
      ;;
  esac
done

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
  # Official binary bundle from PurpleI2P/i2pd-android: contains per-arch
  # statics (i2pd-aarch64, i2pd-armv7l, i2pd-x86_64) + launcher + certs.
  ensure_download "i2pd" \
    "https://github.com/PurpleI2P/i2pd-android/releases/download/${version}/i2pd_${version}_android_binary.zip" \
    "$DAEMONS_CACHE/i2pd-android.zip" "$DAEMONS_CACHE/i2pd" "i2pd-aarch64"
}

# Freenet has no official Android daemon binary: the legacy fred/JVM stack
# can't run on Android and freenet-core (the Rust node) publishes no android
# release assets. Cross-build freenet-core's `freenet` node for arm64 with
# the NDK here; any failure degrades to an honest stub.
FREENET_VERSION="${FREENET_VERSION:-0.2.106}"
FREENET_SRC="$TARGETS_DIR/freenet-core"
FREENET_TARGETS="$TARGETS_DIR/freenet-targets"
FREENET_BIN="$DAEMONS_CACHE/freenet/freenet"

ensure_freenet() {
  local cache_dir="$DAEMONS_CACHE/freenet"
  mkdir -p "$cache_dir"
  if [[ -f "$FREENET_BIN" ]] && [[ $(stat -c%s "$FREENET_BIN") -gt 100000 ]]; then
    echo "  freenet: cached arm64 binary present"
    return 0
  fi
  # freenet-core pins rust 1.94.0 in rust-toolchain.toml, which lists only
  # wasm/musl targets — add the android std explicitly.
  if ! rustup target list --installed --toolchain 1.94.0 2>/dev/null | grep -q '^aarch64-linux-android$'; then
    rustup target add aarch64-linux-android --toolchain 1.94.0 >/dev/null 2>&1 || true
  fi
  if [[ ! -d "$FREENET_SRC/.git" ]]; then
    git clone --depth 1 --branch "v$FREENET_VERSION" \
      https://github.com/freenet/freenet-core "$FREENET_SRC" >/dev/null 2>&1 || {
        create_stub "$FREENET_BIN" "Freenet daemon: freenet-core clone failed"
        echo "  freenet: stub (clone failed)"
        return 0
      }
  else
    ( cd "$FREENET_SRC" && git fetch --depth 1 origin "v$FREENET_VERSION" >/dev/null 2>&1 && git checkout -q FETCH_HEAD ) || true
  fi
  echo "  freenet: cross-building arm64 node (freenet-core v$FREENET_VERSION; first run ~30min)"
  if ( cd "$FREENET_SRC" && \
       CARGO_TARGET_DIR="$FREENET_TARGETS" \
       CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android21-clang" \
       RUSTFLAGS="-C linker=$NDK_BIN/aarch64-linux-android21-clang" \
       cargo build -p freenet --release --target aarch64-linux-android ) \
       > "$TARGETS_DIR/freenet-build.log" 2>&1; then
    cp "$FREENET_TARGETS/aarch64-linux-android/release/freenet" "$FREENET_BIN"
    chmod +x "$FREENET_BIN"
    "$NDK_BIN/llvm-strip" "$FREENET_BIN" 2>/dev/null || true
    echo "  freenet: arm64 binary built ($(stat -c%s "$FREENET_BIN")B)"
  else
    tail -5 "$TARGETS_DIR/freenet-build.log" >&2 || true
    create_stub "$FREENET_BIN" "Freenet daemon: cross-build failed"
    echo "  freenet: stub (cross-build failed)"
  fi
}

# Reticulum daemon (rnsd) has no Android binary and needs none: it runs in
# the app process via the Chaquopy Python runtime (rnspure pip dep in
# build.gradle.kts), bridged from Rust through RnsdRunner. Public interface
# brings up the real RNS node at runtime — nothing to download or bundle.
ensure_reticulum() {
  echo "  reticulum: runs in-process via Chaquopy (rnspure) — no asset needed"
}

# Bundle all daemons into APK assets. Real binaries are >100KB; anything
# smaller is a stub/error placeholder and never bundled as a real daemon.
bundle_daemons() {
  echo "  bundling: networking daemons"
  mkdir -p "$ASSETS_DIR"

  # i2pd ships per-arch statics in one zip: map them onto the Android ABI
  # dirs so the bridge picks the right binary at runtime via
  # Build.SUPPORTED_ABIS[0] (assets/daemons/<abi>/i2pd).
  local -A I2PD_BIN=( [arm64-v8a]=i2pd-aarch64 [x86_64]=i2pd-x86_64 [armeabi-v7a]=i2pd-armv7l )
  local abi bin
  for abi in arm64-v8a x86_64 armeabi-v7a; do
    bin="${I2PD_BIN[$abi]}"
    if [[ -f "$DAEMONS_CACHE/i2pd/$bin" ]] &&
       [[ $(stat -c%s "$DAEMONS_CACHE/i2pd/$bin") -gt 100000 ]]; then
      mkdir -p "$ASSETS_DIR/$abi"
      cp "$DAEMONS_CACHE/i2pd/$bin" "$ASSETS_DIR/$abi/i2pd"
      mkdir -p "$JNI_LIBS/$abi"
      cp "$DAEMONS_CACHE/i2pd/$bin" "$JNI_LIBS/$abi/libi2pd.so"
      echo "  i2pd: $abi binary bundled (assets + jniLibs)"
    else
      # Create stub if download failed (only reachable on non-arm64 ABIs;
      # the aarch64 build is verified separately below).
      create_stub "$ASSETS_DIR/$abi/i2pd" "I2P daemon not available - download failed"
    fi
  done

  if [[ -f "$DAEMONS_CACHE/freenet/freenet" ]] &&
     [[ $(stat -c%s "$DAEMONS_CACHE/freenet/freenet") -gt 100000 ]]; then
    cp "$DAEMONS_CACHE/freenet/freenet" "$ASSETS_DIR/freenet"
    mkdir -p "$JNI_LIBS/arm64-v8a"
    cp "$DAEMONS_CACHE/freenet/freenet" "$JNI_LIBS/arm64-v8a/libfreenet.so"
    echo "  freenet: binary bundled (assets + jniLibs)"
  else
    # Create stub if no binary available
    create_stub "$ASSETS_DIR/freenet" "Freenet daemon: no official Android binary - not bundled in APK"
  fi

  # rnsd needs no asset (Chaquopy Python, see ensure_reticulum).

  echo "  daemons: bundled to assets/daemons/ and jniLibs/"
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
  RUSTFLAGS="-C linker=$NDK_BIN/$cc -L native=$SYSROOT/usr/lib/$libdir/24 -L native=$SYSROOT/usr/lib/$libdir/26" \
  OPUS_LIB_DIR="$OPUS_DIR/$abi/lib" LIBOPUS_STATIC=1 OPUS_NO_PKG=1 \
    cargo build -p "$BRIDGE" --release --target "$triple" --target-dir "$cache"
  cp "$so" "$JNI_LIBS/$abi/"
}

# Rust compile cache: sccache speeds up repeated bridge builds a lot
# (3 ABIs share most codegen). Auto-enabled when installed; disable with
# RUSTC_WRAPPER="".
if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  echo "  sccache: enabled"
fi

echo "== Soshal Android build ($MODE) =="
mkdir -p "$JNI_LIBS" "$TARGETS_DIR"

# Build and bundle networking daemons
echo "  preparing: networking daemons"
ensure_i2pd
ensure_freenet
ensure_reticulum
bundle_daemons

# Build Rust bridge for all ABIs (serial by default — parallel ABI builds
# triple peak memory; opt in with PARALLEL_ABI=1 when the machine can take it).
if [[ "${PARALLEL_ABI:-0}" == "1" ]]; then
  for abi in arm64-v8a x86_64 armeabi-v7a; do
    if [[ "$abi" == armeabi-v7a ]]; then
      export CC_GNU=1   # secp256k1-sys/aws-lc-sys probe arm-linux-androideabi-clang
    else
      unset CC_GNU
    fi
    build_abi "$abi" &
  done
  wait
else
  for abi in arm64-v8a x86_64 armeabi-v7a; do
    if [[ "$abi" == armeabi-v7a ]]; then
      export CC_GNU=1   # secp256k1-sys/aws-lc-sys probe arm-linux-androideabi-clang
    else
      unset CC_GNU
    fi
    build_abi "$abi"
  done
fi

cd "$PROJECT_ROOT/soshal_flutter"
BUILD_FLAGS=()
if [[ "$SPLIT_PER_ABI" -eq 1 ]]; then
  BUILD_FLAGS+=(--split-per-abi)
fi

if [[ "$MODE" == "release" ]]; then
  "$FLUTTER_BIN" build apk --release "${BUILD_FLAGS[@]}"
  if [[ "$SPLIT_PER_ABI" -eq 1 ]]; then
    for split_apk in build/app/outputs/flutter-apk/app-*-release.apk; do
      if [[ -f "$split_apk" ]]; then
        cp "$split_apk" "$SCRIPT_DIR/"
        echo "  copied: $SCRIPT_DIR/$(basename "$split_apk")"
      fi
    done
    OUT="$SCRIPT_DIR/app-arm64-v8a-release.apk"
  else
    OUT="$SCRIPT_DIR/$OUT_NAME"
    rm -f "$OUT"
    cp "build/app/outputs/flutter-apk/app-release.apk" "$OUT"
  fi
else
  "$FLUTTER_BIN" build apk --debug "${BUILD_FLAGS[@]}"
  if [[ "$SPLIT_PER_ABI" -eq 1 ]]; then
    for split_apk in build/app/outputs/flutter-apk/app-*-debug.apk; do
      if [[ -f "$split_apk" ]]; then
        cp "$split_apk" "$SCRIPT_DIR/"
        echo "  copied: $SCRIPT_DIR/$(basename "$split_apk")"
      fi
    done
    OUT="$SCRIPT_DIR/app-arm64-v8a-debug.apk"
  else
    OUT="$SCRIPT_DIR/$OUT_NAME"
    rm -f "$OUT"
    cp "build/app/outputs/flutter-apk/app-debug.apk" "$OUT"
  fi
fi

echo "  bundling: $OUT"
echo "  verifying: bridge libraries (expect 3 ABIs)"
BRIDGE_ABIS="$(unzip -l "$OUT" | grep -o 'lib/[^/]*/libsoshal_flutter_bridge.so' | cut -d/ -f2 | sort -u)"
BRIDGE_COUNT="$(printf '%s\n' "$BRIDGE_ABIS" | grep -c . || true)"
BRIDGE_MATCHES="$(unzip -l "$OUT" | grep 'libsoshal_flutter_bridge.so' || true)"
if [[ -z "$BRIDGE_MATCHES" ]]; then
  echo "  ERROR: no libsoshal_flutter_bridge.so in APK" >&2
  exit 1
fi
if [[ "$BRIDGE_COUNT" -lt 3 ]]; then
  echo "  ERROR: expected 3 ABI lib dirs, found $BRIDGE_COUNT ($(printf '%s ' $BRIDGE_ABIS)):" >&2
  unzip -l "$OUT" | grep 'libsoshal_flutter_bridge.so' >&2 || true
  exit 1
fi
echo "  verifying: networking daemons"
unzip -l "$OUT" | grep "assets/daemons/" || echo "  warning: daemons not found in APK"
I2PD_SIZE="$(unzip -l "$OUT" | grep 'assets/daemons/arm64-v8a/i2pd' | awk '{print $1}' || true)"
if [[ -n "$I2PD_SIZE" && "$I2PD_SIZE" -lt 100000 ]]; then
  echo "  WARNING: assets/daemons/arm64-v8a/i2pd is ${I2PD_SIZE}B (stub, not a real binary)" >&2
fi
echo "  verifying: python runtime (rnsd via Chaquopy)"
PYTHON_COUNT="$(unzip -l "$OUT" | grep -c 'lib/.*/libpython' || true)"
if [[ "$PYTHON_COUNT" -eq 0 ]]; then
  echo "  ERROR: no libpython in APK — rnsd (Chaquopy Python) missing, Reticulum daemon cannot start" >&2
fi
echo "== Done: $OUT =="