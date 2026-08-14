// Flutter Native Assets API Hook for automated Rust crate compilation.
// Automatically invokes cargo build for target ABI/OS during `flutter build` / `flutter run`.

import 'dart:io';

// hooks is declared in dev_dependencies; the analyzer mis-resolves the
// package root for hook/ scripts (known false positive).
// ignore: depend_on_referenced_packages
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    // The Rust bridge .so is prebuilt per ABI by builds/android/build.sh
    // (cargo + NDK) and dropped into android/app/src/main/jniLibs, which
    // gradle packages directly. This hook emits no assets; it registers the
    // .so files as dependencies so the hook cache invalidates on rebuild.
    final jniRoot = input.packageRoot.resolve('android/app/src/main/jniLibs');
    if (Directory.fromUri(jniRoot).existsSync()) {
      for (final abiDir in Directory.fromUri(jniRoot).listSync()) {
        if (abiDir is! Directory) continue;
        for (final entry in abiDir.listSync()) {
          if (entry is File && entry.path.endsWith('.so')) {
            output.dependencies.add(entry.uri);
          }
        }
      }
    }
  });
}
