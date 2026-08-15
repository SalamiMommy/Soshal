#!/bin/bash

set -euo pipefail

SOSHAL_TARGETS_DIR="${SOSHAL_TARGET_DIR:-$HOME/.cache/soshal-targets}"   # disk-backed (tmpfs /tmp is too small)

resolve_flutter_bin() {
  FLUTTER_BIN="${FLUTTER_BIN:-}"
  if [[ -z "$FLUTTER_BIN" ]]; then
    FLUTTER_BIN="$(command -v flutter || true)"
  fi
  if [[ -z "$FLUTTER_BIN" ]] && [[ -x "$HOME/fvm/default/bin/flutter" ]]; then
    FLUTTER_BIN="$HOME/fvm/default/bin/flutter"
  fi
  if [[ -z "$FLUTTER_BIN" ]]; then
    echo "flutter not found on PATH or in ~/fvm; set FLUTTER_BIN to its location" >&2
    exit 1
  fi
}