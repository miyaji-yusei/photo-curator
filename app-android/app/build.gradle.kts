plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.photocurator.next"
    compileSdk = 36

    defaultConfig {
        // **いまのアプリとは別の id。** 並べて入れて見比べられるようにする。
        applicationId = "app.photocurator.next"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
        // 実機は arm64 だけ。他が要るようになったら足す。
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.15" }

    // core の .so はビルド前に scripts/build-core.mjs が置く。
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation(platform("androidx.compose:compose-bom:2024.10.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")

    // **写真の読み込み。** ここを自前でやらないのが native の利点。
    // Coil はメモリとディスクのキャッシュ、要求サイズでのデコード、
    // 画面から消えたときの取り消しまで面倒を見る。
    implementation("io.coil-kt:coil-compose:2.7.0")

    // UniFFI が生成した Kotlin は JNA を使う。
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    debugImplementation("androidx.compose.ui:ui-tooling")
}
