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

# Chaquopy Python runtime + the rnsd (Reticulum) daemon it hosts. R8 must
# not strip the Python interpreter bridge or the app-embedded daemon service.
-keep class com.chaquo.python.** { *; }
-keep class python.** { *; }
-keep class org.python.** { *; }
-keep class com.soshal.app.RnsdRunner { *; }
# Chaquopy/RNS use reflection-heavy loading; silence missing-opt-in warnings
# rather than letting R8 warn-noop on optional Android APIs.
-dontwarn com.chaquo.python.**
-dontwarn org.python.**
