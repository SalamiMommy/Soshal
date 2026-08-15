#!/usr/bin/env bash
# Post-frb-regen cleanup: drop the 41 never-imported generated glue files
# from lib/ffi/ and their import lines from frb_generated.dart.
# Run AFTER flutter_rust_bridge_codegen generate + io.dart corruption fix.
# Keep-list: auth db media network p2p session (imported by app code).
set -euo pipefail
cd "$(dirname "$0")/.."
DIR=soshal_flutter/lib/ffi
KEEP="auth db media network p2p pqc raster session"
for f in "$DIR"/*.dart; do
  base=$(basename "$f" .dart)
  if [[ " $KEEP " != *" $base "* ]]; then
    rm -f "$f"
    for gen in frb_generated.dart frb_generated.io.dart frb_generated.web.dart; do
      sed -i "/^import 'ffi\/$base\.dart';$/d" soshal_flutter/lib/$gen
      sed -i "/^import \"ffi\/$base\.dart\";$/d" soshal_flutter/lib/$gen
    done
  fi
done
echo "ffi glue trimmed (keep: $KEEP)"
