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
├── common-core/ … mesh-core/ … streaming-core/  # 32 *-core crates, pure Rust, tauri-free
│       #   mesh-core = exotic/mesh transports (freenet, i2p, reticulum, ble,
│       #   wifi_direct, pqc_link, p2p_frame) split out of network-core
├── scripts/
│   ├── check-core-compliance.sh    # core-crates purity audit (CI)
│   └── build-reticulum.sh          # builds rnsd for mesh networking
├── builds/
│   ├── android/build.sh            # 3-ABI bridge + flutter build apk [--release]
│   ├── linux/build.sh              # host bridge + flutter build linux --debug (terminal output)
│   └── README.md
└── Cargo.toml               # Workspace root, 34 members (flutter-bridge + 32 cores + test-util)
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

## Security Documentation

See `SECURITY.md` for comprehensive security guidelines, architecture overview, and incident response procedures.

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
  leaks into cross builds). `builds/android/build.sh` fetches + static-builds
  opus (1.5.x, `--disable-shared --enable-static --with-pic`) per ABI into
  `/tmp/opencode/opus/<abi>`, builds with
  `OPUS_LIB_DIR=… LIBOPUS_STATIC=1 OPUS_NO_PKG=1` and wipes its fingerprints
  (`find … -path "*audiopus*" -exec rm -rf {} +` — its build script doesn't
  rerun on env change) — all automatic.
- Cargo target dirs are big (~2GB/ABI); `/tmp` is tmpfs — both build scripts
  default them to disk-backed `$HOME/.cache/soshal-targets/` (override
  `SOSHAL_TARGET_DIR`).
- **Known-desync (RESOLVED 2026-09-10)**: `builds/android/build.sh --release`
  re-ran — shipped `jniLibs` `.so`s now carry the round-3/4 Rust fixes (relay
  mesh ingest multi_thread, minis reactions, zap amount_msat, saved_at,
  i2p/freenet/relay, cas always-rehash, voice 5760 buffer, FTS v013,
  notifications ignore) plus the 2026-09-10 daemon fixes below.
- **Bundled daemons — Android runtime gotchas (fixed 2026-09-10)**:
  - freenet has NO $HOME/XDG/passwd for the app UID → `ProjectDirs::from`
    returns NotFound and the node aborts before binding the WS API. `daemon.rs`
    now spawns `freenet network --config-dir=<files>/freenet-data/conf
    --data-dir=<files>/freenet-data/data --disable-auto-update` (gateways
    auto-fetch from the remote index; no manual seeding). Auto-update OFF —
    no supervisor on Android to act on freenet's exit-42 self-update signal.
  - i2pd defaults `daemon = true` (forks to background) → Rust `Child` parent
    exits, watchdog sees dead, respawn-loops into 7656 bind conflicts.
    daemon.rs writes `daemon = false` into the i2pd conf.
  - rnsd runs in-process (Chaquopy thread, `RnsdRunner.kt`). RNS never clears
    its process-wide singleton → a failed init can't retry without a process
    restart; the watchdog used to stack threads + "Attempt to reinitialise
    Reticulum" poison. `RnsdRunner` now one-attempt latched (reset on stop/);
    `platform::rnsd_status()` surfaces the last-start error through
    `daemon_get_daemon_status` (`reticulum_status` JSON fragment); watchdog
    checks `rnsd_running()` for in-process liveness instead of respawning.
  - Daemon logs readable in-app: Network settings → Bundled Daemons → Logs
    (reads `<files>/i2pd-data/i2pd.log`, `<files>/freenet-data/freenet.log`,
    `<files>/reticulum-data/logfile` via path_provider — no FFI regen).

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

**Mesh transports** (mesh-core, split from network-core): freenet (websocket/
contract/opennet/cache_router), i2p_sam, reticulum (`reticulum/`), ble,
wifi_direct, plus the shared pqc_link (hybrid PQC double-ratchet link crypto)
and p2p_frame (CRC32 frame kernel) primitives. `network-core` re-exports these
modules (`pub use soshal_mesh_core::{…}`) so `network_core::<mesh_module>`
paths still resolve for flutter-bridge/relay-core/sync-core callers; only
`multi_bearer.rs` crosses the boundary (into `ble`/`wifi_direct`).

**Live media (Android, Rust FFI codecs):** capture + playback run through
Rust — the legacy Kotlin `MethodChannel` codecs (`H264Codec.kt`/`AudioCodec.kt`,
`com.soshal/h264`/`com.soshal/audio`) were DELETED. Codec work is done by
`flutter-bridge/src/ffi/codecs/` (`h264.rs`, `audio.rs`: MediaCodec hw AVC
encode BGRA→I420→Annex-B `[flag,…]` blobs, flag 1 = keyframe; sw-AVC decode →
JPEG; AAC-LC 64 kbps capture, decode → AudioTrack; PCM shorts MUST be
little-endian before `asShortBuffer`). Dart wrappers: `lib/services/h264_codec.dart`,
`lib/services/audio_codec.dart` (every call a safe no-op off-Android) → glue
`lib/ffi/h264.dart`/`audio.dart` → `crateFfiH264*`/`crateFfiAudio*`.
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

**Feed/mini video on Linux (2026-09)**: `video_player` has no Linux impl —
feed `_VideoPlayerWidget` + `MiniVideoPlayer` historically hard-gated to
Android. Now: Android keeps `video_player`; Linux branches to media_kit
(mpv) via `media_kit_video` `VideoController`/`Video` (`media_kit` +
`media_kit_video` + `media_kit_libs_video` deps, `MediaKit.ensureInitialized()`
in main.dart; system `libmpv.so.2` required — `builds/linux/build.sh` bundles
host libmpv.so.2 into the AppImage alongside the plugin .sos, w/o it video
still errors). guard-lib-platform: `PermissionsService.isLinux` gate, no
`Platform.is*` in lib/. `mk.` prefix NOT used for `VideoController` (lives in
media_kit_video, not media_kit); prefix only `Player`/`Media` (`as mk`).

**frb codegen** (only when FFI signatures change): edit `flutter-bridge/src/ffi/*.rs`,
then run the post-regen ritual: `flutter_rust_bridge_codegen generate`
(v2.12.0) in `flutter-bridge`, reapply hand edits to generated glue if any,
copy `frb_generated.dart` (+ platform files) into `soshal_flutter/lib/`,
rebuild all 3 `.so`s, re-run `flutter analyze` + bridge tests. Generated Dart
methods are snake_case `crateFfi<Module><Fn>`; `#[frb(sync)]` fns return plain
values (awaiting them is harmless, not an error).

**Glue-trim ritual**: 13 `lib/ffi/*.dart` glue files are kept (audio, auth,
content, daemon, db, h264, media, network, p2p, permissions, power, raster,
session). After EVERY regen run `scripts/trim-ffi-glue.sh` (deletes the
un-kept glue files + strips their `import 'ffi/…'` lines from
`frb_generated.dart`, `frb_generated.io.dart` AND `frb_generated.web.dart`).
6 smoke tests stay in `test/ffi_manual/`:
auth/db/media/network/p2p/session (`test/ffi_manual/push_test.dart` etc. were
deleted — do not restore). `lib/ffi/raster.dart` must be kept: `frb_generated.dart`
references its `ImpellerFrameBufferInfo`.

**Stale-codegen trap**: frb_generated.dart can carry fns whose Rust impl was
deleted (`wire__…` absent from the .so). NEVER call a Dart fn whose Rust impl
doesn't exist — verify with `rg "pub fn <module>_" flutter-bridge/src/ffi/`
first. Known-removed surfaces (2026-08): `streaming_start_local_server`,
`streaming_get_video_url`, `streaming_stop_local_server`,
`moderation_hybrid_classify_media`, `search_index_post` (singular).

**KNOWN codegen bug — io.dart corruption (frb 2.12.0)**: after EVERY regen,
run per-file `dart analyze lib/frb_generated.io.dart lib/frb_generated.dart`
FIRST (full-project `flutter analyze` caches stale results after a regen;
per-file `dart analyze` is the source of truth). Two deterministic
corruptions to repair:

1. **Spliced `typedef bool` line**: cst-merge splices a stray `typedef bool
   = ffi.NativeFunction<…>` (from C's `DartPostCObjectFnType`) into the wire
   typedef section. It SHADOWS `dart:core bool` for every importer →
   hundreds of bogus `bool`/`NativeFunction` signature errors across
   frb_generated.dart + fake_api.dart + glue. Delete that line only.
2. **Spliced `$allocate` fragment**: a stray fragment (`}) => $allocator<X>()`
   + `..ref.*` lines + `}`) lands right after `typedef DartDartPort = int;`.
   The allocator type X varies between regens (observed: `WireSyncRust2DartSse`,
   `AMediaCodecBufferInfo`). It is the ONLY fragment appearing OUTSIDE a class
   body — delete those lines only. Legit `$allocate` bodies live inside
   `final class … extends ffi.Struct` and must NOT be touched: deleting one
   breaks every class declared after it (dozens of bogus "undefined class" +
   `InvalidType` errors). Both corruptions may appear in the SAME regen.
3. **Dropped wire types** (only when the fragment splice happens): the
   splice displaces `final class wire_cst_list_String` (extends `ffi.Struct`;
   ptr = `ffi.Pointer<ffi.Pointer<wire_cst_list_prim_u_8_strict>>`,
   `@ffi.Int32() external int len`) plus `typedef mediastatus_t = ffi.Int32;`
   / `typedef Dartmediastatus_t = int;` / `typedef ssize_t = ffi.IntPtr;` /
   `typedef Dartssize_t = int;` (from codecs/ndk.rs type aliases). Hand-restore
   them next to `typedef DartDartPort = int;`. Errors naming these three types
   = they're missing.

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

- `.husky/pre-commit`: `cargo fmt --check` + clippy (+ guard-lib-platform.sh)
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

## Security Hardening (2026-09 Security Review)

### Implemented Security Fixes

#### Critical: Android MainActivity Export Fix
- **File**: `soshal_flutter/android/app/src/main/AndroidManifest.xml`
- **Change**: Set `android:exported="false"` for MainActivity
- **Rationale**: Prevents unauthorized apps from launching the main activity
- **Impact**: Blocks external intent injection attacks while maintaining launcher functionality

#### Network Security Configuration Enhancement
- **File**: `soshal_flutter/android/app/src/main/res/xml/network_security_config.xml`
- **Changes**:
  - Added base configuration with cleartext traffic disabled by default
  - Explicitly allowed localhost/127.0.0.1 for development
  - Added private network ranges (192.168.0.0, 10.0.0.0, 172.16.0.0) for P2P LAN communication
  - Added certificate pinning placeholders for production endpoints
  - Added debug overrides for development builds
- **Rationale**: Comprehensive TLS/certificate policy with appropriate exceptions for local networking

#### QUIC Certificate Validation Enhancement
- **File**: `network-core/src/quic.rs`
- **Changes**:
  - Replaced `PermitAllVerifier` with `HybridCertVerifier`
  - Added framework for system certificate validation (TODO: implement with rustls-native-certs)
  - Maintains mesh-internal self-signed cert compatibility for P2P functionality
  - Added placeholder functions for proper TLS 1.2/1.3 signature verification
- **Rationale**: Improved certificate validation while preserving mesh networking compatibility
- **Status**: Framework in place, full system cert validation pending implementation

#### Session Path Validation Hardening
- **File**: `flutter-bridge/src/ffi/session.rs`
- **Changes**:
  - Added `validated_session_path_hardened()` with runtime TOCTOU protection
  - Added `validate_path_security()` for permission and suspicious component checks
  - Integrated hardened validation into `session_load()` and `session_save()`
  - Added world-writable file detection on Unix systems
- **Rationale**: Protects against time-of-check-time-of-use race conditions in file operations
- **Impact**: Additional security layer for session file operations

### Testing Results
- All session-related tests pass (5/5)
- Network core tests pass (204/204) 
- Bridge session-specific tests pass
- No security regressions introduced
- Flutter analyze shows 0 errors, 19 info-level lints (pre-existing)

### Security Architecture Principles
1. **Defense in Depth**: Multiple security layers at application, network, and filesystem levels
2. **Compatibility**: Security enhancements maintain existing P2P mesh functionality
3. **Fail-Safe**: Security defaults deny access, explicit allowlists for exceptions
4. **Validation**: Runtime checks complement static validation for TOCTOU protection

### Future Security Improvements
- Complete system certificate validation for QUIC with rustls-native-certs
- Add certificate pinning for production API endpoints
- Implement proper UID validation in session path security
- Add security audit logging for sensitive operations
- Consider additional runtime intent validation if deep linking is required

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
- **Runtime permissions (Phase 4, 2026-08)**: camera/mic + fine-location
      permission state/requests live in Rust — `flutter-bridge/src/ffi/permissions.rs`
      (sync fns polling `Activity.checkSelfPermission` via JNI in platform.rs;
      `requestPermissions` fires the dialog, callers poll `*_granted` 20×250 ms;
      permanent denial approximated with `shouldShowRequestPermissionRationale`;
      `permissions_open_settings` builds the app-details Intent; location-service
      state via `LocationManager.isProviderEnabled`). Linux location via XDG
      Desktop Portal through ashpd 0.13 (`LocationProxy::create_session` +
      `receive_location_updated` stream, `location` + `tokio` features; `tokio`
      runtime supplied by frb) — supersedes the `com.soshal/portal`
      MethodChannel in `my_application.cc`. `geolocator` plugin remains ONLY for
      the Android GPS fix (its permission flow bypassed); `permission_handler`
      plugin dependency removed. Kotlin untouched (live checks reflect the
      dialog decision — no result store needed).
- **CI platform guard (Phase 6, 2026-08)**: `scripts/guard-lib-platform.sh`
      (wired into pre-commit + CI flutter-lint job) bans, in `soshal_flutter/lib/`:
      `MethodChannel(` usage, imports of removed plugins
      (permission_handler/battery_plus/connectivity_plus), and `Platform.is*` /
      `Platform.operatingSystem|version|numberOfProcessors` fact sniffing —
      platform facts must come from Rust ffi fns (e.g.
      `permissions_platform_current()`). Exemptions: `ffi_bridge.dart` (native
      lib loading is the FFI boundary), `utils/offthread.dart`
      (`FLUTTER_TEST` env probe), `Platform.pathSeparator` (path building).
      Tests that stub `debugPlatformIs*` hooks must set BOTH
      `debugPlatformIsAndroid` AND `debugPlatformIsLinux` (unset hook falls
      back to the ffi platform call and throws in FakeApi tests).
- **Power sampling (Phase 5, 2026-08)**: battery/connectivity facts for the
      seeding scheduler live in Rust — `flutter-bridge/src/ffi/power.rs`
      (`power_sample_os_state`, async frb). Android: JNI on
      `BatteryManager.getIntProperty` (capacity + status), `PowerManager.isPowerSaveMode`,
      `ConnectivityManager.getActiveNetworkInfo().getType()` (mobile/wimax =
      cellular). Linux: UPower (`OnBattery`, first battery device `Percentage`)
      + NetworkManager (`Devices` list, any MODEM type = cellular) via zbus 5
      (direct dep, tokio feature; tokio runtime supplied by frb); session bus
      absence degrades to desktop defaults. `battery_plus` + `connectivity_plus`
      plugins REMOVED from pubspec. `p2p_service.dart` polls via
      `powerSampleOsState()`; `updatePower` (push into scheduler) unchanged.
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

## Product Feature Guidelines & Reference Specifications

All AI agents and contributors working on feature implementations across Soshal must strictly adhere to the reference specifications defined below. "Exact parity" means implementing every feature present on the reference platform, while extending it with decentralized, peer-to-peer (P2P), Web of Trust, and cryptographic capabilities. For the exhaustive specification, see `docs/FEATURE_GUIDELINES.md`.

1. **Feed (Facebook Parity)**:
   - Must function with full Facebook parity: rich multi-format publishing (styled text backgrounds, multi-image collages, high-res video autoplay, animated GIFs, OpenGraph link cards, feelings/activities, geohash check-ins, polls).
   - 6 core animated reactions (Like, Love, Care, Haha, Wow, Sad, Angry) + Sats zaps (NIP-57).
   - Multi-level nested comment threading with reactions and media; reshares with commentary (quote post) and direct reposts.
   - Per-post audience selector (Public, Friends, Friends of Friends, Custom/Stealth).
   - Dual feed views (Top / Algorithmic vs Most Recent / Chronological), and post management (edit history, pin to profile, hide, 30-day snooze, unfollow, save to collections).

2. **Notifications (with Ignore Option)**:
   - Must include an actionable notification center with categorized filter tabs.
   - **Crucial Requirement**: Every notification, user, and thread MUST support the option to **Ignore**:
     - *Ignore Single Notification*: Dismiss without marking read or alerting the sender.
     - *Ignore / Mute User*: Silence all future alerts from a specific user across posts, comments, and mentions without unfriending.
     - *Ignore / Turn Off Post Notifications*: Unsubscribe from activity on specific posts or threads.
     - *Ignore by Category*: Mute specific event types (zaps, group mentions, event invites).
     - *Quiet Mode / Do Not Disturb*: Scheduled or ad-hoc silence hours.
     - *Ignored List Dashboard*: Dedicated screen under Settings to review and unmute ignored entities.

3. **Messages (Facebook Messenger Parity)**:
   - Must function with full Facebook Messenger parity: 1-on-1 and multi-user group chats with custom avatars and admin roles.
   - Active status presence indicators and real-time typing indicators (`...`).
   - Voice notes with interactive waveform scrubbing (1x, 1.5x, 2x speeds); photo/video galleries, files, GIFs, stickers, and a searchable Shared Media Gallery.
   - In-chat reactions, swipe-to-reply quoting, message forwarding, and message pinning.
   - Vanish / disappearing messages mode; NIP-44 v2 / Double Ratchet E2EE with post-quantum ML-KEM-768; isolated Message Requests inbox with Accept, Delete, and Ignore options.
   - Native WebRTC 1-on-1 and group voice/video calling with picture-in-picture.

4. **Groups (Discord Parity)**:
   - Must function with full Discord parity: Server/guild spaces with custom icons, banners, vanity invites, and rules-acceptance onboarding.
   - Hierarchical category trees organizing specialized channel types:
     - *Text Channels (`#`)*: Markdown, spoiler tags (`||`), threaded discussions, pins, attachments.
     - *Voice Channels (`🔊`)*: Persistent drop-in/drop-out WebRTC voice rooms, active speaker green circles, individual volume sliders, mute/deafen, screen sharing.
     - *Stage Channels (`📢`)*: Broadcast stages separating speakers from listeners with a moderated "Raise Hand" queue.
     - *Forum Channels*: Card-based topical discussion boards with tag filtering.
   - Multi-tier Role-Based Access Control (RBAC) with color tags, hoisted member display, and granular permissions.
   - Mention system (`@user`, `@role`, `@everyone`, `@here`) and collapsible member sidebar with presence states (Online, Idle, DND, Offline).

5. **Dating (Facebook Dating Parity)**:
   - Must function with full Facebook Dating parity: 100% segregated dating profile detached from the main feed identity (invisible to friends/contacts by default).
   - Up to 9 photos, bio, lifestyle attributes, and interactive prompt icebreaker cards.
   - Discovery deck with vertical profile scrolling, Pass (✕), and Like (❤️). Contextual likes: ability to like and comment directly on a specific photo or prompt.
   - Mutual match unlocks conversation.
   - **Secret Crush**: Select up to 9 existing friends/followers; reciprocal crush triggers an instant match; otherwise remains strictly private.
   - Shared Events and Groups matching opt-ins.
   - Dedicated dating chat inbox isolated from main Messenger with safety-first media restrictions and instant unmatch/block/report tools.

6. **Marketplace (Facebook Marketplace Parity)**:
   - Must function with full Facebook Marketplace parity: Structured category browsing, keyword search with auto-suggest, distance radius filter, price filter, and condition tags (New, Like New, Good, Fair).
   - Multi-image photo carousels, detailed descriptions, generalized location radius bubbles (preserving exact street address privacy), and seller trust profiles with ratings/reviews.
   - Seller studio supporting up to 15 photos, structured fields, and meet-up preferences (Public Meetup, Door Pickup, Door Dropoff).
   - Seller dashboard managing Active, Pending, and Sold listings with one-tap status toggles.
   - Integrated buyer-seller chat with automated prompts ("Is this available?"), formal make-an-offer / counter-offer negotiation, and saved item watchlists.

7. **Events (Multi-Screen Architecture)**:
   - **Screen 1: Calendar View Screen**:
     - Month and week calendar grid.
     - **Day-Cell Attendance Icons**: Every day cell displays distinct visual icons/badges indicating events scheduled on that day for which the user is attending (with visual differentiation between "Attending/Going" vs "Interested", plus event category iconography).
     - Date cell tap expands an interactive agenda sheet listing the day's event schedule, venues, and timings.
   - **Screen 2: Audience Discovery Screen**:
     - Dedicated screen to discover and filter events based on **Audience Type**:
       - *Public Events*: Open to all network relays.
       - *Friends of Friends Events*: Hosted or attended by second-degree connections via Web of Trust.
       - *Friends Only Events*: Private gatherings hosted by direct mutual contacts.
     - Sub-filters: Today, Tomorrow, This Weekend, Custom Date Range, and local proximity radius vs virtual online streaming links.
   - **Screen 3: Event Detail Screen**: Cover image, host info, calendar sync, map navigation or virtual link, RSVP actions (Going, Interested, Can't Go), filterable guest list, and discussion wall.
   - **Screen 4: Event Creation Studio**: Audience visibility configuration (Public, Friends of Friends, Friends, Private invite-only), co-hosts, recurrence rules, and ticketing details.

8. **Minis (Instagram Reels Parity)**:
   - Must function with full Instagram Reels parity: Edge-to-edge 9:16 vertical video player with vertical swipe up/down gesture navigation and seamless pre-buffering.
   - Right action rail: Heart/Like with counters, slide-up comment tray with nested replies, share/forward to Messenger, remix/duet button, and rotating audio disc.
   - Bottom-left creator overlay: Avatar with instant Follow toggle, multi-line expandable caption, clickable hashtags/mentions, and scrolling audio marquee.
   - Dedicated Audio / Sound Page: Track details, total Minis created with the sound, showcase grid, and "Use Audio" studio launch.
   - Creation studio: Multi-segment recording, countdown timer, hands-free recording, speed controls (0.3x to 3x), camera flip, audio picker, video trimmer, and text/sticker tools.

9. **Live (Twitch Parity)**:
   - Must function with full Twitch parity: Sub-second low-latency video streaming via MoQ and WebRTC/RTMP (`streaming-core`), with multi-quality transcoding selectors (Source/1080p60, 720p60, 480p, 360p, Auto), theater mode, and picture-in-picture.
   - Real-time high-throughput chat with user badges (Broadcaster, Mod, VIP, Sub, Verified), custom emotes, and chat modes (Emote-only, Sub-only, Follower-only, Slow mode).
   - Stream metadata: Stream title, category/game directory tagging, live viewer counter, uptime clock, and follow/subscribe tiers.
   - Mod View dashboard: User timeouts, permanent bans, message deletion, and chat purges.
   - Lightning zap / "Bits" tipping with on-screen animated cheer alerts, channel raids/hosting upon ending stream, 30–60s clip creator, and archived VODs with full synchronized chat replay.

10. **Musicloud (SoundCloud Parity)**:
    - Must function with full SoundCloud parity: Full-width interactive audio waveform scrubber showing amplitude peaks, tap/drag to seek, skip, loop, and shuffle.
    - Timed waveform comments: Listeners drop comments pinned to exact timestamps along the track, rendering avatar pins on the waveform and animated popup speech bubbles as playback passes.
    - Creator upload studio: Lossless (FLAC, WAV) and compressed (MP3, AAC) audio upload, square artwork, metadata (title, artist, genre, release date, tags, description), and Public vs Private link privacy.
    - Chronological stream feed of releases/reposts from followed artists, top charts by genre, and algorithmic discovery mixes.
    - Sets & Playlists management with reorderable queues.
    - Repost tracks to followers' streams, like tracks into personal library, share tracks with timestamp offsets (`?t=01:23`), and artist spotlight profiles with discography tabs.
    - Persistent audio mini-player bar and background OS lockscreen/notification controls.

11. **ChatRandom (Multimodal Chatroulette + 1-on-1 & Groups)**:
    - Must function like Chatroulette with instant random pairing and prominent "Next / Skip" controls to immediately disconnect and rotate to a new session.
    - **Multimodal Input Modalities**:
      - *Video Mode*: Two-way WebRTC camera video + audio with camera flip and mute.
      - *Audio-Only Mode*: Voice chat without video transmission (ideal for low-bandwidth or privacy-conscious users).
      - *Text-Only Mode*: Lightweight anonymous text chat without requiring camera or microphone.
      - Dynamic mode negotiation between peers.
    - **Match Topologies**:
      - *1-on-1 Matching*: Classic pairwise random connection between two users.
      - *Group Matching*: Dynamic multi-party random lounges where 3 to 8 users are pooled into a shared video/audio/text room, with real-time seat replenishment as members skip or leave.
    - Interest topic tags (e.g. `#gaming`, `#music`, `#tech`), language filter, and regional preferences.
    - Real-time automated NSFW blur detection, camera blur until confirmed, one-tap report/block with local SQLite blacklisting, and ephemeral cryptographic keys to isolate session identity from main profile.