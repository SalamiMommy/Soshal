# ProGuard / R8 code shrinking rules for Soshal Flutter App & Rust FFI Bridge

# Keep Flutter & flutter_rust_bridge generated classes and native bindings
-keep class com.soshal.app.** { *; }
-keepclassmembers class * {
    native <methods>;
}

# Keep JNI helper bindings
-keepclasseswithmembernames class * {
    native <methods>;
}

# Preserve RustLib and FRB entry points
-keep class FlutterRustBridge** { *; }
