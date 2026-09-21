# Soshal Flutter FFI Bridge

Direct Flutter → Rust communication via `flutter_rust_bridge` (v2.12.0).
No webview, no IPC server, no Tauri. This is the only client path into the
workspace.

## Architecture

```
┌──────────────────────────────────────┐
│ Flutter App (Dart)                   │
│  screens → services → frb_generated  │
└──────────────┬───────────────────────┘
               │ generated Dart API (crateFfi<Module><Fn>)
               ↓
┌──────────────────────────────────────┐
│ soshal-flutter-bridge (Rust)         │
│  src/ffi/*.rs — 55 thin modules      │
│  #[frb(sync, serialize)] fns         │
└──────────────┬───────────────────────┘
               │ direct fn calls
               ↓
┌──────────────────────────────────────┐
│ 32 *-core crates (all logic)         │
└──────────────────────────────────────┘
```

Rules:

- The adapter is thin: every bridge fn delegates to a core; zero business
  logic in `src/ffi/`.
- Dart never calls `RustLib` outside `lib/services/*.dart`; screens use
  services only.
- Model classes live in their owning service file (one service per domain).

## Naming & Typing

- Bridge fns are `<module>_<name>` snake_case in Rust; generated Dart is
  `crateFfi<Module><Fn>` (camelCase).
- Most fns are `#[frb(sync, serialize)]` returning `Result<…, String>` →
  Dart receives a plain value (`await` on it compiles and is a no-op).
- Async-only fns (`pub async fn`) are the exception (~30 across analytics,
  auth, calls, chatrandom, crypto, events, feed, guestbook, headless,
  identity, media, messaging, minis, music, network, p2p, permissions, pin,
  power, protocol_handler, render, scheduled, search, signer, storage, sync,
  vouch, zap) → Dart receives a `Future`.

### ⚠️ Stale codegen trap

`frb_generated.rs` can carry wire fns whose Rust side was deleted (e.g.
`streaming_start_local_server`, `streaming_get_video_url`,
`streaming_stop_local_server`, `moderation_hybrid_classify_media`,
`search_index_post` singular — only the plural `search_index_posts`
remains). NEVER call a generated fn without verifying the Rust impl exists:

```bash
rg "pub fn <module>_" flutter-bridge/src/ffi/
```

An unwired call panics at runtime (undefined symbol).

## Module Map (55 modules in `src/ffi/`)

| Module | Covers |
|--------|--------|
| `auth` | mnemonic generate/restore, npub encode, keypair |
| `signer` | in-process signer: pubkey, lock state, lock, keychain ops (pubkeys only) |
| `session` | multi-account load/save/switch (hardened path validation) |
| `identity` | profiles, trust score, block/unblock, follow, WoT status |
| `crypto` | NIP-44 v2 encrypt/decrypt, PQC, signing primitives |
| `db` | SQLite via db-core (`with_db`/`with_db_result`) |
| `turso` | Turso Cloud replication: URL/token config, sync trigger, status |
| `feed` | publish (kind 1, pipelines through FTS index), reactions, fetch/delete |
| `messaging` | DM send/decrypt/fetch (NIP-44) |
| `groups` | NIP-29 create/join/post/fetch messages |
| `search` | FTS5 posts/profiles/hashtags + index hooks (`search_index_*`) |
| `dating` | profiles, like/superlike, matches |
| `events` | nearby, RSVP, check-in |
| `marketplace` | listings, orders, reviews |
| `notifications` | unread, mark read, per-type |
| `moderation` | mute lists, word filters, `should_filter` |
| `network` | relay pool (add/remove/subscribe/publish/query — all live), I2P/Freenet/SAM, Reticulum mesh, multi-bearer status |
| `relay` | local relay node start/stop/status (`soshal-relay-core`) |
| `p2p` | mDNS discovery, LAN chunk server, peer blob fetch, QUIC streams, swarm downloads |
| `media` | local load, mime, fetch (async) |
| `zap` | LNURL/NWC (NIP-47): connect, fetch invoice, send payment, receipts, totals |
| `streaming`, `webrtc` | live status, MoQ group encode/decode, ICE/SDP sanitize |
| `protocol_handler` | relay events ingest / handler (avatar feeds, protocol fns) |
| `sync` | background engine start/stop/running, outbox, watermark |
| `scheduled` | scheduled posts (future `scheduled_at`, sync publishes on time) |
| `social`, `relations`, `vouch`, `guestbook` | friend suggestions / friend requests (kind-3), WoT vouches (31989), guestbook entries (30080/30081) |
| `analytics`, `audit`, `telemetry` | engagement/social stats, audit log, telemetry read |
| `minis` | kind-31020 registry, WASM filter/rank (Err — host roadmapped) |
| `music` | audio tracks, waveform, playlists (SoundCloud parity) |
| `calls`, `chatrandom` | WebRTC call mgmt, chatroulette pairing/modes |
| `ephemeral` | burn-after-read DM media (view-count capped) |
| `bookmarks` | save/bookmark to collections |
| `permissions` | Android/Linux runtime camera/mic/location state + requests (JNI / XDG portal) |
| `power` | battery/connectivity power sampling for the seeding scheduler |
| `daemon` | bundled daemon lifecycle (i2pd, freenet, rnsd): extract, spawn, liveness |
| `ebpf` | traffic-shaper (kernel modes Err; socket-filter user-space fallback) |
| `h264`, `audio` | Android MediaCodec H.264/AAC codecs (Rust FFI) |
| `raster` | Impeller frame-buffer injection (`raster_signal_impeller_frame_ready` no-op) |
| `render` | WGPU compute shaders for offscreen mesh layout |
| `headless` | background task runner (WorkManager / BGTaskScheduler, no Flutter engine) |
| `zk` | ZK state rollup verify/apply (honest SHA-256 commitments) |
| `spatial`, `storage`, `content`, `util`, `pin` | geohash/geofence, blob cache, content utils, helpers, PIN lock |

## Usage Pattern

**Rust (`src/ffi/messaging.rs`):**
```rust
#[frb(sync, serialize)]
pub fn messaging_send_dm(content: String, recipient: String, sender: String) -> Result<String, String> {
    soshal_messaging_core::send(content, recipient, sender).map_err(|e| e.to_string())
}
```

**Rust (`src/ffi/network.rs`, async example):**
```rust
#[frb(serialize)]
pub async fn network_i2p_status() -> Result<String, String> { /* … */ }
```

**Dart (`lib/services/messaging_service.dart`):**
```dart
final id = await RustLib.instance.api.crateFfiMessagingSendDm(
  content: text, recipient: pubkey, sender: myPubkey,
);
```

## Codegen Ritual (only when FFI signatures change)

1. Edit `flutter-bridge/src/ffi/*.rs`.
2. `cd flutter-bridge && flutter_rust_bridge_codegen generate` (v2.12.0).
3. Reapply hand edits to generated glue if any.
4. Copy `frb_generated.dart` (+ platform files) into `soshal_flutter/lib/`.
5. Rebuild all 3 Android `.so`s (`builds/android/build.sh`) or host `.so`
   (`builds/linux/build.sh`), then `flutter analyze` + bridge tests.

## Security Invariants

- **Key material**: nsec enters the bridge only at signer init (mnemonic at
  onboarding, or keyring restore). `signer_*_keyring` ops pass pubkeys only —
  never raw key bytes across FFI. `zeroize` intermediates.
- **Signer lock**: `signer_lock` wipes in-memory keys; unlock via keychain or
  recovery phrase.
- **Relay data untrusted**: relay surfaces filter through
  `verified_events`/`event.verify()` + p-tag-to-me; outgoing URLs block
  private/loopback (SSRF); zap totals come from the BOLT-11 amount only.
- **NIP-44 v2**: wire format `2 ‖ nonce ‖ ciphertext ‖ hmac`; legacy decode
  kept only for stored data, never emitted.
- **SQLite**: bundled Turso `libsql`, `trusted_schema=OFF` + `secure_delete=ON`,
  transactional migrations. All DB access runs through `soshal_db_core::block_on`.

## Debugging

- Bridge errors are `Err(String)` wrapped as Dart exceptions by services;
  keep UI messages user-friendly.
- Sync fns block the Dart isolate — do heavy work in the cores' tokio pools
  via the async variants where relevant.
- Generated Dart lives in `soshal_flutter/lib/frb_generated.dart` — do not
  hand-edit; re-run codegen.

## Replacements (legacy → current)

| Old (pre-refactor) | Current |
|--------------------|---------|
| `FfiResult<T>` wrapper | `Result<…, String>` directly |
| `lib/bridge_generated.dart` via `build_runner` | `flutter_rust_bridge_codegen generate` → `lib/frb_generated.dart` |
| `app://media/...` webview protocol | native Flutter widgets + `media_load_local`/mime via service |
| `network_subscribe`/`publish_event` (old wire-only fns) | **now live** async relays fns in `network.rs` — callable; see trap |
| Tauri IPC / webview | gone (2026-08); Flutter is the only client |