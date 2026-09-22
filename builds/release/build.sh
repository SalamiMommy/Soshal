#!/bin/bash
# Run every eligible Soshal build script on this host and collect the
# artifacts into builds/release/.
#
#   ./builds/release/build.sh [--release] [--android-split-per-abi]
#
# Host capability matrix:
#   Linux  -> Android APK(s) + Linux AppImage
#   Windows (Git Bash / MSYS2) -> Windows zip (runner bundle incl. bridge DLL)
# Each target build script is fail-fast (set -euo pipefail): the first
# failure aborts the whole release. The Windows target is only run on a
# Windows host; on other hosts it is skipped as ineligible, not a failure.
#
# Output:
#   builds/release/soshal_flutter[-release].apk | app-<abi>-…apk
#   builds/release/soshal_flutter-linux-x64.AppImage
#   builds/release/soshal_flutter-windows-x64[-release].zip

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"

resolve_script_dir
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

RELEASE=0
SPLIT_PER_ABI=0
for arg in "$@"; do
  case "$arg" in
    --release) RELEASE=1 ;;
    --android-split-per-abi) SPLIT_PER_ABI=1 ;;
    *) echo "usage: $0 [--release] [--android-split-per-abi]" >&2; exit 2 ;;
  esac
done

SUFFIX=""
if [[ "$RELEASE" -eq 1 ]]; then SUFFIX="-release"; fi

ANDROID_MARKER="$SCRIPT_DIR/.build-android-start"

echo "== Soshal release build =="

# Stale-artifact cleanup. Never delete build.sh itself (the earlier glob
# `rm -rf "$SCRIPT_DIR"/*` ate the running script — it survived via its open
# inode and completed, then was gone on disk).
find "$SCRIPT_DIR" -mindepth 1 -maxdepth 1 ! -name 'build.sh' -exec rm -rf {} +
rm -f "$ANDROID_MARKER"

HOST="$(uname -s 2>/dev/null || echo unknown)"
case "$HOST" in
  Linux)
    # --- Android ---
    echo "==[ android ]=="
    # Freshness marker: the android script leaves prior APKs in its own
    # folder across runs (e.g. a stale --release apk survives a debug run).
    # Only this-run artifacts are collected below (-newer marker).
    touch "$ANDROID_MARKER"
    ANDROID_ARGS=()
    if [[ "$RELEASE" -eq 1 ]]; then ANDROID_ARGS+=(--release); fi
    if [[ "$SPLIT_PER_ABI" -eq 1 ]]; then ANDROID_ARGS+=(--split-per-abi); fi
    "$PROJECT_ROOT/builds/android/build.sh" "${ANDROID_ARGS[@]}"
    echo "==[ linux ]=="
    "$PROJECT_ROOT/builds/linux/build.sh"
    ;;
  MINGW*|MSYS*)
    # --- Windows (Git Bash / MSYS2 host only) ---
    echo "==[ windows ]=="
    WINDOWS_ARGS=()
    if [[ "$RELEASE" -eq 1 ]]; then WINDOWS_ARGS+=(--release); fi
    "$PROJECT_ROOT/builds/windows/build.sh" "${WINDOWS_ARGS[@]}"
    ;;
  *)
    echo "ERROR: no eligible build targets for host '$HOST' (need Linux or Windows/Git Bash)" >&2
    exit 1
    ;;
esac

echo "== collecting artifacts =="

# Report ineligible targets so a skipped platform is never mistaken for a
# silent success.
SKIPPED=()
if [[ "$HOST" != "Linux" ]]; then
  SKIPPED+=(android linux)
fi
if [[ "$HOST" != "MINGW"* && "$HOST" != "MSYS"* ]]; then
  SKIPPED+=(windows)
fi
if [[ ${#SKIPPED[@]} -gt 0 ]]; then
  echo "  skipped (ineligible on host '$HOST'): ${SKIPPED[*]}"
fi

# Android: only APKs written by this run (find -newer marker) — prior APKs
# lingering in builds/android/ are stale and must not ship.
if [[ "$HOST" == "Linux" ]]; then
  APKS=()
  while IFS= read -r apk; do
    cp "$apk" "$SCRIPT_DIR/"
    APKS+=("$(basename "$apk")")
  done < <(find "$PROJECT_ROOT/builds/android" -maxdepth 1 -name '*.apk' \
            -newer "$ANDROID_MARKER" | sort)
  rm -f "$ANDROID_MARKER"
  if [[ ${#APKS[@]} -eq 0 ]]; then
    echo "ERROR: no fresh APKs found in builds/android/ after Android build" >&2
    exit 1
  fi
fi

# Linux AppImage (the linux script rewrites its own output every run, so a
# plain exists-check is enough to prove freshness).
if [[ -f "$PROJECT_ROOT/builds/linux/soshal_flutter-linux-x64.AppImage" ]]; then
  cp "$PROJECT_ROOT/builds/linux/soshal_flutter-linux-x64.AppImage" "$SCRIPT_DIR/"
else
  echo "ERROR: AppImage missing from builds/linux/ after Linux build" >&2
  exit 1
fi

# Windows: copy the runner bundle (exe + bridge DLL + data/ + flutter_runtime
# + plugins + mpv libs) next to the script and zip it into one artifact.
if [[ "$HOST" == "MINGW"* || "$HOST" == "MSYS"* ]]; then
  CONFIG="Debug"
  if [[ "$RELEASE" -eq 1 ]]; then CONFIG="Release"; fi
  BUNDLE="$PROJECT_ROOT/soshal_flutter/build/windows/x64/runner/$CONFIG"
  WIN_NAME="soshal_flutter-windows-x64$SUFFIX"
  STAGE="$SCRIPT_DIR/$WIN_NAME"
  if [[ ! -x "$BUNDLE/soshal_flutter.exe" ]]; then
    echo "ERROR: Windows bundle missing at $BUNDLE" >&2
    exit 1
  fi
  rm -rf "$STAGE"
  cp -r "$BUNDLE" "$STAGE"
  if command -v zip >/dev/null 2>&1; then
    ( cd "$SCRIPT_DIR" && zip -qr "$WIN_NAME.zip" "$WIN_NAME" )
    WIN_ARTIFACT="$SCRIPT_DIR/$WIN_NAME.zip"
  elif command -v tar >/dev/null 2>&1; then
    ( cd "$SCRIPT_DIR" && tar -czf "$WIN_NAME.tar.gz" "$WIN_NAME" )
    WIN_ARTIFACT="$SCRIPT_DIR/$WIN_NAME.tar.gz"
  else
    echo "ERROR: neither 'zip' nor 'tar' available to stage the Windows bundle" >&2
    exit 1
  fi
  rm -rf "$STAGE"
  if [[ ! -f "$WIN_ARTIFACT" ]]; then
    echo "ERROR: Windows artifact not produced at $WIN_ARTIFACT" >&2
    exit 1
  fi
fi

echo "== Release artifacts in $SCRIPT_DIR =="
( cd "$SCRIPT_DIR" && ls -lh )
echo "== Done =="