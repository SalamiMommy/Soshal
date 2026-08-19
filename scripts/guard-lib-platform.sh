#!/usr/bin/env bash
# Guard: lib/ must not reach into platform channels or platform facts.
#
# Architecture rule: all platform access goes through Rust (frb ffi). Dart
# UI code may use dart:io for plain file I/O, but NOT MethodChannels, NOT
# plugin platform services from the denylist, and NOT `Platform.is*` fact
# sniffing (use the ffi platform fns instead, e.g.
# `permissionsPlatformCurrent()`).
#
# Exemptions (documented):
#   - ffi_bridge.dart  : the FFI loader itself must pick the native library
#                        per host OS (dart:io Platform here is the boundary).
#   - utils/offthread.dart: FLUTTER_TEST env probe defined by the test runner.
#   - Platform.pathSeparator: plain path building, not a platform fact.
#   - test/**, lib/ffi/**: generated bridge glue, out of scope.
#
# Usage: scripts/guard-lib-platform.sh   (exit 1 = violations)
# Wired into CI (flutter-lint job) and pre-commit.

set -u
cd "$(dirname "$0")/.."
LIB=soshal_flutter/lib
FAIL=0

fail() {
  echo "guard-lib-platform: $1"
  FAIL=1
}

# 1. MethodChannel construction (all platform channels must be Rust-side).
HITS=$(grep -rnE "MethodChannel[[:space:]]*\(" "$LIB" --include='*.dart' --exclude-dir=ffi || true)
[ -n "$HITS" ] && { echo "$HITS"; fail "MethodChannel usage in lib/ — platform channels must live in Rust"; }

# 2. Removed plugin platform services must not come back.
for dep in permission_handler battery_plus connectivity_plus; do
  HITS=$(grep -rn "package:${dep}/" "$LIB" --include='*.dart' || true)
  [ -n "$HITS" ] && { echo "$HITS"; fail "banned plugin '${dep}' imported in lib/"; }
done

# 3. `Platform.is*` fact sniffing (exempt: ffi_bridge.dart loader).
HITS=$(grep -rn "Platform\.\(is[A-Z]\|operatingSystem\|version\|numberOfProcessors\)" \
  "$LIB" --include='*.dart' | grep -v 'ffi_bridge.dart' | grep -v 'offthread.dart' || true)
[ -n "$HITS" ] && { echo "$HITS"; fail "Platform.is* facts must come from Rust (ffi platform fns)"; }

exit $FAIL