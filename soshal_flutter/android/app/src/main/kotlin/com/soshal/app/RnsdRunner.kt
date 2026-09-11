package com.soshal.app

import android.content.Context
import com.chaquo.python.Python
import com.chaquo.python.android.AndroidPlatform
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

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

    @Volatile
    private var lastError: String? = null

    @Volatile
    private var attemptMade = false

    private var thread: Thread? = null
    private var py: Python? = null

    @Synchronized
    fun start(context: Context, configDir: String): Boolean {
        if (running) return true
        // One launch attempt per enable: RNS never clears its process-wide
        // Reticulum singleton, so a failed init can't be retried without a
        // process restart, and the daemon watchdog would otherwise stack a
        // new thread (and hit "Attempt to reinitialise Reticulum") every
        // sweep. stop() resets the latch for a manual retry.
        if (attemptMade) return false
        attemptMade = true
        lastError = null
        return try {
            if (py == null) {
                Python.start(AndroidPlatform(context))
                py = Python.getInstance()
            }
            val initLatch = CountDownLatch(1)
            thread = Thread {
                // Bring up RNS on this thread. The result is authoritative:
                // `running` stays false when RNS init fails so the app shows
                // the honest state (see lastError / status()) instead of
                // claiming a daemon that never started.
                var started = false
                try {
                    val mod = py!!.getModule("rnsd_service")
                    val res = mod.callAttr("start", configDir)
                    started = res?.toBoolean() ?: false
                    if (!started && lastError == null) {
                        try {
                            lastError = mod.callAttr("status")?.get("error")?.toString()
                                ?: "rnsd start returned false"
                        } catch (_: Exception) {}
                    }
                } catch (e: Exception) {
                    lastError = e.toString()
                } finally {
                    running = started
                    initLatch.countDown()
                }

                // Keep the RNS transport threads alive by pumping the loop;
                // RNS.Reticulum() starts its own worker threads, the pump
                // only exits when stop() is called.
                if (running) {
                    try {
                        val mod = py!!.getModule("rnsd_service")
                        mod.callAttr("_pump")
                    } catch (e: Exception) {
                        // interpreter torn down
                    } finally {
                        running = false
                    }
                }
            }.apply {
                name = "rnsd-runner"
                isDaemon = true
                start()
            }
            initLatch.await(3, TimeUnit.SECONDS)
            running
        } catch (e: Exception) {
            lastError = e.toString()
            running = false
            false
        }
    }

    @Synchronized
    fun stop(context: Context): Boolean {
        running = false
        attemptMade = false
        lastError = null
        try {
            py?.getModule("rnsd_service")?.callAttr("stop")
        } catch (e: Exception) {
            // interpreter not started — nothing to stop
        }
        return true
    }

    fun isRunning(): Boolean = running

    fun status(): String =
        (lastError ?: "").let { err ->
            val escaped = err.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", " ")
            "\"running\":" + running + (if (escaped.isEmpty()) "" else ",\"error\":\"$escaped\"")
        }
}
