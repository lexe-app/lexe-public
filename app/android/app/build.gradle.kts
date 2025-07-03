plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "app.lexe.lexeapp"

    // Match the values in `app_rs_dart/android/build.gradle`
    compileSdk = flutter.compileSdkVersion
    ndkVersion = android.ndkVersion

    // println("app: flutter.minSdkVersion: ${flutter.minSdkVersion}")
    // println("app: flutter.targetSdkVersion: ${flutter.targetSdkVersion}")
    // println("app: flutter.compileSdkVersion: ${flutter.compileSdkVersion}")
    // println("app: flutter.ndkVersion: ${flutter.ndkVersion}")
    // println("app: android.ndkVersion: ${android.ndkVersion}")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_11.toString()
    }

    defaultConfig {
        applicationId = "app.lexe.lexeapp"

        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = 23
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    // Comment this out to force flutter to produce an unsigned appbundle. We'll
    // sign the appbundle with our own tooling.
    // buildTypes {
    //     release {
    //         // Signing with the debug keys for now, so
    //         // `flutter run --release` works.
    //         signingConfig = signingConfigs.getByName("debug")
    //     }
    // }

    flavorDimensions += "default"

    // We're now using different application "flavors" to differentiate
    // prod/staging/dev/design instead of flutter `--dart-define` args.
    //
    // The key advantage is that each flavor gets its own separately installed
    // app on-device. This way we can have multiple flavors installed
    // simultaneously. The app variants then don't step on each other's toes.
    productFlavors {
        create("design") {
            // Lexe design mode.
            dimension = "default"
            // The app name. ex: displayed under the icon on the user's home screen.
            resValue("string", "app_name", "Lexe Design")
            applicationIdSuffix = ".design"
            versionNameSuffix = "-design"

            // The google drive oauth2 callback URI scheme
            // Keep in sync with <app/lib/gdrive_auth.dart::_GDriveCredentials>
            // Signer: <debug>
            resValue(type = "string", name = "google_callback_uri_scheme", value = "com.googleusercontent.apps.495704988639-qhjbk0nkfaibgr16h0gimlqcae8cl13e")
        }

        create("dev") {
            // (Default) Local development against a local lexe backend.
            dimension = "default"
            // The app name. ex: displayed under the icon on the user's home screen.
            resValue(type = "string", name = "app_name", value = "Lexe Dev")
            applicationIdSuffix = ".dev"
            versionNameSuffix = "-dev"

            // AndroidManifest.xml requires this value to be set, even though
            // local dev doesn't use gDrive.
            resValue(type = "string", name = "google_callback_uri_scheme", value = "DUMMY")
        }

        create("staging") {
            // Lexe testnet/staging backend.
            dimension = "default"
            // The app name. ex: displayed under the icon on the user's home screen.
            resValue(type = "string", name = "app_name", value = "Lexe Staging")
            applicationIdSuffix = ".staging"
            versionNameSuffix = "-staging"

            // The google drive oauth2 callback URI scheme
            // Keep in sync with <app/lib/gdrive_auth.dart::_GDriveCredentials>
            // Signer: <debug>
            resValue(type = "string", name = "google_callback_uri_scheme", value = "com.googleusercontent.apps.495704988639-fvkq7thnksbqi7n3tanpopu5brr2pa4a")
        }

        create("prod") {
            // Lexe mainnet/production backend.
            dimension = "default"
            // The app name. ex: displayed under the icon on the user's home screen.
            resValue(type = "string", name = "app_name", value = "Lexe")

            // The google drive oauth2 callback URI scheme
            // Keep in sync with <app/lib/gdrive_auth.dart::_GDriveCredentials>
            // Signer: <Google Play>
            resValue(type = "string", name = "google_callback_uri_scheme", value = "com.googleusercontent.apps.495704988639-cr7bvcr117n7aks3p3e3qntoa7ps0lj1")
        }
    }
}

flutter {
    source = "../.."
}
