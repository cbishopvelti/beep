// The app is Rust: a NativeActivity loads libbeep_android.so, which
// `cargo ndk` builds into src/main/jniLibs before every build. The Java in
// src/main/java only forwards what Android delivers by class name (the
// watch's broadcasts, the Bluetooth callbacks) to Rust.

plugins {
    id("com.android.application")
}

// The version people see is Cargo.toml's (`version = "0.1.0"`), so the app and the APK agree.
val cargoVersion: String = Regex("(?m)^version = \"([^\"]+)\"")
    .find(rootProject.file("Cargo.toml").readText())!!.groupValues[1]

// Android won't install an APK over one with a higher versionCode, so the code is when it was built
// (minutes since 2026): any newer build installs over an older one, whichever branch it's from.
val buildMinute: Int = ((System.currentTimeMillis() / 1000 - 1_767_225_600L) / 60).toInt()

android {
    namespace = "com.cbishopvelti.beep"
    compileSdk = 36
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "com.cbishopvelti.beep"
        minSdk = 26
        targetSdk = 36
        versionCode = buildMinute
        versionName = cargoVersion
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    signingConfigs {
        // The repo's own debug key, so an APK built on any machine installs over one built on
        // another (Android refuses an update signed with a different key, and uninstalling loses
        // the phone's results). It's a debug key, not a secret.
        getByName("debug") {
            storeFile = file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        debug {
            signingConfig = signingConfigs.getByName("debug")
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    // The Rust library goes into the APK uncompressed, aligned to 16 KB pages, so Android maps it
    // straight from the APK (.cargo/config.toml lays the library itself out for 16 KB pages).
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
}

dependencies {
    // FileProvider, which hands the share sheet a content:// uri for an export (Share TCX, Share
    // CSV), called from Rust over JNI.
    implementation("androidx.core:core:1.13.1")
}

// Rust ABIs to build; override with -PrustTargets=arm64-v8a for a faster loop.
val rustTargets = (findProperty("rustTargets") as String? ?: "arm64-v8a,x86_64").split(",")

val cargoBuild by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds the Rust library for Android with cargo-ndk."
    workingDir = rootProject.projectDir
    // The NDK Gradle uses (ndkVersion above, from the SDK), unless set already.
    if (System.getenv("ANDROID_NDK_HOME") == null) {
        environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    }
    val args = mutableListOf("cargo", "ndk", "-P", "26", "-o", "app/src/main/jniLibs")
    rustTargets.forEach { args += listOf("-t", it) }
    args += listOf("build", "--release", "--lib")
    commandLine(args)
}

tasks.named("preBuild") { dependsOn(cargoBuild) }
