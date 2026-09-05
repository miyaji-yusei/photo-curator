# Photo Curator

A Windows desktop app for quickly curating large photo collections through lightweight, repeated human decisions.

## Stack

- Tauri 2 / Rust
- Nuxt 3 / Vue 3 / TypeScript
- Vuetify 3
- Pinia
- Vitest

## Development

```powershell
pnpm install
pnpm tauri:dev
```

Useful checks:

```powershell
pnpm typecheck
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

The product workflow and screen structure will be designed before feature implementation begins.

## Android

実機（USB デバッグ）で動かすまで。

```powershell
pnpm android:apk       # APK を作る
adb install -r src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk
```

### 必要な環境

| | 値 |
|---|---|
| `JAVA_HOME` | **JDK 21**。Android Studio 同梱の JBR は 25 で、Gradle 8.14 が対応していない（`Unsupported class file major version 69`） |
| `ANDROID_HOME` | Android SDK |
| `NDK_HOME` | NDK 27.2.12479018 |
| Rust ターゲット | `aarch64-linux-android` ほか（`rustup target add`） |

### `tauri android build` を直接使わない理由

CLI は cargo が作った `.so` を jniLibs へ**シンボリックリンク**で置くが、
Windows でそれを作るには開発者モードか管理者権限が要る。
`pnpm android:apk` は同じ手順をコピーで行う。開発者モードを有効にできる環境なら
`pnpm tauri android build` で足りる。
