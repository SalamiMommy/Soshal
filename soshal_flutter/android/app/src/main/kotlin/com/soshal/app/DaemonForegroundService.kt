package com.soshal.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Foreground service anchoring the bundled networking daemons (i2pd,
 * freenet, rnsd). The daemon processes are children of the app process
 * (spawned by the Rust bridge); this service keeps the process (and thus
 * the daemons) alive while the app is backgrounded.
 *
 * Uses the `specialUse` foreground-service type: `dataSync` carries a
 * 6-hour/day Android 15+ cap which would kill a 24/7 mesh node. No time
 * limit applies to `specialUse`; the Play Store would require a
 * justification, this app is sideloaded.
 *
 * Wired from Rust via JNI (`daemon_service_start` / `daemon_service_stop`
 * in flutter-bridge/src/platform.rs), mirroring the LiveRecorder pattern.
 */
object DaemonForegroundService {
    const val SERVICE_ACTION_START = "com.soshal.app.DAEMON_SERVICE_START"
    const val SERVICE_ACTION_STOP = "com.soshal.app.DAEMON_SERVICE_STOP"

    private const val NOTIFICATION_ID = 3001
    private const val CHANNEL_ID = "daemons"

    @Volatile
    var running: Boolean = false

    fun start(context: Context): Boolean {
        val intent = Intent(context, DaemonServiceInstance::class.java)
            .setAction(SERVICE_ACTION_START)
        context.startService(intent)
        running = true
        return true
    }

    fun stop(context: Context): Boolean {
        val intent = Intent(context, DaemonServiceInstance::class.java)
            .setAction(SERVICE_ACTION_STOP)
        context.stopService(intent)
        running = false
        return true
    }

    internal fun notification(context: Context): Notification {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Networking daemons",
                NotificationManager.IMPORTANCE_LOW,
            ).apply { description = "Keeps i2pd, freenet, and rnsd running" }
            manager.createNotificationChannel(channel)
        }
        val intent = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val pending = PendingIntent.getActivity(
            context,
            0,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("Soshal networking daemons")
            .setContentText("i2pd, freenet, rnsd running in background")
            .setContentIntent(pending)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder.setChannelId(CHANNEL_ID)
        }
        return builder.build()
    }

    internal fun startForegroundCompat(service: Service, notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            service.startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            service.startForeground(NOTIFICATION_ID, notification)
        }
    }
}

/**
 * Actual service instance. Foreground service types require the manifest
 * declaration on the concrete class; the object above only holds helpers.
 */
class DaemonServiceInstance : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == DaemonForegroundService.SERVICE_ACTION_STOP) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        DaemonForegroundService.running = true
        DaemonForegroundService.startForegroundCompat(
            this,
            DaemonForegroundService.notification(this),
        )
        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        DaemonForegroundService.running = false
    }
}
