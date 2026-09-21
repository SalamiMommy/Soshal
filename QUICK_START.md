# Soshal Quick Start

Soshal is a fully native Flutter app (Android/iOS/desktop) backed by a Rust
core: 32 platform-agnostic `*-core` crates + one `flutter-bridge` FFI adapter,
exposed to Dart through auto-generated bindings (`frb_generated.dart`). The
legacy Dioxus/Tauri client was removed; Flutter is the only UI.

## Build & Run

```bash
# Flutter app
cd soshal_flutter
flutter pub get
flutter build apk          # Android (debug: flutter build apk --debug)
flutter build ios          # iOS
flutter run                # Desktop preview (optional)

# Rust workspace (cores + FFI bridge)
cargo check --workspace
cargo test --workspace
```

Android `.so` builds for the bridge use the NDK per-ABI link recipe (see
`AGENTS.md`); `libsoshal_flutter_bridge.so` ships in
`soshal_flutter/android/app/src/main/jniLibs/{arm64-v8a,x86_64,armeabi-v7a}/`.

## Architecture

```rust
// Cores (32 crates, all platform-agnostic, pure Rust)
soshal_identity_core::mnemonic::generate_mnemonic()  // Pure Rust
soshal_crypto_core::nip44::NIP44::encrypt(...)        // Pure Rust
soshal_network_core::relay::RelayPool::new(...)       // Async Rust
soshal_db_core::repo::Database::open(...)             // SQLite (libsql)

// Platform Adapter (thin marshaling only)
// flutter-bridge/src/ffi/auth.rs → calls core → generated Dart (frb_generated.dart)
```

## FFI Modes

### 1. Sync FFI (CPU-bound)
```rust
#[frb(sync)]
pub fn crypto_sha256_hex(input: String) -> Result<String, String> {
    // ...
}
```
Dart call is plain (non-Future), `await` is harmless.

### 2. Async FFI (I/O-bound: network, disk)
```rust
#[frb]
pub async fn zap_fetch_invoice(lnurl: String, amount_msat: u64, comment: String, _nostr_event: String) -> Result<String, String> {
    // ...
}
```
Dart: `final data = await ffi.zapFetchInvoice(...)`.

## Key Files

| File | Purpose |
|------|---------|
| `flutter-bridge/src/ffi/*.rs` | 55 FFI modules (auth, feed, messaging, permissions, power, daemon, …) |
| `flutter-bridge/FFI_BRIDGE_GUIDE.md` | FFI patterns |
| `ARCHITECTURE_PLATFORM_AGNOSTIC.md` | Core organization + design principles |
| `scripts/check-core-compliance.sh` | Automated compliance checker |
| `soshal_flutter/lib/` | Flutter UI (services, screens, routes) |
| `soshal_flutter/lib/frb_generated.dart` | Auto-generated FFI bindings (frb codegen) |

## Testing Before Merge

```bash
# Verify cores are platform-agnostic (no tauri/dioxus/flutter leakage)
bash scripts/check-core-compliance.sh

# Run all Rust tests (cores + bridge)
cargo test --workspace

# Check formatting + lint
cargo fmt --check
cargo clippy --workspace -- -D warnings

# Flutter lint (must be 0 errors, 0 warnings)
cd soshal_flutter && flutter analyze
```

Hooks: pre-commit runs `cargo fmt --check` + clippy (+ `guard-lib-platform.sh`);
pre-push runs `cargo check --workspace && cargo test --workspace` (+ `cargo
audit` when installed). CI mirrors these plus `ffi-bridge-tests`,
`core-compliance`, and `flutter-lint` jobs (0 errors / 0 warnings on
`flutter analyze`).

## Common Tasks

### Add a New FFI Function

1. **Add to Rust module** (`flutter-bridge/src/ffi/feed.rs`):
```rust
#[frb(sync, serialize)]
pub fn feed_new_feature(param: String) -> Result<bool, String> {
    // Call core
    Ok(true).into()
}
```

2. **Regenerate bindings** (frb codegen 2.12.0):
```bash
cd flutter-bridge
flutter_rust_bridge_codegen generate   # verify frb.toml first — see AGENTS.md ritual
```

3. **Use in Flutter** (`lib/services/feed_service.dart`):
```dart
final ok = RustLib.instance.api.crateFfiFeedFeedNewFeature(param: 'x');
```

### Update Core Logic (e.g., feed ranking)

1. **Edit core** (`feed-core/src/ranking.rs`) — pure functions only.
2. **FFI wrapper** already delegates; Dart interface unchanged.
3. Rebuild the Android `.so` per target ABI (NDK recipe in `AGENTS.md`)
   and replace them under `jniLibs/`.

## Backend-Gated Surfaces (roadmap only)

These FFI/Rust sites need external infrastructure — UI buttons are honest
about them (explicit "unavailable (roadmap)" notes, never silent fake
success):
- Push notifications — need Firebase (`google-services.json`) + FCM
- TURN provisioning — `webrtc_get_turn_servers` Err until `turn_endpoint` is set
- WebRTC voice/video media transport — relay signaling kinds 20001-20004 work,
  media still roadmapped
- WASI wasm runtime in minis-core (`minis_wasm_execute_filter`/`minis_wasm_rank_feed` Err)
- ZK provers in sync-core — honest SHA-256 commitments, not proofs
- Real FROST threshold signing in crypto-core (Err, disabled)
- eBPF kernel modes in network-core (`KernelTcXdp`/`SocketFilterBpf` Err;
  user-space fallback only)
- Freenet seednode announce

Everything else — `zap_fetch_invoice`/`zap_send_payment` (NIP-47 NWC), friend
suggestions/requests, `minis_fetch`, dating reactions, scheduled publishing,
notifications, analytics — is real (verifiable against
`flutter-bridge/src/ffi/*.rs`; use `rg "pub fn <module>_"` to confirm).

## Troubleshooting

### "symbol not found" / stale bindings
- Bindings live in `soshal_flutter/lib/frb_generated.dart` + platform glue.
  Only regenerate when FFI signatures change (see `AGENTS.md` for the full
  post-regen ritual: codegen → merge glue → copy files).
- `flutter clean && flutter pub get`

### Native lib not loading on Android
- Confirm the ABI matches: `jniLibs/<abi>/libsoshal_flutter_bridge.so`
- Confirm the NDK linker flags match the recipe (ring/proc-macro crates need
  the right sysroot lib dir per ABI).

### DMs not decrypting
- NIP-44 v2 only; inbox bubbles show 🔒 until Decrypt is tapped; the
  conversation list previews undecrypted ciphertext with a lock prefix.