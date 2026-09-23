# Photo Curator

A human-centered workflow for quickly curating large photo collections through
lightweight, repeated decisions (bursts, ratings, a learned near-duplicate
threshold). Three apps share one judgment engine:

| | 場所 | 役割 |
|---|---|---|
| **core** | `core/` | 判断そのもの（星・連写のまとめ方・指紋・サイドカー等）を持つ Rust クレート。**唯一の正**。PC はこれを直接呼ぶ。Android は UniFFI 経由、Web は `core-wasm/`（wasm-bindgen）経由で同じ core を使う。 |
| **PC・Web（このリポジトリの本体）** | `pages/` `components/` `composables/` `lib/` `src-tauri/` | Tauri 2 + Nuxt 3/Vue 3/Vuetify。デスクトップ（Windows／Tauri）とブラウザ（iPad 等／Web）の両方をこの 1 つの Nuxt アプリから出す。**判断は持たない**（core に聞くだけ）。 |
| **Android（作り直し版）** | `app-android/` | Kotlin のネイティブアプリ。core を UniFFI 経由で呼ぶ。旧 Tauri Android ビルド（`tauri android`／`gen/android`）は 2026-09 に廃止し、こちらに一本化した。 |

設計の正は Obsidian の「引き継ぎ/Photo Curator 設計書/」（01〜09 章）。実装より
先に設計を書く。

## Stack

- **core**: Rust（UniFFI で Android へ、wasm-bindgen で Web へ）
- **PC・Web**: Tauri 2 / Rust、Nuxt 3 / Vue 3 / TypeScript、Vuetify 3、Pinia、Vitest
- **Android**: Kotlin（Jetpack Compose）、Gradle

## Development（PC・Web）

```powershell
pnpm install
pnpm tauri:dev
```

Web だけ確かめたい場合（ブラウザの `showDirectoryPicker`／写真ピッカーの
どちらの環境も、実機なしでこの 1 コマンドから確かめられる）:

```powershell
pnpm dev
```

Useful checks:

```powershell
pnpm typecheck
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path core/Cargo.toml
```

## Development（Android）

実機（USB デバッグ、AVD `dev_fold`／`dev_pixel8`）で動かすまで。

```powershell
node scripts/next-apk.mjs
```

`core`（Rust）を Android 向けに組んでから `app-android/` を `assembleDebug` し、
`adb install` まで行う（`--no-install` で install だけ省略できる）。

### 必要な環境

| | 値 |
|---|---|
| `JAVA_HOME` | **JDK 21**。Android Studio 同梱の JBR は対応していないバージョンのことがある |
| `ANDROID_HOME` | Android SDK |
| Rust ターゲット | `aarch64-linux-android`（`rustup target add`） |

`app-android/` は不変が前提（PC・Web 側の作業でここを変更しない）。core に
関数を足すのは可（UDL・既存関数は不変）。
