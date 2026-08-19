package com.example.soshal_flutter

import android.content.Context
import com.chaquo.python.Python
import com.chaquo.python.android.AndroidPlatform

/**
 * Runs the reference Reticulum daemon (rnsd) via the Chaquopy Python
 * runtime — RNS is pure Python, so no separate binary exists for Android.
 * Runs on a daemon thread inside the app process; the foreground service
 * keeps the process alive while backgrounded.
 *
 * Wired from Rust via JNI (`rnsd_start` / `rnsd_stop` / `rnsd_running` in
 * flutter-bridge/src/platform.rs), mirroring the LiveRecorder pattern.
 */
object RnsdRunner {

    @Volatile
    var running: Boolean = false
        private set

    private var thread: Thread? = null
    private var py: Python? = null

    @Synchronized
    fun start(context: Context, configDir: String): Boolean {
        if (running) return true
        try {
            if (py == null) {
                Python.start(AndroidPlatform(context))
                py = Python.getInstance()
            }
            thread = Thread {
                try {
                    val mod = py!!.getModule("rnsd_service")
                    mod.callAttr("start", configDir)
                } catch (e: Exception) {
                    // Python init failed; report via running flag
                }
                // Keep the RNS transport threads alive by pumping the loop;
                // RNS.Reticulum() starts its own worker threads, the pump
                // only exits when stop() is called.
                try {
                    val mod = py!!.getModule("rnsd_service")
                    while (running) {
                        mod.callAttr("_pump")
                    }
                } catch (e: Exception) {
                    // interpreter torn down
                }
            }.apply {
                isDaemon = true
                start()
            }
            running = true
            return true
        } catch (e: Exception) {
            running = false
            return false
        }
    }

    @Synchronized
    fun stop(context: Context): Boolean {
        running = false
        try {
            py?.getModule("rnsd_service")?.callAttr("stop")
        } catch (e: Exception) {
            // interpreter not started — nothing to stop
        }
        return true
    }

    fun isRunning(): Boolean = running
}
