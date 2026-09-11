plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
    // Python runtime for the Reticulum daemon (rnsd) — RNS is pure Python
    // with no official Android binary. Packaged per-ABI into the APK.
    id("com.chaquo.python") version "17.0.0"
}

import java.util.Properties
import java.io.FileInputStream

// Release signing: create android/key.properties with
//   storePassword=… keyPassword=… keyAlias=… storeFile=… (relative to android/)
// The keystore itself is NOT committed (see android/README or AGENTS.md).
val keystoreProperties = Properties().apply {
    val f = rootProject.file("key.properties")
    if (f.exists()) {
        FileInputStream(f).use { load(it) }
    }
}

android {
    namespace = "com.soshal.app"
    compileSdk = 37
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "com.soshal.app"
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
            if (keystoreProperties["storeFile"] != null) {
                signingConfig = signingConfigs.create("release") {
                    keyAlias = keystoreProperties["keyAlias"] as String
                    keyPassword = keystoreProperties["keyPassword"] as String
                    storeFile = rootProject.file(keystoreProperties["storeFile"] as String)
                    storePassword = keystoreProperties["storePassword"] as String
                }
            }
        }
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
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
            install("rnspure==1.5.2")
        }
    }
}

tasks.register("copyDaemonsToJniLibs") {
    doLast {
        val assetsDir = file("src/main/assets/daemons")
        val jniLibsDir = file("src/main/jniLibs")
        val abis = listOf("arm64-v8a", "x86_64", "armeabi-v7a")
        for (abi in abis) {
            val i2pdSrc = file("$assetsDir/$abi/i2pd")
            if (i2pdSrc.exists() && i2pdSrc.length() > 100000) {
                val targetDir = file("$jniLibsDir/$abi")
                targetDir.mkdirs()
                i2pdSrc.copyTo(file("$targetDir/libi2pd.so"), overwrite = true)
            }
        }
        val freenetSrc = file("$assetsDir/freenet")
        if (freenetSrc.exists() && freenetSrc.length() > 100000) {
            val targetDir = file("$jniLibsDir/arm64-v8a")
            targetDir.mkdirs()
            freenetSrc.copyTo(file("$targetDir/libfreenet.so"), overwrite = true)
        }
    }
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
    dependsOn("copyDaemonsToJniLibs")
}

