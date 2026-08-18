# Soshal – Agent Guidelines

Native social media app: Flutter (Dart) UI + Rust backend. All logic in Rust;
zero TypeScript; the legacy Dioxus WASM UI and Tauri desktop shell were
removed — Flutter is the only client.

## Project Structure

```
Soshal/
├── soshal_flutter/            # Flutter UI (the only client)
│   ├── lib/main.dart          # app root, provider registration (13+ services)
│   ├── lib/routes/app_router.dart  # go_router routes (~30)
│   ├── lib/frb_generated.dart # auto-generated FFI bindings (flutter_rust_bridge)
│   ├── lib/services/          # 16+ ChangeNotifier providers backing every screen
│   │                          #   (auth, feed, session, messaging+identity, notifications,
│   │                          #    search, dating, events, groups, marketplace, zap,
│   │                          #    streaming, moderation, network, signer, ffi_bridge)
│   │                          #   + permissions_service.dart (static class: Android
│   │                          #     permission_handler camera/mic + geolocator GPS;
│   │                          #     Linux XDG portal location via com.soshal/portal)
│   ├── lib/screens/           # ~24 screens: splash, auth, feed, composer, thread, inbox,
│   │                          #   profile, search, dating(+profile), events(+detail),
│   │                          #   groups(+detail), marketplace, live, stories, settings
│   │                          #   (+accounts, backup, blocked, moderation, security,
│   │                          #    notification-settings, edit-profile)
│   └── android/app/src/main/jniLibs/{arm64-v8a,x86_64,armeabi-v7a}/libsoshal_flutter_bridge.so
├── flutter-bridge/            # FFI adapter crate (soshal-flutter-bridge)
│   ├── frb.toml               # flutter_rust_bridge codegen config
│   └── src/ffi/               # 30 thin modules (auth, feed, messaging, session, …)
│       │                      #   each #[frb(sync|serialize)] fn delegates to *-core
│       ├── db.rs              #   SQLite access via db-core (with_db / with_db_result)
│       ├── signer.rs          #   in-process signer (nsec via FFI at init; OS keychain
│       │                      #   ops exchange pubkeys only — never key bytes)
│       └── frb_generated.rs   # codegen output — do NOT edit by hand
├── common-core/ … streaming-core/  # 30 *-core crates, pure Rust, tauri-free
├── scripts/
│   ├── check-core-compliance.sh    # core-crates purity audit (CI)
│   └── build-reticulum.sh          # builds rnsd for mesh networking
├── builds/
│   ├── android/build.sh            # 3-ABI bridge + flutter build apk [--release]
│   ├── linux/build.sh              # host bridge + flutter build linux --debug (terminal output)
│   └── README.md
└── Cargo.toml               # Workspace root, 31 members (flutter-bridge + 30 cores)
```

## Commands

```bash
cargo check --workspace                        # Type-check all crates
cargo test --workspace                         # Run all Rust tests (~1385: cores + bridge)
cargo fmt --check                              # Format check (pre-commit hook)
cargo clippy --workspace -- -D warnings        # Lint (pre-commit hook)
cargo audit                                    # Dependency audit (pre-push + CI)
cd soshal_flutter && flutter analyze           # Dart lint: must stay 0 errors / 0 warnings (infos OK)
cd soshal_flutter && flutter build apk --debug # Android debug APK
```

## Android (Flutter + bridge .so)

Rust cdylib must be built per ABI and dropped into `jniLibs/<abi>/`:

```bash
export ANDROID_HOME=$HOME/Android/Sdk ANDROID_SDK_ROOT=$HOME/Android/Sdk
export NDK_BIN=$ANDROID_HOME/ndk/27.1.12297006/toolchains/llvm/prebuilt/linux-x86_64/bin
export PATH=$NDK_BIN:$PATH      # separate statement: zsh expands RHS before assignment
# per ABI, triple ≤ android-24 sysroot:
#   arm64:  aarch64-linux-android21-clang         / aarch64-linux-android/24
#   x86_64: x86_64-linux-android21-clang          / x86_64-linux-android/24
#   armv7:  armv7a-linux-androideabi21-clang      / arm-linux-androideabi/24
CC_<triple>_linux_android=$NDK_BIN/<clang> \
RUSTFLAGS="-C linker=$NDK_BIN/<clang> -L native=$NDK_ROOT/.../sysroot/usr/lib/<libdir>/24" \
  cargo build -p soshal-flutter-bridge --release --target <triple> --target-dir /tmp/opencode/so-<abi>
cp /tmp/opencode/so-<abi>/<triple>/release/libsoshal_flutter_bridge.so \
   soshal_flutter/android/app/src/main/jniLibs/<abi>/
export PATH="$HOME/fvm/default/bin:$PATH"   # flutter via fvm
cd soshal_flutter && flutter build apk --debug
unzip -l build/app/outputs/flutter-apk/app-debug.apk | grep libsoshal_flutter_bridge  # verify 3 ABIs
```

NDK clang on PATH is required for ring's build script (probes
`aarch64-linux-android-clang`); reqwest uses
`default-features = false, features = ["json", "rustls-tls"]`.

**.so build gotchas** (2026-08 verified):
- rustc target is `armv7-linux-androideabi` (NO 'a'); NDK wrapper triple is
  `armv7a-linux-androideabi21-clang`.
- NDK 27 wrappers only set `--target` — no `--sysroot`; keep bare-name
  aliases (`arm-linux-androideabi-clang` — secp256k1-sys/aws-lc-sys probe for
  it) as **copied** scripts in `$SOSHAL_TARGET_DIR/ndk-bin` that add
  `--sysroot=$NDK_ROOT/sysroot` (wrappers resolve `dirname $0`, symlinks
  break them; `clang`/`clang++`/`ld.lld` may be symlinked in alongside).
  `builds/android/build.sh` does all this automatically.
- audiopus_sys builds libopus from source and IGNORES CC/CFLAGS (host .so
  leaks into cross builds). Prebuilt static opus (opus-1.4,
  `--disable-shared --enable-static --with-pic`) lives at
  `/tmp/opencode/opus-prefix-<abi>/lib`; build with
  `OPUS_LIB_DIR=… LIBOPUS_STATIC=1 OPUS_NO_PKG=1` and wipe its fingerprints
  (`find … -path "*audiopus*" -exec rm -rf {} +` — its build script doesn't
  rerun on env change). The build script fetches/static-builds opus into
  `/tmp/opencode/opus/<abi>` and wipes fingerprints for you.
- Cargo target dirs are big (~2GB/ABI); `/tmp` is tmpfs — both build scripts
  default them to disk-backed `$HOME/.cache/soshal-targets/` (override
  `SOSHAL_TARGET_DIR`).

**P2P transports** (network-core): TCP HMAC LAN chunk server (`lan_transport.rs`)
is the legacy bulk path; QUIC stream channel (`quic.rs` stream section) is the
preferred bulk path — `start_quic_stream_server[_with_store]` + blocking
`fetch_quic_chunk`/`fetch_quic_verified_chunk`, same HMAC beacon handshake +
private-IP-only + power-scheduler gating as TCP, plus rustls 0.23
`CryptoProvider` install (ring) inside `tls_configs()`. QUIC datagrams remain
micro-events only. MoQ group stream framing lives in
`streaming-core/src/moq.rs` (`encode_group_stream`/`decode_group_stream`,
little-endian binary, hostile-input capped), wired to quinn streams — live
media landed. FFI: `p2p_quic_server_start/port/stop`, `p2p_quic_fetch_chunk`,
`p2p_moq_encode_group/decode_group`, `p2p_moq_publish_group/subscribe_fetch`,
`p2p_fetch_blob_from_peer`. Swarm downloads prefer QUIC: mDNS TXT advertises
`quic_port` (`mdns.rs`), `P2pPeerDto.quicPort` surfaces it in Dart, and
`swarmDownload(quicPorts:)` (parallel `Vec<Option<u16>>`) falls back to TCP per
peer when null (`swarm.rs::fetch_from_peer`).

**Live media (Android, no Rust rebuild):** capture + playback run on Kotlin
`MethodChannel`s registered in `MainActivity.kt` — `com.soshal/h264`
(`H264Codec.kt`: MediaCodec hw AVC encode BGRA→I420→Annex-B `[flag,…]` blobs,
flag 1 = keyframe; sw-AVC decode → JPEG via `getOutputImage`→NV21→`YuvImage`)
and `com.soshal/audio` (`AudioCodec.kt`: AudioRecord 48 kHz mono → AAC-LC
64 kbps on a background thread, queue-drained; decode → AudioTrack; PCM shorts
MUST be little-endian — `ByteBuffer.order(LITTLE_ENDIAN)` before
`asShortBuffer`). Dart wrappers: `lib/services/h264_codec.dart`,
`lib/services/audio_codec.dart` (every call a safe no-op off-Android).
Semantics:
- MoQ tracks: 0 = JPEG keyframe groups (~4 fps fallback, codec-free),
  1 = H.264 groups (`VideoKeyframe`/`VideoDelta` per encoder flag),
  2 = AAC `AudioDatagram` groups (payload = `[2|1, …aac]` tag byte: 2 = codec
  config — published first + re-sent every 100 groups for late joiners,
  1 = audio frame).
- Broadcast: `live_broadcast_screen.dart` — camera (`camera` plugin,
  `ResolutionPreset.low`, `bgra8888` → `image` pkg JPEG q60 or H.264 path),
  15 fps H.264 / 4 fps JPEG throttles, mic toggle + 100 ms drain timer,
  group seq from `StreamingService.nextMoqGroupSeq()`.
- Viewers: `moq_viewer_screen.dart` — H.264 decode only starts after the
  first `VideoKeyframe` (deltas before GOP start are dropped), AAC frames
  dropped until the config object arrives; re-subscribes in 3 s windows.
- AndroidManifest: `android.permission.CAMERA` added.
- Playback quality/interop (JPEG track, keyframe gating) unverified on-device;
  build verification pending.

**frb codegen** (only when FFI signatures change): edit `flutter-bridge/src/ffi/*.rs`,
then run the post-regen ritual: `flutter_rust_bridge_codegen generate`
(v2.12.0) in `flutter-bridge`, reapply hand edits to generated glue if any,
copy `frb_generated.dart` (+ platform files) into `soshal_flutter/lib/`,
rebuild all 3 `.so`s, re-run `flutter analyze` + bridge tests. Generated Dart
methods are snake_case `crateFfi<Module><Fn>`; `#[frb(sync)]` fns return plain
values (awaiting them is harmless, not an error).

**Glue-trim ritual**: only 7 `lib/ffi/*.dart` glue files are kept (auth, db,
media, network, p2p, raster, session). After EVERY regen run
`scripts/trim-ffi-glue.sh` (deletes the 41 un-kept glue files + strips their
`import 'ffi/…'` lines from `frb_generated.dart`, `frb_generated.io.dart` AND
`frb_generated.web.dart`). 6 smoke tests stay in `test/ffi_manual/`:
auth/db/media/network/p2p/session (`test/ffi_manual/push_test.dart` etc. were
deleted — do not restore). `lib/ffi/raster.dart` must be kept: `frb_generated.dart`
references its `ImpellerFrameBufferInfo`.

**KNOWN codegen bug — io.dart corruption (frb 2.12.0)**: after EVERY regen,
check `grep -n "typedef bool" soshal_flutter/lib/frb_generated.io.dart`. The
cst-merge step deterministically splices a stray `typedef bool =
ffi.NativeFunction<...>` (from C's `DartPostCObjectFnType`) plus a dangling
`=> $allocator<WireSyncRust2DartSse>()…` fragment into
`wire_cst_list_String`'s section (right after `typedef DartDartPort = int;`).
That typedef SHADOWS `dart:core bool` for every importer → hundreds of bogus
`bool` mismatch errors across `lib/ffi/*.dart` + services + glued code (they
look "phantom" — they are real, caused by the shadowing). Delete that whole
block down to the `final class wire_cst_list_String` line; keep the
`typedef DartPort/DartDartPort` lines. Verify with
`dart analyze lib/frb_generated.io.dart lib/frb_generated.dart` — full-project
`flutter analyze` caches stale results after a regen; per-file `dart analyze`
is the source of truth.

**DB migrations** (db-core): `SCHEMA_VERSION` + a `v0NN_*.rs` migration file;
each migration SQL records its own version
(`INSERT OR IGNORE INTO _migrations (version) VALUES (N)`) and applies inside
`BEGIN IMMEDIATE … COMMIT`. Bump `SCHEMA_VERSION` when adding one.

## Architecture Rules

### FFI wiring (adapters are thin)

- Every bridge surface: `#[frb(sync, serialize)] pub fn <module>_<name>(...) ->
  Result<…, String>` in `flutter-bridge/src/ffi/<module>.rs` → codegen →
  Dart call via `RustLib.instance.api.crateFfi<Module><Name>(...)`.
- Stale codegen trap: frb_generated.rs can carry functions whose Rust side was
  deleted (`wire__…: n`). NEVER call a Dart fn whose Rust impl doesn't exist —
  verify with `rg "pub fn <module>_" flutter-bridge/src/ffi/` first.
- `flutter analyze` is the only reliable Dart lint; editor LSP diagnostics on
  multi-file Dart edits are frequently stale (last-green list: 0 errors,
  0 warnings, N info lints like `use_build_context_synchronously` — accepted).
- Services: one ChangeNotifier per domain, registered in `main.dart`; screens
  subscribe via `context.watch/read<Service>()`, never call RustLib directly
  except inside services.
- Sync vs async fns: most bridge fns are `#[frb(sync, serialize)]`; a few
  (`zap_*`, `network_*` status) are async-only. If generated Dart returns a
  plain type, it's sync — `await` still compiles and is a no-op.

### Core Crate Boundaries

- Each `*-core` crate: pure, platform-agnostic Rust — no tauri, no
  flutter_rust_bridge, no wasm, no ndk/jni (enforced by
  `scripts/check-core-compliance.sh` + CI). Desktop-specific deps (ring,
  libsql, nostr-sdk, reqwest) live in cores; `flutter-bridge` wires them to
  FFI.
- Only `flutter-bridge` may import platform crates.

### UI ↔ Backend

- Screens import `../services/*.dart` — never call `RustLib` or `FfiBridge`
  directly outside services; `FfiBridge` holds only `init()` + `getDbPath()`.
- Model classes belong to their owning service file (FeedPost → feed_service,
  DirectMessage/ProfileInfo → messaging_service, SessionAccount/Data →
  session_service, KeyPair → auth_service); do not recreate the deleted
  `models/` or legacy `ffi_bridge` dumps.

### Testing

- Unit tests inline in each core + integration tests in `tests/<crate>_tests.rs`;
  flutter-bridge has lib + integration tests (`tests/flutter_bridge_tests.rs`).
- Test command: `cargo test --workspace`.
- CI gate: 0 errors + 0 warnings on `flutter analyze` (info lints tolerated).

### CI + Hooks

- `.husky/pre-commit`: `cargo fmt --check` + clippy
- `.husky/pre-push`: `cargo check --workspace && cargo test --workspace` (+ audit if installed)
- `.github/workflows/ci.yml`: fmt, check, test, cargo-audit, ffi-bridge-tests,
  core-compliance, flutter-lint (pinned SHAs), plus a `coverage` job that gates
  line coverage ≥80% (`cargo llvm-cov --workspace --fail-under-lines 80`) and
  `flutter test` inside the flutter-lint job.
- Test-parallelism rule: bridge tests touching the process-global signer state
  (`signer_lock`/`signer_unlock`) MUST hold `test_lock::SIGNER_TEST_LOCK`;
  tests touching the global DB handle hold `DB_TEST_LOCK`. Missing the signer
  lock = cross-module flake ("signer locked" assertions racing unlocks).
- db-core in-memory gotcha: `Database::open_in_memory` pools connections and
  extra `connect()`s get FRESH empty in-memory databases — never hold one
  `conn()` guard while calling a repo method (it forces a second, empty conn).

## Security Invariants (inherited from the pre-Flutter hardening audit)

- **Key material**: nsec enters the bridge only at signer init (mnemonic at
  onboarding, or keyring restore). OS keychain ops (`signer_*_keyring`) take
  pubkeys only — never raw keys across FFI. `zeroize` around intermediates.
- **Signer lock**: `signer_lock` wipes in-memory keys; unlock via keychain or
  recovery phrase. Lock UI lives in Security settings.
- **Relay data is untrusted**: every relay-fetched surface filters through
  `commands::util::verified_events`/`event.verify()` + p-tag-to-me checks;
  outgoing URLs block private/loopback (SSRF); zap totals come from the
  BOLT-11 invoice amount only.
- **NIP-44 v2** (`crypto-core/src/nip44.rs`): `2 ‖ nonce ‖ ciphertext ‖ hmac`;
  legacy decode kept for stored data only, never emitted.
- **SQLite (libsql)**: Turso `libsql` 0.6 (bundled, async API) replaced
  rusqlite (2026 migration). All DB access runs through
  `soshal_db_core::block_on(async …)` (`pub` in `db-core/src/lib.rs`). API
  notes: `libsql::Connection` is an Arc-backed `Clone` struct with `&self`
  async methods; `libsql::params!`/`params_from_iter`/`named_params!` exist;
  NO IntoParams for 1-tuples (use `[T; 1]`); use `params![x.as_str()]` (can't
  move out of `&String`); `libsql::Builder::new_local(..).build()` returns a
  Future (wrap in `block_on`); `Row::get_value(i32)` yields
  `Value::{Null,Integer,Real,Text,Blob}`. `trusted_schema=OFF` +
  `secure_delete=ON`; migrations transactional.
- **Webview/CSP**: N/A (no webview UI anymore) — content is native Flutter.

## Key Architecture Decisions

1. **All-Rust logic** — business logic in 30 `*-core` crates, thin FFI
   adapter, Flutter UI. Tauri/Dioxus/WASM deleted (2026 migration).
2. **Thin adapters** — `flutter-bridge/src/ffi/` delegates to cores; no
   business logic in bridge fns.
3. **In-process signer** — desktop secrets-agent subprocess model retired with
   Tauri; the Flutter app uses the in-process signer + OS keychain.
4. **Backend-gated surfaces** — mostly real now; remaining gaps are
   external-infra only:
- Real: `zap_fetch_invoice`/`zap_send_payment` (NIP-47 NWC exchange),
      friend suggestions (WoT over contact graph) + friend requests
      (kind-3 follows), `webrtc_get_turn_servers` reads `turn_endpoint`
      setting, `minis_fetch` (local DB kind-31020 registry, fed by sync
      engine), `analytics_compute_stats` (SQL aggregates),
      `background_sync_task` (bounded engine pass), dating profile
      update/report persistence, hashtag extraction + geohash encoding,
      dating reactions (local + outbox-relayed), events RSVP/check-in
      (outbox-relayed, 500 m geofence), push token scoping (active
      account), preference-aware compatibility score (dealbreakers +
      weights), identicon avatar fallback (media-core, seeded by pubkey).
   - Still gated (needs external infra, UI honest about it; fake-success
      stubs honest-ified 2026-08 — silent simulations now return explicit
      `Err` + UI "unavailable (roadmap)" notes): FCM delivery (Firebase
      `google-services.json`; `registerPushToken` guarded on token presence),
      TURN provisioning (server endpoint; `webrtc_get_turn_servers` Err when
      `turn_endpoint` unset), WebRTC voice/video media transport (relay
      signaling kinds 20001-20004 work), WASI wasm runtime in minis-core
      (`minis_wasm_execute_filter`/`minis_wasm_rank_feed` Err — hex
      validation kept, wasmtime host roadmap; minis screen shows badge),
      ZK provers in sync-core zk_rollup (honest SHA-256 commitment; sync
      service verifies after apply, labels "commitment" not "proof"),
      `raster_signal_impeller_frame_ready` (engine-integration no-op), real
      FROST in crypto-core (bridge Err "non-cryptographic simulation,
      disabled"; moderation screen: no fake share input, "jury voting
      unavailable" note), eBPF kernel modes in network-core (`KernelTcXdp`/
      `SocketFilterBpf` Err at `EbpfShaper::new`; user-space fallback only),
      freenet seednode announce (`announce_to_seednodes_json` Err).
      `protocol_handle_avatar` is private inside
      protocol_handler.rs (was pub, downgraded — not an FFI surface).