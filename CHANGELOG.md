# Changelog

All notable changes to Soshal.

## [0.2.0] — 2026-08-12

### Added
- Flutter-only client: native UI replaces the legacy Dioxus WASM webview stack
- `flutter-bridge`: 30 `#[frb]` FFI modules wired end-to-end — every real Rust
  surface reachable from the UI (auth, feed, messaging, search/FTS indexing,
  groups, events, marketplace, dating, moderation, network status, signer,
  trust scores, post deletion with index cleanup, mention autocomplete,
  hashtag extraction)
- `builds/android/build.sh` + `builds/linux/build.sh`: one-shot native builds
  (3-ABI bridge + APK / host bridge + Linux bundle)
- Signer lock + OS keychain restore UI (pubkeys only across FFI, no key bytes)
- Moderation screen (mutes, word filters, content checker), network status
  section (I2P/Freenet), backup stats card

### Changed
- Removed legacy client: `src-ui` (Dioxus WASM), `src-tauri`, `soshal-tools`,
  `secrets-agent`, WASM/Tauri build tooling, migration-era docs (476 files)
- Workspace: 28 members (`flutter-bridge` + 27 `*-core` crates); 580 Rust
  tests pass; `flutter analyze` gated at 0 errors / 0 warnings
- Dep tooling pruned (tauri/wasm/qrcode/bech32/libc/anyhow/uuid dead deps);
  go_router 17, flutter_lints 6
- CI rewritten: fmt, clippy, check, test, cargo-audit, ffi-bridge tests,
  core-compliance, flutter-lint; husky hooks updated

### Removed
- Webview/CSP security surface (content is native Flutter now)

## Unreleased

- Native Reticulum mesh stack integration (`soshal-network-core::reticulum`) with 128-bit destination address derivation, packet encoding, SLIP framing, mesh path routing, identity ANNOUNCE, UDP/Multicast/RNode interfaces, and Nostr event sync
- MuteConversationService extraction from MuteService
- Test suite reorganisation into domain directories

## [1.0.0] — 2026-07-22

### Added
- Social feed with post composer, reactions, replies, reposts, bookmarks
- End-to-end encrypted direct messaging (NIP-44)
- NIP-29 groups with channels, roles, permissions
- Swipe-based dating with 10-dimension compatibility scoring
- NIP-15 marketplace with listings, reviews, escrow
- NIP-52 calendar events with RSVP
- Custom profiles with drag-and-drop widget builder
- Live streaming (NIP-30080/30081)
- Short-form video (Minis, kind 31020)
- Musicloud audio tracks
- ChatRandom peer/group matching with WebRTC
- Stories with reactions and expiry
- Post-quantum crypto (ML-KEM-768, ML-DSA-65, Double Ratchet)
- WoT spam filter and friend-of-friend discovery
- I2P transport with automatic proxy detection
- Data hosting at configurable hop depth
- Stealth privacy mode with whitelist
- Multi-account support
- Biometric lock and PIN protection
- Lightning zaps via NWC/LNURL
- 210+ services across 12 domains
- 132 Rust compute kernels compiled to WASM
- 45 screens in 11 feature folders
- 49 domain model modules

### Security
- 60+ security vulnerabilities remediated across Rust/TS/Electron
- SSRF protection in URL validation
- Prototype pollution prevention in JSON parsing
- Constant-time PIN comparison
- PQC-derived encryption keys for SecureStore
- Audit logging for security events

### Changed
- All services converted to DI via ServiceContainer
- Pure-TS fallbacks extracted to src/services/fallback/
- Rust FFI macros unified in macros.rs
- cstring_or_empty consolidated in shared.rs
- 40+ DB query-builder modules with RepositoryBase
