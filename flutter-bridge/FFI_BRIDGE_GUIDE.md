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
│  src/ffi/*.rs — 30 thin modules      │
│  #[frb(sync, serialize)] fns         │
└──────────────┬───────────────────────┘
               │ direct fn calls
               ↓
┌──────────────────────────────────────┐
│ 27 *-core crates (all logic)         │
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
- A few are async-only (`pub async fn` in `protocol_handler.rs`, `zap.rs`,
  `network.rs`, `media.rs`) → Dart receives a `Future`.

### ⚠️ Stale codegen trap

`frb_generated.rs` can carry wire fns whose Rust side was deleted (e.g.
`network_add_relay`, `network_init_relays`, `network_get_relay_status`,
`network_publish_event`, `network_subscribe`, `network_query_events`,
`network_remove_relay`, `network_unsubscribe`). NEVER call a generated fn
without verifying the Rust impl exists:

```bash
rg "pub fn <module>_" flutter-bridge/src/ffi/
```

An unwired call panics at runtime (undefined symbol).

## Module Map (30 modules in `src/ffi/`)

| Module | Covers |
|--------|--------|
| `auth` | mnemonic generate/restore, npub encode |
| `signer` | in-process signer: pubkey, lock state, lock, keychain ops (pubkeys only) |
| `session` | multi-account load/save/switch |
| `identity` | profiles, trust score, block/unblock, follow, WoT status |
| `crypto` | NIP-44 v2 encrypt/decrypt, PQC, signing primitives |
| `db` | SQLite via db-core (`with_db`/`with_db_result`) |
| `feed` | publish (kind 1, pipelines through FTS index), reactions, fetch/delete |
| `messaging` | DM send/decrypt/fetch (NIP-44) |
| `groups` | NIP-29 create/join/post/fetch messages |
| `search` | FTS5 posts/profiles/hashtags + index hooks (`search_index_*`) |
| `dating` | profiles, like/superlike, matches |
| `events` | nearby, RSVP, check-in |
| `marketplace` | listings, orders, reviews |
| `notifications` | unread, mark read, per-type |
| `moderation` | mute lists, word filters, `should_filter` |
| `network` | I2P/Freenet status (async) — relay API is stale-only, see trap |
| `media` | local load, mime, fetch (async) |
| `zap` | LNURL/BOLT-11 (async; `zap_fetch_invoice` is a stub → always Err until NWC) |
| `streaming`, `webrtc` | stream status, ICE/SDP sanitize |
| `social`, `relations` | friend suggestions (stub → `vec![]`), send friend request (stub) |
| `analytics`, `minis`, `push`, `spatial`, `storage`, `content`, `util`, `protocol_handler` | misc utilities |

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
- **SQLite**: bundled rusqlite, `trusted_schema=OFF` + `secure_delete=ON`,
  transactional migrations.

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
| `network_subscribe`/`publish_event` | stale codegen — never call (relay wiring lands with backend) |
| Tauri IPC / webview | gone (2026-08); Flutter is the only client |