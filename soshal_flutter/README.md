# Soshal Flutter

Native Flutter client for Soshal, backed by the Rust workspace through a
direct `flutter_rust_bridge` FFI bridge. This is the only client UI.

## Architecture

- **Rust backend**: 27 `*-core` crates, pure logic, no platform deps
- **FFI adapter**: `flutter-bridge` (30 modules) delegates to cores; generated
  bindings in `lib/frb_generated.dart`
- **Flutter frontend**: Material 3 UI, go_router navigation, Provider
  (ChangeNotifier services); screens never call the bridge directly — only
  services do
- **Key material**: nsec enters only at signer init; OS keychain ops exchange
  pubkeys, never key bytes

## Project Structure

```
soshal_flutter/
├── lib/
│   ├── main.dart           # App root, provider registration
│   ├── frb_generated.dart    # Generated FFI bindings (do not hand-edit)
│   ├── services/             # 16 ChangeNotifier services (auth, feed, session, …)
│   ├── screens/              # ~24 screens
│   ├── routes/               # go_router configuration (~30 routes)
│   └── widgets/              # Shared widgets
├── android/app/src/main/jniLibs/  # libsoshal_flutter_bridge.so (3 ABIs)
└── pubspec.yaml
```

## Development

### Prerequisites

- Flutter SDK 3.47+ (fvm: `export PATH="$HOME/fvm/default/bin:$PATH"`)
- Rust toolchain + NDK 27.1.12297006 for Android
- flutter_rust_bridge_codegen 2.12.0 (only when FFI signatures change)

### Building

```bash
./builds/android/build.sh            # 3-ABI bridge + debug APK (add --release)
./builds/linux/build.sh              # host bridge + Linux bundle (add --release)
flutter analyze                      # lint gate: 0 errors / 0 warnings
```

## Status

- FFI surface fully wired: every real Rust function has a UI path; no callable
  stale codegen artifacts (network relay/publish fns exist only as leftover
  wire stubs in `frb_generated.rs` — never call them).
- Backend-gated surfaces (stubs, honest UI): NWC zap invoice fetch, push
  notifications, friend requests/suggestions, minis, WebRTC voice/video.
- Legacy Tauri 2.0 + Dioxus WASM client deleted (2026-08); Flutter is the only
  client.