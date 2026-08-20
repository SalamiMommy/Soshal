package com.soshal.app

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        PlatformBridge.activity = this
        LiveRecorder.init(applicationContext)
    }

    override fun onDestroy() {
        super.onDestroy()
        PlatformBridge.activity = null
    }
}
