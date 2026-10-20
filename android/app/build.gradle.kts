// The Rust game is built by ../build-rust.sh into src/main/jniLibs (not committed) before this
// packages it. Release signing reads LILY_KEYSTORE / LILY_KEYSTORE_PASSWORD / LILY_KEY_ALIAS /
// LILY_KEY_PASSWORD; without them a release build is signed with the debug key.
plugins {
    id("com.android.application")
}

android {
    namespace = "app.lily"
    compileSdk = 35

    defaultConfig {
        applicationId = (project.findProperty("lilyApplicationId") as String?) ?: "app.lily"
        // AAudio (cpal's Android backend) needs 26.
        minSdk = 26
        targetSdk = 35
        versionCode = 3
        versionName = "0.2.2"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    signingConfigs {
        create("release") {
            val store = System.getenv("LILY_KEYSTORE")
            if (store != null) {
                storeFile = file(store)
                storePassword = System.getenv("LILY_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("LILY_KEY_ALIAS")
                keyPassword = System.getenv("LILY_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = if (System.getenv("LILY_KEYSTORE") != null) {
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
}
