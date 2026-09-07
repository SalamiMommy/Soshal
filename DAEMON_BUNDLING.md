# Networking Daemons Bundling for Android APK

This document describes how networking daemons (I2P, Freenet, Reticulum) are bundled into the Soshal Android APK.

## Overview

The Soshal app includes three networking daemons that provide compatibility with different decentralized networks:

1. **I2P (i2pd)** - Invisible Internet Project daemon for anonymous networking
2. **Freenet** - Distributed data store daemon 
3. **Reticulum (rnsd)** - Mesh networking daemon

## Architecture

### Build Process

The Android build script (`builds/android/build.sh`) has been enhanced to:

1. **Download and prepare daemon binaries** for Android ARM64 architecture
2. **Bundle daemons into APK assets** under `assets/daemons/`
3. **Verify daemon inclusion** in the final APK

### Runtime Management

The Android app includes:

1. **DaemonManager** - Kotlin object that extracts daemons from assets to app data directory
2. **DaemonService** - Android foreground service that keeps daemons running
3. **DaemonService (Dart)** - Flutter service that provides Dart interface to daemon management

### Flutter Integration

The Dart layer provides:

- `DaemonService` class with methods for daemon management
- Method channel communication with native Android code
- Daemon status monitoring and path resolution

## Files Modified

### Build System
- `builds/android/build.sh` - Enhanced to download and bundle daemons

### Android Native Code
- `android/app/src/main/kotlin/com/soshal/app/DaemonService.kt` - Daemon manager and service
- `android/app/src/main/kotlin/com/soshal/app/MainActivity.kt` - Method channel setup
- `android/app/src/main/AndroidManifest.xml` - Permissions and service registration

### Flutter Code
- `lib/services/daemon_service.dart` - Dart interface for daemon management

## Building the APK

### Prerequisites

Ensure you have:
- Android SDK with NDK 27.1.12297006
- Flutter (via fvm or PATH)
- Rust toolchain
- Network access for downloading daemon binaries

### Build Commands

```bash
# Debug APK with bundled daemons
./builds/android/build.sh

# Release APK with bundled daemons
./builds/android/build.sh --release
```

### Build Process Details

1. **Daemon Download**: The build script downloads daemon binaries from official repositories:
   - I2P: https://github.com/PurpleI2P/i2pd/releases
   - Freenet: https://github.com/freenet/freenet-core/releases
   - Reticulum: Bundled as the Chaquopy `rnspure` Python package (runs in-process)

2. **Asset Bundling**: Daemons are copied to `assets/daemons/` in the APK

3. **Bridge Compilation**: Rust bridge is compiled for all Android ABIs (arm64-v8a, x86_64, armeabi-v7a)

4. **APK Assembly**: Flutter builds the final APK with all components

## Runtime Behavior

### First Launch

1. **Daemon Extraction**: Daemons are extracted from assets to app data directory
2. **Permission Setup**: App requests necessary permissions (foreground service, network, etc.)
3. **Service Start**: DaemonService starts as foreground service
4. **Path Resolution**: App resolves daemon binary paths for FFI calls

### Subsequent Launches

1. **Service Start**: DaemonService starts automatically
2. **Path Resolution**: Daemon paths are resolved from existing extraction
3. **Network Integration**: Daemons are available for networking operations

## Daemon Status

### I2P (i2pd)
- **Status**: Binary download implemented
- **Notes**: Full Android ARM64 binary available
- **Integration**: SAM V3 client connects to local i2pd instance

### Freenet
- **Status**: Binary download implemented  
- **Notes**: Android ARM64 binary available from official releases
- **Integration**: WebSocket client connects to local Freenet node

### Reticulum (rnsd)
- **Status**: Complete — in-process Chaquopy Python daemon
- **Notes**: No Android binary exists; RNS is pure Python. Bundled via the
  Chaquopy plugin (`rnspure==1.5.2` pip dep in `build.gradle.kts`), running
  inside the app process through `RnsdRunner` (Kotlin) → `rnsd_service.py`
  (Python) → `RNS.Reticulum`. Rust bridges it via `rnsd_start`/`rnsd_stop`/
  `rnsd_running` in `flutter-bridge/src/platform.rs`.
- **Integration**: RNS node runs in-process; the native Rust Reticulum stack
  (`soshal-network-core::reticulum`) backs the relay/P2P transport.

## Limitations

1. **Reticulum on Android** runs the reference Python RNS node (rnspure via
   Chaquopy, in-process). It uses pure-Python crypto primitives (no
   PyCA/pyserial on Android), which is slower than the OpenSSL backend — the
   native Rust Reticulum stack doubles as the fast path for mesh transport.

2. **Daemon Updates**: Daemon binaries are downloaded during build; updates require rebuilding the APK.

3. **Architecture Support**: Currently optimized for ARM64; other architectures may need different binaries.

## Future Enhancements

1. **Daemon Updates**: Implement in-app daemon binary updates

2. **Multi-architecture**: Support additional Android architectures (x86, ARMv7)

3. **Daemon Health Monitoring**: Add health checks and automatic restart for
   daemons (rnsd liveness is reported via `daemon_get_daemon_status` and
   `daemon_is_rnsd_running`).

## Troubleshooting

### Daemons Not Found in APK

If daemons are missing from the APK:
1. Check network connectivity during build (for downloads)
2. Verify daemon URLs in build script are correct
3. Check target directory permissions

### Daemon Execution Failures

If daemons fail to execute:
1. Verify executable permissions on extracted binaries
2. Check Android logcat for execution errors
3. Ensure app has necessary permissions

### Build Failures

If build fails:
1. Verify NDK version matches script expectations
2. Check Flutter installation
3. Ensure sufficient disk space for build artifacts

## Security Considerations

1. **Binary Verification**: Daemon binaries should be verified with checksums
2. **Network Security**: Daemon downloads use HTTPS from official repositories
3. **Permissions**: App requests minimal necessary permissions
4. **Sandboxing**: Daemons run within app sandbox with limited privileges

## Performance Impact

1. **APK Size**: Daemon binaries add ~10-20MB to APK size
2. **Memory Usage**: Running daemons consume additional memory
3. **Battery**: Foreground service impacts battery life
4. **Network**: Daemons maintain network connections in background

## Compliance

- **Google Play**: Foreground service notification required for compliance
- **Permissions**: All permissions are justified and documented
- **Privacy**: Daemons process data locally; no external data collection
