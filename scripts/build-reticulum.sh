#!/bin/bash
# Build standalone Reticulum (rnsd) binary as a Python package entry point.
# On desktop (Linux/macOS), packages the RNS wheel + PyInstaller into rnsd.
# On mobile (Android), use Python-for-Android (future).

set -e

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../builds/common.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RELEASE_DIR="$PROJECT_ROOT/target/release"

mkdir -p "$RELEASE_DIR"

PLATFORM="$(uname -m)-$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$PLATFORM" in
    x86_64-linux|aarch64-linux|x86_64-darwin|aarch64-darwin)
        RNS_VERSION="1.4.2"
        RNS_WHEEL="rns-${RNS_VERSION}-py3-none-any.whl"
        RNS_URL="https://github.com/markqvist/Reticulum/releases/download/${RNS_VERSION}/${RNS_WHEEL}"
        ;;
    *)
        echo "Unsupported platform: $PLATFORM"
        echo "Reticulum build only supported on Linux and macOS"
        exit 0
        ;;
esac

if ! command -v python3 &> /dev/null; then
    echo "Python 3 required"
    exit 1
fi

PIP_CMD="pip3"
if ! command -v pip3 &> /dev/null; then
    PIP_CMD="pip"
fi

BUILD_DIR=$(mktemp -d)
trap "rm -rf $BUILD_DIR" EXIT

echo "Building Reticulum standalone binary for $PLATFORM"

# Download RNS wheel
echo "Downloading rns wheel from $RNS_URL..."
cd "$BUILD_DIR"
if ! curl -L -f -o "$RNS_WHEEL" "$RNS_URL"; then
    echo "Failed to download rns wheel; creating stub"
    create_stub "" "Reticulum daemon stub - wheel download failed"
    exit 0
fi

echo "rns wheel downloaded successfully"

# Create virtualenv
echo "Creating temporary virtualenv..."
python3 -m venv "$BUILD_DIR/venv"
source "$BUILD_DIR/venv/bin/activate"

echo "Installing rns wheel and PyInstaller..."
"$BUILD_DIR/venv/bin/pip" install "$RNS_WHEEL" pyinstaller 2>&1 | tail -5 || {
    echo "Failed to install rns or PyInstaller"
    create_stub "" "Reticulum daemon stub - install failed"
    exit 0
}

# Create wrapper that imports and runs RNS
cat > "$BUILD_DIR/rnsd_wrapper.py" << 'EOF'
#!/usr/bin/env python3
"""
Reticulum daemon wrapper for PyInstaller bundling.
"""
import sys

try:
    import RNS
    from RNS.Utilities.rnsd import main
    main()
except ImportError as e:
    print(f"Error: Could not import RNS: {e}", file=sys.stderr)
    sys.exit(1)
except Exception as e:
    print(f"Error: Failed to start Reticulum daemon: {e}", file=sys.stderr)
    import traceback
    traceback.print_exc()
    sys.exit(1)
EOF

chmod +x "$BUILD_DIR/rnsd_wrapper.py"

echo "Building standalone binary with PyInstaller..."
pyinstaller \
    --onefile \
    --name rnsd \
    --distpath "$RELEASE_DIR" \
    --console \
    --collect-all RNS \
    --copy-metadata rns \
    "$BUILD_DIR/rnsd_wrapper.py" 2>&1 | tail -10 || {
    echo "PyInstaller failed; creating stub"
    create_stub "" "Reticulum daemon stub - PyInstaller failed"
    exit 0
}

chmod +x "$RELEASE_DIR/rnsd"
echo "Reticulum standalone binary built: $RELEASE_DIR/rnsd"

