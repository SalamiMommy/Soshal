#!/bin/bash
# Soshal Core Crates Dependency Audit & Compliance Checker
# Ensures all core business logic uses only standard, platform-agnostic Rust
#
# Usage: bash check-core-compliance.sh

set -e

echo "========================================"
echo "Soshal Core Compliance Audit"
echo "========================================"
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

ERRORS=0
WARNINGS=0

# Helper functions
error() {
    echo -e "${RED}❌ ERROR: $1${NC}"
    ERRORS=$((ERRORS + 1))
}

warning() {
    echo -e "${YELLOW}⚠️  WARNING: $1${NC}"
    WARNINGS=$((WARNINGS + 1))
}

success() {
    echo -e "${GREEN}✅ $1${NC}"
}

# 1. Check for forbidden dependencies in CORE crates ONLY (not adapters)
echo "1. Checking for forbidden dependencies in core crates (not adapters)..."
FORBIDDEN_DEPS=("tauri" "flutter_rust_bridge" "wasm-bindgen" "web-sys" "openssl-sys" "sqlx" "ndk" "android" "objc" "jni")

for dep in "${FORBIDDEN_DEPS[@]}"; do
    # Only check *-core crates, NOT the flutter-bridge adapter
    if grep -r "$dep" *-core/Cargo.toml 2>/dev/null | grep -v "^Binary"; then
        error "Found forbidden dependency '$dep' in core crates"
    fi
done

# 2. Check for platform-specific business logic in cores
echo ""
echo "2. Checking for platform-specific business logic in cores..."

# Allow #[cfg(test)] and specific PRAGMA tuning, but flag others
PLATFORM_CFG_COUNT=$(grep -r "#\[cfg(target_" *-core/ --include="*.rs" 2>/dev/null | grep -v "test" | grep -v "PRAGMAS" | wc -l)

if [ "$PLATFORM_CFG_COUNT" -gt 0 ]; then
    warning "Found $(echo "$PLATFORM_CFG_COUNT") platform-specific #[cfg(...)] directives in core crates"
    echo "  (Acceptable only if used for tuning, not logic)"
    grep -r "#\[cfg(target_" *-core/ --include="*.rs" 2>/dev/null | grep -v "test" | grep -v "PRAGMAS" | head -5
else
    success "No platform-specific logic in core crates"
fi

# 3. Verify all async code uses tokio
echo ""
echo "3. Checking async runtime consistency..."

if grep -r "async_std\|smol\|embassy" *-core/Cargo.toml 2>/dev/null | grep -v "^Binary"; then
    error "Found non-tokio async runtime in core crates (must use tokio)"
else
    success "All async code uses tokio (or no async)"
fi

# 4. Verify HTTP client consistency
echo ""
echo "4. Checking HTTP client consistency..."

if grep -r "hyper\|actix" *-core/Cargo.toml 2>/dev/null | grep -v "^Binary"; then
    warning "Found alternative HTTP client (recommended: reqwest only)"
else
    success "HTTP client is consistent (reqwest or none)"
fi

# 5. Check for dangerous crypto libraries
echo ""
echo "5. Checking crypto library consistency..."

if grep -r "openssl\|md5\|sha1" *-core/Cargo.toml 2>/dev/null | grep -v "^Binary"; then
    error "Found weak or platform-dependent crypto (use ring instead)"
else
    success "Crypto uses ring or libsodium (safe)"
fi

# 6. Verify database access pattern
echo ""
echo "6. Checking database abstraction..."

DB_CORE_HAS_LIBSQL=$(grep -c "libsql" db-core/Cargo.toml 2>/dev/null || echo "0")
if [ "$DB_CORE_HAS_LIBSQL" -gt 0 ]; then
    success "DB core uses libsql (Turso embedded SQLite)"
else
    error "DB core should use libsql, not sqlx or other"
fi

# 7. Core crates should not import Tauri or Flutter
echo ""
echo "7. Checking for cross-adapter contamination..."

if grep -r "use.*flutter_rust_bridge" *-core/ --include="*.rs" 2>/dev/null; then
    error "Core crates must not import from tauri or flutter adapters"
else
    success "Core crates have no platform adapter imports"
fi

# 8. Adapters are expected to import platform crates (tauri, flutter_rust_bridge)
echo ""
echo "8. Checking adapter imports..."

if grep -r "use flutter_rust_bridge" flutter-bridge/src/ffi/*.rs 2>/dev/null | grep -q "frb"; then
    success "Flutter adapter correctly imports flutter_rust_bridge"
else
    error "Flutter adapter missing flutter_rust_bridge import"
fi

if grep -r "use flutter_rust_bridge" flutter-bridge/src/*.rs 2>/dev/null | grep -q "frb"; then
    success "Flutter adapter correctly imports flutter_rust_bridge"
else
    error "Flutter adapter missing flutter_rust_bridge import"
fi

# 9. Compile all cores standalone
echo ""
echo "9. Testing core crates compile independently..."

cd common-core
if cargo build --quiet 2>/dev/null; then
    success "common-core compiles independently"
else
    error "common-core failed to compile"
fi
cd ..

cd db-core
if cargo build --quiet 2>/dev/null; then
    success "db-core compiles independently"
else
    error "db-core failed to compile"
fi
cd ..

cd network-core
if cargo build --quiet 2>/dev/null; then
    success "network-core compiles independently"
else
    error "network-core failed to compile"
fi
cd ..

# 10. Run tests on cores only
echo ""
echo "10. Running core crate tests..."

if cargo test --workspace --quiet 2>/dev/null; then
    success "All core tests pass"
else
    error "Some core tests failed (review output)"
fi

# 11. Check for secrets in code
echo ""
echo "11. Checking for hardcoded secrets..."

if grep -r "password\|secret\|api_key\|token" *-core/ --include="*.rs" | grep -E '=\s*"[^"]{5,}"' | grep -v "test\|TODO\|FIXME\|alias"; then
    warning "Possible hardcoded secrets found (review above)"
else
    success "No obvious hardcoded secrets"
fi

# 12. Verify serde usage
echo ""
echo "12. Checking serialization consistency..."

if grep -r "bincode\|msgpack\|rmp\|protobuf" *-core/Cargo.toml 2>/dev/null | grep -v "^Binary"; then
    warning "Found alternative serialization format (serde_json recommended for cross-platform)"
else
    success "Serialization is consistent (serde_json or none)"
fi

# Summary
echo ""
echo "========================================"
echo "Audit Summary"
echo "========================================"

if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo -e "${GREEN}✅ All checks passed! Core is platform-agnostic.${NC}"
    exit 0
elif [ $ERRORS -eq 0 ]; then
    echo -e "${YELLOW}⚠️  $WARNINGS warning(s) - Review and address if needed${NC}"
    exit 0
else
    echo -e "${RED}❌ $ERRORS error(s) found - Must fix before merging${NC}"
    exit 1
fi
