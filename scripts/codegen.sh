#!/usr/bin/env bash
# Automated flutter_rust_bridge codegen pipeline:
# 1. Runs flutter_rust_bridge_codegen generate (v2.12.0)
# 2. Applies automatic sanitization for known frb 2.12.0 io.dart corruptions
# 3. Trims unused FFI glue files
# 4. Validates frb_generated files with dart analyze
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/fvm/default/bin:$PATH"

echo "==> [1/4] Running flutter_rust_bridge_codegen generate..."
(cd "$ROOT_DIR/flutter-bridge" && flutter_rust_bridge_codegen generate)

echo "==> [2/4] Sanitizing frb_generated.io.dart..."
python3 "$ROOT_DIR/scripts/sanitize-frb.py"

echo "==> [3/4] Trimming FFI glue..."
"$ROOT_DIR/scripts/trim-ffi-glue.sh"

echo "==> [4/4] Validating generated Dart bindings..."
(cd "$ROOT_DIR/soshal_flutter" && dart analyze lib/frb_generated.io.dart lib/frb_generated.dart)

echo "==> FRB codegen pipeline completed successfully!"
