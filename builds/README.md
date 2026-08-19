# Soshal Build Scripts

One command each. The final artifact lands next to its script.

| Run | Output |
|-----|--------|
| `./builds/android/build.sh [--release]` | `builds/android/soshal_flutter[. -release].apk` |
| `./builds/linux/build.sh` | `builds/linux/soshal_flutter-linux-x64.AppImage` (dev AppImage) |

That's it. Each script builds the Rust bridge for the target, wires it into
the Flutter build, runs `flutter build`, copies the final artifact into its
own folder (Android), and verifies the bridge is inside it (prints the
result). The Linux script is dev-mode only and prints everything to the
terminal so compile errors surface immediately.

Notes:

- Run from anywhere — scripts resolve the repo root themselves.
- Android accepts `--release`; the Linux script has no options.
- Cached bridge `.so`s live under `$HOME/.cache/soshal-targets/` (override
  with `SOSHAL_TARGET_DIR`) — they speed up reruns and are rebuilt
  automatically when sources change. Forget they exist.
- Flutter is taken from `PATH`, falls back to `~/fvm/default/bin/flutter`; set
  `FLUTTER_BIN` only if neither exists.
- Android needs the NDK; defaults: `ANDROID_HOME=$HOME/Android/Sdk`,
  `NDK_VERSION=27.1.12297006` (overridable via env vars if your SDK lives
  elsewhere). The script creates patched bare `<triple>-clang` aliases
  (NDK 27 ships only versioned wrappers, and ring/aws-lc-sys/secp256k1-sys
  probe the bare names) plus a static per-ABI libopus for audiopus_sys —
  both cached under `SOSHAL_TARGET_DIR`.
- Linux dev AppImage: run `builds/linux/soshal_flutter-linux-x64.AppImage` (add
  `--appimage-extract-and-run` if FUSE is unavailable); the bridge `.so` is
  verified inside it after bundling.