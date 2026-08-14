package com.example.soshal_flutter

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private val DAEMON_CHANNEL = "com.example.soshal/daemons"
    private val H264_CHANNEL = "com.soshal/h264"
    private val AUDIO_CHANNEL = "com.soshal/audio"

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        LiveRecorder.init(applicationContext)

        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            H264_CHANNEL,
        ).setMethodCallHandler(H264Codec())
        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            AUDIO_CHANNEL,
        ).setMethodCallHandler(AudioCodec())

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, DAEMON_CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "extractDaemons" -> {
                    val success = DaemonManager.extractDaemons(this)
                    result.success(success)
                }
                "getDaemonPath" -> {
                    val daemonName = call.argument<String>("daemonName")
                    if (daemonName != null) {
                        val path = DaemonManager.getDaemonPath(this, daemonName)
                        result.success(path)
                    } else {
                        result.error("INVALID_ARGUMENT", "daemonName is required", null)
                    }
                }
                "areDaemonsAvailable" -> {
                    val available = DaemonManager.areDaemonsAvailable(this)
                    result.success(available)
                }
                "getDaemonStatus" -> {
                    val status = DaemonManager.getDaemonStatus(this)
                    result.success(status)
                }
                "startDaemons" -> {
                    val extracted = DaemonManager.extractDaemons(this)
                    val started = DaemonManager.startI2pd(this) && DaemonManager.startRnsd(this)
                    result.success(extracted && started)
                }
                "stopDaemons" -> {
                    DaemonManager.stopI2pd()
                    DaemonManager.stopRnsd()
                    result.success(true)
                }
                "isI2pdRunning" -> {
                    result.success(DaemonManager.isI2pdRunning())
                }
                "isRnsdRunning" -> {
                    result.success(DaemonManager.isRnsdRunning())
                }
                else -> {
                    result.notImplemented()
                }
            }
        }
    }
}
