package com.soshal.app

/**
 * Static platform handles for the Rust bridge (jni). Holds only the
 * Activity reference; all logic lives in Rust (flutter-bridge/platform.rs).
 */
object PlatformBridge {
    @Volatile
    var activity: MainActivity? = null
        set(value) {
            field = value
            if (value != null) {
                try {
                    nativeInit(value, value.applicationContext)
                } catch (_: Throwable) {}
            }
        }

    @JvmStatic
    external fun nativeInit(activity: Any, context: Any)
}
