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

    packaging {
        resources {
            // smbj が持ち込む重複メタデータ。**入れても意味がないので落とす。**
            excludes += setOf(
                "META-INF/DEPENDENCIES", "META-INF/LICENSE*", "META-INF/NOTICE*",
                "META-INF/*.kotlin_module"
            )
        }
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

    // EXIF を読む。**NAS では原本の先頭 64KB だけ**から撮影時刻と縮小画像を取る。
    implementation("androidx.exifinterface:exifinterface:1.3.7")

    // **NAS（SMB）。** smbj は純 Java で、Android でもそのまま動く。
    // ログは SLF4J 越しに出るので、Android の Log に流す実装を入れる。
    implementation("com.hierynomus:smbj:0.13.0")
    implementation("org.slf4j:slf4j-api:1.7.36")
    implementation("uk.uuid.slf4j:slf4j-android:1.7.30-0")

    // UniFFI が生成した Kotlin は JNA を使う。
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    debugImplementation("androidx.compose.ui:ui-tooling")
}
