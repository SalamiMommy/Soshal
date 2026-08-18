package com.example.soshal_flutter

/**
 * Static platform handles for the Rust bridge (jni). Holds only the
 * Activity reference; all logic lives in Rust (flutter-bridge/platform.rs).
 */
object PlatformBridge {
    @Volatile
    var activity: MainActivity? = null
}
