#!/bin/bash
# Build the Windows desktop app in dev (debug) mode and bundle the Rust
# bridge DLL next to the executable.
#
#   ./builds/windows/build.sh [--release]
#
# Run from Git Bash / MSYS2 on Windows. Requires:
#   - Visual Studio (MSVC toolchain; `flutter doctor` must pass for Windows)
#   - Rust with the Windows target installed:
#       rustup target add x86_64-pc-windows-msvc
# The bridge DLL is copied into the Flutter bundle (the exe's directory),
# where Dart's ffi_bridge.dart opens `soshal_flutter_bridge.dll`.
#
# Daemons are NOT bundled on Windows (rnsd/i2pd/freenet have no repo-built
# Windows binaries) — the bundled-daemons surfaces degrade honestly.
# Output: soshal_flutter/build/windows/x64/runner/{Debug,Release}/soshal_flutter.exe

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"

resolve_script_dir
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# This script must run under a Bash-on-Windows (Git Bash / MSYS2). WSL is a
# Linux guest: the MSVC linker and Windows flutter tooling aren't reachable
# from it in the normal way.
case "$(uname -s 2>/dev/null)" in
  MINGW*|MSYS*) : ;;            # native Bash on Windows — good
  *)
    echo "ERROR: run builds/windows/build.sh from Git Bash / MSYS2 on Windows" >&2
    echo "  (got uname: $(uname -s 2>/dev/null || echo unknown))" >&2
    exit 1
    ;;
esac

resolve_flutter_bin

CACHE_DIR="$SOSHAL_TARGETS_DIR/so-windows"
WINDOWS_TARGET="${WINDOWS_TARGET:-x86_64-pc-windows-msvc}"
BRIDGE="soshal-flutter-bridge"

RELEASE=0
case "${1:-}" in
  "") ;;
  --release) RELEASE=1 ;;
  *) echo "usage: $0 [--release]" >&2; exit 2 ;;
esac

echo "== Soshal Windows build =="

# Fail fast if the Rust target is missing (rustc on Windows defaults to the
# MSVC host triple, so this is normally already installed).
if ! rustc -vV 2>/dev/null | grep -q "host: $WINDOWS_TARGET" \
   && ! rustup target list --installed 2>/dev/null | grep -q "^${WINDOWS_TARGET}$"; then
  echo "ERROR: Rust target $WINDOWS_TARGET not installed." >&2
  echo "  Add it with: rustup target add $WINDOWS_TARGET" >&2
  exit 1
fi

# Rust profile follows the Flutter build mode: cargo release only when
# flutter builds release, so caching stays sharp (debug cache reused on debug
# runs, release cache on release runs).
PROFILE="debug"
CONFIG="Debug"
CARGO_FLAGS=()
if [[ "$RELEASE" -eq 1 ]]; then
  PROFILE="release"
  CONFIG="Release"
  CARGO_FLAGS+=(--release)
fi

echo "  bridge: cargo build ($PROFILE profile)"
cargo build -p "$BRIDGE" --target "$WINDOWS_TARGET" \
  --target-dir "$CACHE_DIR" "${CARGO_FLAGS[@]}"

OUT_DLL="$CACHE_DIR/$WINDOWS_TARGET/$PROFILE/soshal_flutter_bridge.dll"
if [[ ! -f "$OUT_DLL" ]]; then
  echo "ERROR: bridge DLL not produced at $OUT_DLL" >&2
  exit 1
fi
export SOSHAL_BRIDGE_DLL="$OUT_DLL"

cd "$PROJECT_ROOT/soshal_flutter"
echo "  flutter: build windows $CONFIG"
"$FLUTTER_BIN" build windows $([ "$RELEASE" -eq 1 ] && echo --release)

# The bundle lives in the build tree; the DLL lands beside the exe via the
# windows/CMakeLists.txt install rule (SOSHAL_BRIDGE_DLL).
BUNDLE="build/windows/x64/runner/$CONFIG"
EXE="$BUNDLE/soshal_flutter.exe"
if [[ ! -x "$EXE" ]]; then
  echo "ERROR: expected bundle not found at $BUNDLE" >&2
  exit 1
fi

if [[ -f "$BUNDLE/soshal_flutter_bridge.dll" ]]; then
  echo "  bridge DLL verified beside the exe ($(stat -c%s "$BUNDLE/soshal_flutter_bridge.dll")B)"
else
  echo "  WARNING: soshal_flutter_bridge.dll not found next to the exe" >&2
fi

echo "== Done: $EXE =="
echo "  Run it: $EXE"
echo "  (Media playback is bundled via media_kit_libs_windows_video — no manual mpv step.)"