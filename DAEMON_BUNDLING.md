# Networking Daemons Bundling for Android APK

This document describes how the networking daemons (I2P, Freenet, Reticulum)
are bundled into the Soshal Android APK and managed at runtime.

## Overview

Soshal includes three networking daemons that provide compatibility with
different decentralized networks:

1. **I2P (i2pd)** — Invisible Internet Project daemon for anonymous networking
2. **Freenet** — distributed data store daemon; no official Android release binary,
   so the build cross-compiles the Rust `freenet-core` node for arm64 and
   degrades to an honest error stub when that cross-build fails
3. **Reticulum (rnsd)** — Mesh networking daemon

Lifecycle is owned by Rust (`flutter-bridge/src/ffi/daemon.rs`): binary
extraction, config writing, spawn, and a respawn watchdog all live in the
bridge. Kotlin only provides the process anchor and the platform handles the
Rust code needs.

## Bundling (build time)

`builds/android/build.sh` builds and bundles daemons per ABI:

- **i2pd**: real Android binaries from the official
  `PurpleI2P/i2pd-android` release zip (`I2PD_VERSION`, default 2.50.0) are
  extracted into a cache, then copied to
  `assets/daemons/<abi>/i2pd` **and** `jniLibs/<abi>/libi2pd.so` per ABI
  (arm64-v8a / x86_64 / armeabi-v7a). The Rust bridge resolves the
  ABI-matching path via `Build.SUPPORTED_ABIS[0]`.
- **Freenet**: no official Android binary (the legacy fred/JVM stack cannot
  run on Android; freenet-core publishes no android release assets), so
  `build.sh` **cross-builds freenet-core's `freenet` node** for arm64 with
  the NDK (`FREENET_VERSION`, default 0.2.106; pinned rust-toolchain 1.94.0;
  first build ~30 min, cached). The result ships as `assets/daemons/freenet`
  and `jniLibs/arm64-v8a/libfreenet.so`. Any failure (clone/build error)
  degrades to an honest error **stub** — anything under 100 KB is recognized
  as a stub and never treated as a real daemon.
- **Reticulum (rnsd)**: no Android binary exists at all — RNS is pure Python.
  It runs **in-process** via the Chaquopy Python runtime (`rnspure==1.5.2`
  pip dependency in `android/app/build.gradle.kts`), so nothing is bundled as
  an asset.

### Binary resolution (runtime)

`daemon.rs::find_daemon_binary` resolves an executable in order:

1. Android `nativeLibraryDir` → `lib<name>.so` (jniLibs, extracted by
   PackageManager — SELinux permits execve there on Android 10+).
2. Extracted `<files>/daemons/<name>` (from `daemon_extract_daemons`).
3. AppImage bundle `daemons/` and `target/release/` (desktop dev builds).
4. System `PATH` (desktop).

Every candidate must be a real binary (>100 KB) — stubs never execute and
surface as "not available" in `daemon_get_daemon_status`.

## Runtime Management

### Rust side — `flutter-bridge/src/ffi/daemon.rs`

The bridge owns the full lifecycle:

- `daemon_extract_daemons` / `daemon_are_daemons_available` /
  `daemon_get_daemon_path` / `daemon_get_daemon_status` — extraction state.
- `daemon_start_daemons` — extracts (if needed), spawns all three via
  independent spawners, then starts a **watchdog thread** that re-spawns a
  dead daemon on a bounded schedule (consecutive-failure cap before giving
  up). Reuses an already-running external daemon when its port is live
  (SAM 7656 / Freenet API 7509 / rnsd 4242) instead of double-spawning.
- `daemon_stop_daemons` — stops spawned children and the watchdog.

Android runtime gotchas handled here (2026-09-10):

- **freenet** — Android app processes have no `$HOME`/XDG dirs and no passwd
  entry for the app UID, so freenet's default `ProjectDirs` resolution aborts
  the node before it binds the WS API. daemon.rs spawns
  `freenet network --config-dir=<files>/freenet-data/conf
  --data-dir=<files>/freenet-data/data --disable-auto-update`, with
  `TMPDIR`/`TMP`/`TEMP` pointed at an app-writable dir (the default
  `/data/local/tmp` is not writable by untrusted apps). Gateways auto-fetch
  from the remote index; auto-update is OFF because there is no supervisor on
  Android to act on freenet's exit-42 self-update signal.
- **i2pd** — defaults to `daemon = true` (forks to background), which makes
  the Rust `Child` parent exit, tripping the watchdog into a respawn loop of
  bind conflicts on 7656. daemon.rs writes a `daemon = false` conf into
  `<files>/i2pd-data/i2pd.conf` plus SAM (7656), SOCKS (4447), and HTTP
  (4444) proxy sections, then spawns with `--datadir` + `--conf`.
- **rnsd** — runs in-process (`platform::rnsd_start`, Chaquopy thread,
  `RnsdRunner.kt`). RNS never clears its process-wide singleton, so a failed
  init cannot be retried without a process restart; the watchdog would
  otherwise stack threads and hit "Attempt to reinitialise Reticulum".
  `RnsdRunner` is one-attempt latched (reset on `stop()`), and
  `platform::rnsd_status()` surfaces the last-start error through the
  `reticulum_status` fragment of `daemon_get_daemon_status`. The watchdog
  checks `platform::rnsd_running()` for liveness instead of respawning.

### Kotlin side (`android/app/src/main/kotlin/com/soshal/app/`)

- **`MainActivity.kt`** — plain `FlutterActivity`. Initializes
  `PlatformBridge.activity` and `LiveRecorder.init(applicationContext)`. No
  method channels.
- **`PlatformBridge.kt`** — static platform handles for the Rust bridge
  (JNI). Holds the `Activity`/`Context` reference and calls
  `nativeInit(activity, context)`; all logic lives in `flutter-bridge/src/platform.rs`.
- **`DaemonForegroundService.kt`** — foreground service anchoring the
  daemon processes (children of the app process) while the app is
  backgrounded; started from Rust via `platform::daemon_service_start()`
  (no-op off-Android). Uses an allowed foreground-service type for
  networking/data-sync.
- **`RnsdRunner.kt`** — in-process rnsd via Chaquopy (`rnspure`), wired from
  Rust via JNI `rnsd_start` / `rnsd_stop` / `rnsd_running`. Mirrors the
  `LiveRecorder` pattern.
- **`LiveRecorder.kt`** — on-device MP4 recording (not a daemon; included
  for completeness).

### Flutter side

- `lib/services/daemon_service.dart` — Dart interface: per-daemon
  extract/start/stop/status via the `daemon_*` FFI fns (generated
  `crateFfiDaemon*`).
- Daemon logs are readable **in-app**: Network settings → Bundled Daemons →
  Logs reads `<files>/i2pd-data/i2pd.log`, `<files>/freenet-data/freenet.log`,
  `<files>/reticulum-data/logfile` via `path_provider` — no extra FFI.

## Building the APK

### Prerequisites

- Android SDK with NDK 27.1.12297006
- Flutter (via fvm or PATH)
- Rust toolchain
- Network access for downloading i2pd binaries + opus source

### Build Commands

```bash
# Debug APK with bundled daemons
./builds/android/build.sh

# Release APK with bundled daemons
./builds/android/build.sh --release
```

### Build Process Details

1. **Daemon Download**: i2pd ARM64 binaries are downloaded + verified from
   official GitHub releases; freenet gets an error stub; rnsd needs no binary
   (Chaquopy).
2. **Asset Bundling**: real daemon binaries (≥100 KB) are copied to
   `assets/daemons/` in the APK; stubs are never bundled as real daemons.
3. **Bridge Compilation**: Rust bridge is compiled for all Android ABIs
   (arm64-v8a, x86_64, armeabi-v7a) — see `AGENTS.md` for the ABI/NDK recipe.
4. **APK Assembly**: Flutter builds the final APK with all components.

## Runtime Behavior

### First Launch

1. **Daemon Extraction**: daemons are extracted from assets to the app data
   directory (<files>/daemons) on demand.
2. **Config Write**: i2pd.conf (with `daemon = false`) is written per-daemon.
3. **Spawn**: `daemon_start_daemons` spawns each daemon independently
   (reusing an externally-running instance when its port is live); the
   watchdog thread starts.
4. **Service Start**: the DaemonForegroundService is anchored from Rust.

### Subsequent Launches

1. **Watchdog**: re-spawns dead daemons, bounded per-daemon by a consecutive
   failure cap after which it gives up (status reflects it).
2. **Status**: `daemon_get_daemon_status` returns per-daemon JSON
   (`i2pd`, `freenet`, `reticulum` = rnsd), including rnsd's last-start error.

## Daemon Status

### I2P (i2pd)
- **Status**: bundled per-ABI (Android ARM64 binary)
- **Integration**: SAM V3 client (7656) + SOCKS (4447) + HTTP (4444) proxies
- **Notes**: `daemon = false` written into the generated conf; reuses an
  already-running system i2pd when 7656 is live.

### Freenet
- **Status**: NDK cross-build of freenet-core's `freenet` node (arm64) via
  `builds/android/build.sh`; ships as `assets/daemons/freenet` +
  `jniLibs/arm64-v8a/libfreenet.so`. If the clone/cross-build fails the
  build lands an error stub (>100 KB gate → never executed).
- **Integration**: WebSocket client connects to the local node (API port 7509);
  daemon.rs pins `--config-dir`/`--data-dir` (no ProjectDirs on Android) and
  disables auto-update.
- **Notes**: on desktop where a freenet binary is present (PATH/target
  release) it runs normally.

### Reticulum (rnsd)
- **Status**: Complete — in-process Chaquopy Python daemon (`rnspure==1.5.2`
  pip dep in `build.gradle.kts`) via `RnsdRunner` (Kotlin) → `rnsd_service.py`
  (Python) → `RNS.Reticulum`. Rust bridges it via `rnsd_start`/`rnsd_stop`/
  `rnsd_running` in `flutter-bridge/src/platform.rs`. One-attempt latched.
- **Integration**: RNS node runs in-process; the native Rust Reticulum stack
  (`soshal-network-core::reticulum`, via mesh-core) backs the relay/P2P
  transport as the fast path.

## Limitations

1. **Reticulum on Android** runs the reference Python RNS node (rnspure via
   Chaquopy, in-process). It uses pure-Python crypto primitives (no
   PyCA/pyserial on Android), which is slower than the OpenSSL backend — the
   native Rust Reticulum stack doubles as the fast path for mesh transport.
2. **Freenet on Android** relies on the arm64 cross-build of freenet-core
   shipping in the APK; if the build script cannot clone/cross-build it,
   only an error stub is bundled and the spawn fails with a clear status.
3. **Daemon Updates**: i2pd binaries are downloaded at build time; updates
   require rebuilding the APK.
4. **rnsd restart**: RNS's process-wide singleton means a failed init cannot
   be retried without a process restart — `RnsdRunner` is one-attempt latched
   (reset on `stop()`).

## Future Enhancements

1. **Daemon Updates**: implement in-app daemon binary updates.
2. **Multi-architecture**: verify bundled i2pd across x86/ARMv7.
3. **Freenet on Android**: ship a real freenet binary when an Android build
   becomes available.

## Troubleshooting

### Daemons Not Found in APK

1. Check network connectivity during build (for i2pd downloads).
2. Verify daemon URLs in `builds/android/build.sh` are correct.
3. Check target directory permissions.

### Daemon Execution Failures

1. Verify executable permissions on extracted binaries.
2. Check Android logcat + the in-app daemon log viewer (Network settings →
   Bundled Daemons → Logs).
3. Check `daemon_get_daemon_status` JSON — it reports per-daemon
   extraction/spawn state + the rnsd last-start error.

### Build Failures

1. Verify NDK version matches script expectations.
2. Check Flutter installation.
3. Ensure sufficient disk space for build artifacts.

## Security Considerations

1. **Binary Verification**: i2pd binaries are SHA-verified after download.
2. **Network Security**: daemon downloads use HTTPS from official
   repositories.
3. **Permissions**: app requests minimal necessary permissions.
4. **Sandboxing**: daemons run within the app sandbox with limited
   privileges; the foreground service uses an allowed service type.

## Performance Impact

1. **APK Size**: i2pd adds ~10-20MB to APK size.
2. **Memory Usage**: running daemons consume additional memory (rnsd runs
   inside the app process).
3. **Battery**: the foreground service impacts battery life while daemons run.
4. **Network**: daemons maintain network connections in background.

## Compliance

- **Google Play**: foreground service notification required for compliance.
- **Permissions**: all permissions are justified and documented.
- **Privacy**: daemons process data locally; no external data collection.