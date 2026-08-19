plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
    // Python runtime for the Reticulum daemon (rnsd) — RNS is pure Python
    // with no official Android binary. Packaged per-ABI into the APK.
    id("com.chaquo.python")
}

android {
    namespace = "com.example.soshal_flutter"
    compileSdk = 37
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "com.example.soshal_flutter"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        // AAudio (libaaudio, API 26) is linked into the Rust bridge (.so)
        // for the H264/AAC codec layer, so devices older than Android 8.0
        // cannot load it.
        minSdk = 26
        targetSdk = flutter.targetSdkVersion
        // Matches the Rust bridge ABIs so Chaquopy packages libpython for
        // every APK variant (and only those).
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64", "armeabi-v7a")
        }
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

// Reticulum daemon (rnsd) Python stack. rnspure = rns without the hard
// pip deps (cryptography/pyserial); RNS loads them only when available and
// falls back to its built-in primitives otherwise — right for Android.
chaquopy {
    defaultConfig {
        pip {
            install("rnspure==1.4.2")
        }
    }
}
