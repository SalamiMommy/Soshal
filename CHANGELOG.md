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

### Added
- `mesh-core` crate: exotic/mesh transports split out of `network-core`
  (freenet, i2p_sam, reticulum, ble, wifi_direct, pqc_link, p2p_frame);
  `network-core` re-exports them so `network_core::<mesh_module>` paths still
  resolve
- QUIC stream channel as the preferred P2P bulk transport (rustls 0.23 +
  `HybridCertVerifier`, first-contact self-signed certs accepted for mesh
  peers); mDNS advertises `quic_port` and swarm downloads negotiate QUIC with
  TCP fallback
- Bundled networking daemons: per-ABI i2pd, NDK cross-build of freenet-core's
  `freenet` node (arm64), and in-process rnsd via Chaquopy (`rnspure`) —
  Android runtime fixed (freenet `--config-dir/--data-dir/
  --disable-auto-update`, i2pd `daemon=false`, RnsdRunner one-attempt latch),
  daemon logs readable in-app (Network settings → Bundled Daemons → Logs)
- Rust-runtime permissions (`ffi/permissions.rs`: JNI `checkSelfPermission` /
  XDG location portal) and power sampling (`ffi/power.rs`); local relay node
  (`ffi/relay.rs` + `relay-core`); Turso Cloud replication (`ffi/turso.rs`);
  scheduled posts (`ffi/scheduled.rs`); vouch (31989) + guestbook
  (30080/30081); PIN lock; ephemeral (burn) media; WGPU render + Impeller
  raster hooks
- 2-tier hybrid moderation filter; media playback unified on media_kit (mpv)
  across Android + Linux; live media codecs in Rust FFI (MediaCodec H.264 /
  AAC, MoQ tracks 0/1/2); 5 transport modes + device-as-relay mesh

### Changed
- Workspace: 34 members (`flutter-bridge` + 32 `*-core` crates + `test-util`);
  ~2500 Rust tests green (`cargo test --workspace`)
- SQLite backend on Turso `libsql` 0.10 (`0.10.0-pre.4`) — rusqlite gone;
  `battery_plus`/`connectivity_plus`/`permission_handler` plugins removed;
  `video_player` replaced by media_kit
- Kotlin `H264Codec.kt`/`AudioCodec.kt` + `com.soshal/{h264,audio}`
  MethodChannels deleted — codecs are Rust FFI now
- `android:exported="false"` for MainActivity; network security config with
  cleartext disabled + private-range P2P allowances
- DB migrations re-enabled: `v001_initial.rs` frozen as the squashed
  pre-release baseline (`SCHEMA_VERSION = 1`); schema changes now land as new
  `v0NN_*.rs` migration files with a `SCHEMA_VERSION` bump (forward-only from
  version 1 — v001 never mutated in place)

### Fixed
- Audit rounds 4-8 (docs/AUDIT.md): crash/robustness sweep, relay timestamp
  clamp, test flakes, sync-engine liveness watchdog, FFI drift
- Sept 16-21 hardening batches (5-20): sync/search/relay/streaming/audio/
  mesh/zk, social/vouch/webrtc/zap/guestbook/chatrandom, logic/authorization/
  case-sensitivity/input-bound sweeps
- Signer rate limits, session integrity, p2p transport races, cache-crash
  guards, scheduled publish engine (+ blocked-draft loop), notification
  producer (event-id keyed), reaction/like semantics, cursor pagination,
  NIP-44 legacy decode, SDP validation, base64url padding, linkpreview
  whitespace, runstls TLS CVE bump, Android XML build break, .so resync with
  round-3/4 Rust fixes, honest-fied simulated stubs
- Transport hardening (`763d592`, 2026-09-21): swarm refuses symlink
  `out_path` (WP11 path traversal); QUIC handshake/connect/stream-write
  timeouts + 16 MiB stream-frame cap (stalled peer can't pin accept loops or
  send buffers); gossip verify-before-amplify + per-account dedup keys (L7);
  bridge-identity p-tag checks; account switch stops the Rust mesh relay node
  + drops relay state (new-identity traffic never routes under old pubkey);
  freenet websocket + mesh relay fixes; honest relay status from bridge truth
  (no fabricated connected flags); publish error surfaced when no relay
  accepts; Linux location-service pre-check; relay status re-poll for offline
  banner

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
