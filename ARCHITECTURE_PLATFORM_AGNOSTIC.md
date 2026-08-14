# Soshal Architecture: Platform-Agnostic Core

## Principle

**All core business logic lives in Rust core crates using only standard, platform-agnostic dependencies.**

Platform bindings are **thin adapters only**—they marshal data, invoke core functions, and return results. No business logic should ever live in adapter layers. The only client today is the Flutter app via the `flutter-bridge` FFI adapter; the legacy Tauri 2.0 and Dioxus WASM adapters were deleted (2026-08).

```
┌───────────────────────────────────────────────────────┐
│  Platform Layers (Thin Adapters)                      │
├────────────────────────┬──────────────────────────────┤
│  Flutter UI            │  FFI Adapter                 │
│  (soshal_flutter)      │  (flutter-bridge, 30 modules)│
└───────────┬────────────┴────────────┬─────────────────┘
            │                         │
            │  Generated Dart (frb)   │ #[frb(sync, serialize)] fns
            ↓                         ↓
┌────────────────────────────────────────────────────────┐
│  Core Crates (All Business Logic)                      │
│  27 pure Rust libraries                                │
├────────────────────────────────────────────────────────┤
│  Standard Rust Dependencies ONLY:                       │
│  - tokio (async runtime)                               │
│  - rusqlite (SQLite)                                   │
│  - reqwest (HTTP, with rustls-tls)                     │
│  - nostr-sdk (Nostr protocol)                          │
│  - ring (crypto primitives)                            │
│  - serde / serde_json (serialization)                  │
│  - regex, base64, hex, url, etc. (utilities)           │
│                                                        │
│  NO platform deps: no tauri, no flutter_rust_bridge,   │
│  no Android/iOS SDKs, no wasm-bindgen                  │
└────────────────────────────────────────────────────────┘
```

## Core Crate Categories

### Tier 1: Zero Dependencies (Pure Logic)

These crates have NO external dependencies, only `serde`:

- **common-core** — URL validation, safe rendering, formatting
- **content-core** — Hashtag/mention extraction, compression, URL parsing
- **social-core** — Relationship tracking primitives
- **spatial-core** — Geolocation utilities

### Tier 2: Crypto & Encoding (Minimal Deps)

These use only crypto libraries + serde:

- **crypto-core** — NIP-44 v2, PQC (KEM/DSA), HMAC, HKDF
  - Deps: `ring`, `chacha20poly1305`, `chacha20`, `aead`, `hex`, `base64`
  - NO async, NO I/O
  
- **identity-core** — Key generation, mnemonics, BIP-39/BIP-32
  - Deps: `nostr`, `bip39`, `bip32`, `ring`
  - Pure functions, NO I/O

### Tier 3: Data & State (DB + Serialization)

These layer Rust data structures + DB access:

- **db-core** — SQLite pool, schema, migrations
  - Deps: `rusqlite` (bundled SQLite)
  - Pragmas tuned per-platform (Android vs. desktop) but logic is same

- **nostr-core** — Nostr event model, key operations
  - Deps: `nostr-sdk`
  - Event creation, serialization (NO relay I/O here)

- **feed-core** — Feed ranking, aggregation
  - Deps: `nostr-core`, `rayon` (parallel iteration)
  - Pure algorithmic logic

- **groups-core** — NIP-29 group logic
- **messaging-core** — NIP-44/NIP-04 message wrapping
- **dating-core** — Scoring, filtering
- **marketplace-core** — Listing/order state machine
- **notification-core** — Filtering + prioritization
- **search-core** — FTS5 query building

### Tier 4: Network & I/O (Async, Reqwest)

These use async + tokio + reqwest:

- **network-core** — Relay pool, WebSocket subscriptions, I2P/Freenet routing
  - Deps: `tokio`, `reqwest`, `nostr-sdk`, `url`
  - ASYNC functions return `Result<T, String>` or `Future<Output=Result<T, String>>`
  - No platform-specific code; conditionals are protocol-based only

- **media-core** — Blossom HTTP fetch, image metadata
  - Deps: `reqwest`, `url`
  - ASYNC, cacheable results

- **streaming-core** — WebRTC peer management
  - Deps: `tokio`, depends on what WebRTC crate
  - ASYNC peer lifecycle

- **zap-core** — LNURL fetching + BOLT-11 parsing
  - Deps: `reqwest`
  - ASYNC

- **webrtc-core** — ICE config generation, SDP sanitization
  - Deps: `regex` for SDP parsing
  - Pure logic (no platform I/O)

### Tier 5: Analytics & Tools (Optional Deps)

- **analytics-core** — Event logging, stats
- **moderation-core** — Content filtering, word lists
- **pqc-core** — Post-quantum cryptography (liboqs bindings, if used)

### Special: Platform Adapter (NOT a Core)

- **flutter-bridge** — FFI adapter, 30 `#[frb]` modules, thin marshaling only
- **soshal_flutter** — Flutter UI (NO business logic); services call the bridge, screens call services

## Dependency Rules

### ✅ ALLOWED in Core Crates

**Async Runtime:**
- `tokio` (with default features or specific subset)
- `async-trait`

**HTTP/Networking:**
- `reqwest` with `features = ["json", "rustls-tls"]` (never openssl-sys)
- `url`
- `ring` for crypto primitives

**Database:**
- `rusqlite` with `bundled` feature (SQLite is bundled, always available)

**Serialization:**
- `serde`, `serde_json`
- `base64`, `hex`, `regex`
- `bip39`, `bip32` (for keys only)
- `nostr`, `nostr-sdk` (Nostr protocol)

**Utilities:**
- `rayon` (parallel iteration, safe for CPU-bound work)
- `getrandom` (RNG seeding)
- `uuid` (ID generation)
- `zeroize` (secure memory clearing)
- `lazy_static` (static initialization)

### ❌ FORBIDDEN in Core Crates

- `flutter_rust_bridge`, `tauri` (bridges/adapters, not core)
- `wasm-bindgen`, `web-sys` (core must not know about WASM)
- `tokio-desktop` or platform-specific async
- `openssl-sys` (use ring or rustls instead)
- ANY Android/iOS SDK bindings (NDK, JNI, Swift bridges)
- `sqlx` (we use rusqlite for embedded DB)
- `ndk`, `android`, `objc`, `cocoa` (platform code belongs in adapters)

### ⚠️ CONDITIONAL: Platform Pragmas Only

Some crates MAY have `#[cfg(target_os = "...")]` **but ONLY for tuning, not logic**:

```rust
// ✅ GOOD: Pragma tuning per-platform
#[cfg(target_os = "android")]
const CACHE_SIZE: &str = "PRAGMA cache_size=-16000";
#[cfg(not(target_os = "android"))]
const CACHE_SIZE: &str = "PRAGMA cache_size=-64000";

// ❌ BAD: Different logic per-platform
#[cfg(target_os = "android")]
fn do_something() { /* Android-specific code */ }
#[cfg(not(target_os = "android"))]
fn do_something() { /* Desktop code */ }
// Instead: Abstract with traits, let adapter call appropriate path
```

## Function Signature Pattern

All core functions should return standard Rust types:

### Synchronous (CPU-bound, crypto, data structures)

```rust
/// Parse a nostr event from JSON
pub fn parse_event(json: &str) -> Result<Event, String> {
    serde_json::from_str(json)
        .map_err(|e| format!("Invalid event JSON: {}", e))
}

/// Calculate feed ranking score
pub fn rank_event(event: &Event, viewer_pubkey: &str) -> f32 {
    // Pure algorithm, no I/O
}
```

### Asynchronous (I/O-bound: network, disk, relay queries)

```rust
/// Fetch media from Blossom server
pub async fn fetch_media(url: &str) -> Result<Vec<u8>, String> {
    reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

/// Subscribe to relay events
pub async fn subscribe_relay(relay_url: &str, filter: &str) -> Result<String, String> {
    // Returns subscription ID; events pushed via callback or channel
}
```

## Adapter Pattern

The FFI adapter (`flutter-bridge/src/ffi/*.rs`) follows this template:

### Example: FFI Adapter (Good)

**flutter-bridge/src/ffi/auth.rs:**
```rust
use soshal_identity_core::mnemonic::generate_mnemonic; // Core import

#[frb(sync, serialize)]
pub fn auth_generate_mnemonic() -> Result<String, String> {
    // 1. Call core
    generate_mnemonic()
    // 2. The error is already a String; no logic lives here
}
```

**Not:**
```rust
// ❌ DON'T do business logic in the adapter
#[frb(sync, serialize)]
pub fn auth_generate_mnemonic() -> Result<String, String> {
    // bip39 key-derivation logic belongs in identity-core, not here
    todo!()
}
```

## Testing Strategy

### Core Crates: Unit Tests Only

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_parsing() {
        let json = r#"{"kind":1,"content":"hello"}"#;
        let event = parse_event(json).expect("should parse");
        assert_eq!(event.content, "hello");
    }

    #[tokio::test]
    async fn test_fetch_media() {
        let data = fetch_media("https://example.com/image.jpg")
            .await
            .expect("should fetch");
        assert!(!data.is_empty());
    }
}
```

**No platform-specific tests in cores.** All integration testing happens in:
- `flutter-bridge` integration tests (bridge-specific behavior)
- Flutter integration tests (platform-specific)

### Integration Tests

```bash
# Test all cores and the adapter (no platform deps needed)
cargo test --workspace

# Test Flutter (Dart lint gate: 0 errors / 0 warnings)
cd soshal_flutter && flutter analyze
```

## Dependency Audit

### Run This to Check Compliance

```bash
# Find any adapter deps in core crates (should be empty)
grep -r "flutter_rust_bridge\|tauri" *-core/ Cargo.toml

# Find any platform-specific code in cores
grep -r "#\[cfg(target_" *-core/ --include="*.rs" | grep -v "PRAGMAS\|test"

# Check for forbidden crates
grep -r "openssl-sys\|sqlx\|ndk\|android\|objc" Cargo.lock
```

### Workspace Lints

**Cargo.toml (root):**
```toml
[workspace.lints.rust]
unsafe_code = "deny"  # Prevent unsafe code in cores (must use safe APIs)

[workspace.lints.clippy]
todo = "warn"
dbg_macro = "deny"  # No debug prints in production
```

**Each core crate:**
```toml
[lints]
workspace = true
```

## Migration Checklist (If Refactoring)

- [ ] Audit all `*-core/src/*.rs` for platform imports
- [ ] Move platform-specific code to adapters (`flutter-bridge`)
- [ ] Replace `sqlx` with `rusqlite` if used
- [ ] Replace `openssl-sys` with `ring` or `rustls`
- [ ] Ensure all async code uses `tokio` (not platform-specific runtimes)
- [ ] Check all HTTP calls use `reqwest` with `rustls-tls`
- [ ] Remove any `#[cfg(target_os = "...")]` for logic (keep only for tuning)
- [ ] Add workspace lints for unsafe_code = "deny"
- [ ] Run `cargo test --workspace` to verify cores work standalone
- [ ] Verify `flutter-bridge` only marshals data

## Verification Commands

```bash
# Ensure cores compile standalone (no platform deps)
cd common-core && cargo build && cd ..
cd db-core && cargo build && cd ..
cd network-core && cargo build && cd ..

# Check all cores can be tested without platform deps
cargo test --workspace

# Verify the adapter is thin (calls into cores, no business logic)
grep -c "^use soshal_" flutter-bridge/src/ffi/*.rs   # delegates to 27 cores
grep -c "logic\|algorithm\|state" flutter-bridge/src/ffi/*.rs
```

## Benefits of This Architecture

| Benefit | Why |
|---------|-----|
| **Testability** | Core logic tested independently of platform |
| **Code reuse** | Flutter (mobile/desktop) + any future client share the exact same cores |
| **Performance** | No bloat from unused platform code |
| **Maintenance** | Bug fixes in core apply to all platforms automatically |
| **Security** | Crypto/auth logic auditable without platform noise |
| **Flexibility** | Easy to add new platforms (CLI, Electron, etc.) |

## Platform Onboarding (Adding a New Platform)

To add a new platform (e.g., CLI, Electron, native app):

1. Create adapter layer (new crate, e.g., `soshal-cli`)
2. Import cores: `use soshal_identity_core::*; use soshal_feed_core::*;`
3. Wire core functions to platform API
4. Run `cargo build -p soshal-cli` — should just work

**Example: CLI**
```bash
cd cli-adapter
cargo new --lib .
# Cargo.toml includes core crate deps
# src/lib.rs exposes command wrappers
# main.rs = CLI argument parsing + calling adapters
```

No core logic changes needed. Ever.

---

**Last Updated:** August 12, 2026  
**Architecture Version:** 2.0 (All-Rust, Platform-Agnostic)  
**Status:** ENFORCE with lints and CI checks
