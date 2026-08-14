package com.example.soshal_flutter

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import java.io.File
import java.io.FileOutputStream

/**
 * Manager for bundled networking daemons (I2P, Freenet, Reticulum)
 * 
 * Daemons are bundled in assets/daemons/ and extracted to app data directory on first run.
 */
object DaemonManager {
    
    private const val DAEMONS_DIR = "daemons"
    private const val I2PD_BINARY = "i2pd"
    private const val FREENET_BINARY = "freenet"
    private const val RETICULUM_BINARY = "rnsd"
    
    /**
     * Extracts bundled daemons from assets to app data directory
     */
    fun extractDaemons(context: Context): Boolean {
        try {
            val daemonDir = File(context.filesDir, DAEMONS_DIR)
            if (!daemonDir.exists()) {
                daemonDir.mkdirs()
            }
            
            val binaries = listOf(I2PD_BINARY, FREENET_BINARY, RETICULUM_BINARY)
            
            for (binary in binaries) {
                val targetFile = File(daemonDir, binary)
                if (!targetFile.exists()) {
                    extractAsset(context, "daemons/$binary", targetFile)
                    targetFile.setExecutable(true)
                }
            }
            
            return true
        } catch (e: Exception) {
            e.printStackTrace()
            return false
        }
    }
    
    /**
     * Gets the path to a specific daemon binary
     */
    fun getDaemonPath(context: Context, daemonName: String): String? {
        val daemonDir = File(context.filesDir, DAEMONS_DIR)
        val daemonFile = File(daemonDir, daemonName)
        return if (daemonFile.exists()) {
            daemonFile.absolutePath
        } else {
            null
        }
    }
    
    /**
     * Checks if all daemons are available
     */
    fun areDaemonsAvailable(context: Context): Boolean {
        val daemonDir = File(context.filesDir, DAEMONS_DIR)
        return daemonDir.exists() && 
               File(daemonDir, I2PD_BINARY).exists() &&
               File(daemonDir, FREENET_BINARY).exists() &&
               File(daemonDir, RETICULUM_BINARY).exists()
    }
    
    /**
     * Gets daemon status information
     */
    fun getDaemonStatus(context: Context): Map<String, Boolean> {
        val daemonDir = File(context.filesDir, DAEMONS_DIR)
        return mapOf(
            "i2pd" to File(daemonDir, I2PD_BINARY).exists(),
            "freenet" to File(daemonDir, FREENET_BINARY).exists(),
            "reticulum" to File(daemonDir, RETICULUM_BINARY).exists()
        )
    }

    private var i2pdProcess: Process? = null
    private var rnsdProcess: Process? = null

    /**
     * Launches the bundled i2pd binary with SAM (7656), SOCKS (4447) and
     * HTTP console (4444) enabled, logging to filesDir/i2pd-data.
     */
    fun startI2pd(context: Context): Boolean {
        stopI2pd()
        val binary = File(context.filesDir, "$DAEMONS_DIR/$I2PD_BINARY")
        if (!binary.exists()) {
            return false
        }
        val dataDir = File(context.filesDir, "i2pd-data").apply { mkdirs() }
        val confFile = File(dataDir, "i2pd.conf")
        confFile.writeText(
            "[general]\n" +
                "log = file\n" +
                "logfile = ${File(dataDir, "i2pd.log").absolutePath}\n" +
                "[sam]\n" +
                "enabled = true\n" +
                "[proxy]\n" +
                "enabled = true\n" +
                "port = 4447\n" +
                "[http]\n" +
                "enabled = true\n" +
                "port = 4444\n"
        )
        i2pdProcess = try {
            ProcessBuilder(
                binary.absolutePath,
                "--datadir=${dataDir.absolutePath}",
                "--conf=${confFile.absolutePath}"
            )
                .redirectErrorStream(true)
                .redirectOutput(ProcessBuilder.Redirect.appendTo(File(dataDir, "i2pd.stdout.log")))
                .start()
        } catch (e: Exception) {
            e.printStackTrace()
            null
        }
        return i2pdProcess?.isAlive == true
    }

    /**
     * Stops the spawned i2pd process if running.
     */
    fun stopI2pd() {
        i2pdProcess?.destroy()
        i2pdProcess = null
    }

    /**
     * Process liveness check (not just binary existence).
     */
    fun isI2pdRunning(): Boolean = i2pdProcess?.isAlive == true

    /**
     * Launches the bundled Reticulum daemon (rnsd) for mesh networking.
     * Requires Python runtime on the device.
     */
    fun startRnsd(context: Context): Boolean {
        stopRnsd()
        val binary = File(context.filesDir, "$DAEMONS_DIR/$RETICULUM_BINARY")
        if (!binary.exists()) {
            return false
        }
        val dataDir = File(context.filesDir, "reticulum-data").apply { mkdirs() }
        rnsdProcess = try {
            ProcessBuilder(binary.absolutePath)
                .directory(dataDir)
                .redirectErrorStream(true)
                .redirectOutput(ProcessBuilder.Redirect.appendTo(File(dataDir, "rnsd.log")))
                .start()
        } catch (e: Exception) {
            e.printStackTrace()
            null
        }
        return rnsdProcess?.isAlive == true
    }

    /**
     * Stops the spawned Reticulum daemon if running.
     */
    fun stopRnsd() {
        rnsdProcess?.destroy()
        rnsdProcess = null
    }

    /**
     * Check if Reticulum daemon is running.
     */
    fun isRnsdRunning(): Boolean = rnsdProcess?.isAlive == true
    
    private fun extractAsset(context: Context, assetPath: String, targetFile: File) {
        context.assets.open(assetPath).use { inputStream ->
            FileOutputStream(targetFile).use { outputStream ->
                inputStream.copyTo(outputStream)
            }
        }
    }
}

/**
 * Foreground service for running networking daemons (I2P, Freenet, Reticulum)
 * 
 * This service runs as a foreground service to ensure daemons aren't killed by the OS.
 * The actual daemon binaries are managed by DaemonManager.
 */
class DaemonService : Service() {
    
    private val CHANNEL_ID = "daemon_service_channel"
    private val NOTIFICATION_ID = 1001
    
    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }
    
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = createNotification()
        startForeground(NOTIFICATION_ID, notification)
        
        // Extract daemons on first start
        DaemonManager.extractDaemons(this)

        // Launch the i2pd daemon so SAM/SOCKS are reachable for the app.
        DaemonManager.startI2pd(this)
        
        // Launch the Reticulum daemon for mesh networking
        DaemonManager.startRnsd(this)

        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        DaemonManager.stopI2pd()
        DaemonManager.stopRnsd()
    }
    
    override fun onBind(intent: Intent?): IBinder? {
        return null
    }
    
    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Networking Daemons",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Background service for networking daemons"
        }
        
        val notificationManager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        notificationManager.createNotificationChannel(channel)
    }
    
    private fun createNotification(): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Soshal Networking")
            .setContentText("Running networking daemons")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }
}
