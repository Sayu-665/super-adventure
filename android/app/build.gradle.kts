plugins {
    id("com.android.application")
}

// Optional release keystore supplied through the environment. When any value is missing the
// release build falls back to the debug key so `assembleRelease` always yields an installable APK.
val releaseKeystore: String? = System.getenv("OPENSURF_KEYSTORE")
val releaseKeystorePassword: String? = System.getenv("OPENSURF_KEYSTORE_PASSWORD")
val releaseKeyAlias: String? = System.getenv("OPENSURF_KEY_ALIAS")
val releaseKeyPassword: String? = System.getenv("OPENSURF_KEY_PASSWORD")
val hasReleaseKey = listOf(releaseKeystore, releaseKeystorePassword, releaseKeyAlias, releaseKeyPassword)
    .all { !it.isNullOrBlank() }

android {
    namespace = "com.opensurf.browser"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.opensurf.browser"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "1.0.0"
    }

    signingConfigs {
        // Sign with every APK signature scheme (v1 is otherwise skipped at minSdk 24) so that
        // OEM package installers and file managers that still inspect JAR signatures accept it.
        getByName("debug") {
            enableV1Signing = true
            enableV2Signing = true
            enableV3Signing = true
        }
        if (hasReleaseKey) {
            create("release") {
                storeFile = file(releaseKeystore!!)
                storePassword = releaseKeystorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            vcsInfo.include = false // reproducible output, no git metadata in the APK
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = if (hasReleaseKey) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // Privacy: do not embed the (Google-encrypted) dependency metadata block in the APK.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    lint {
        abortOnError = true
        checkReleaseBuilds = true
        // SDK / AGP / dependency versions are pinned by the product spec on purpose.
        disable += setOf("OldTargetApi", "GradleDependency", "AndroidGradlePluginVersion", "NewerVersionAvailable")
    }
}

base {
    archivesName.set("OpenSurf-1.0.0")
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
