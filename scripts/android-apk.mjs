/**
 * Android の APK を組み立てる。**`tauri android build` の代わり。**
 *
 * ## なぜ CLI をそのまま使えないか
 *
 * `tauri android build` は、cargo が作った `.so` を jniLibs へ
 * **シンボリックリンク**で置く。Windows でシンボリックリンクを作るには
 * 開発者モードか管理者権限が要り、この環境にはどちらも無い:
 *
 *     Creation symbolic link is not allowed for this system.
 *
 * やっていることは「ビルドして置く」だけなので、リンクの代わりに**コピー**する。
 * それ以外は CLI と同じ順序で、同じものを作る。
 *
 * ## 手順
 *
 * 1. `pnpm build` … 画面を `.output/public` へ。
 *    **これは .so に埋め込まれる**（`tauri/custom-protocol`）ので、
 *    assets へのコピーは要らない。
 * 2. `cargo build --target <target> --lib` … Rust を Android 向けに。
 * 3. `.so` を `jniLibs/<abi>/` へコピー。
 * 4. `gradlew assemble<Arch><Profile> -x rustBuild<Arch><Profile>`
 *    … Rust のタスクは 2 で済ませたので外す（ここが CLI を呼び返して
 *    シンボリックリンクを作ろうとする）。
 *
 * 開発者モードを有効にできる環境なら `pnpm tauri android build` で足りる。
 * そのときはこのスクリプトは不要。
 *
 * 使い方: `pnpm android:apk [--release]`
 */
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'

const ROOT = resolve(import.meta.dirname, '..')
const ANDROID = join(ROOT, 'src-tauri', 'gen', 'android')

/** いまは実機（arm64）だけ。他の ABI が要るようになったら足す。 */
const TARGET = {
  rust: 'aarch64-linux-android',
  abi: 'arm64-v8a',
  arch: 'Arm64',
  library: 'libphoto_curator_lib.so'
}

/** `app/build.gradle.kts` の minSdk と揃える。clang の名前に入る。 */
const MIN_SDK = 24

const release = process.argv.includes('--release')
const profile = release ? 'Release' : 'Debug'
const profileDir = release ? 'release' : 'debug'

/**
 * NDK のツールチェインを cargo に教える。
 *
 * `rusqlite` は C を含む（bundled SQLite）ので、cc-rs が clang を探す。
 * 素の PATH には無く、`failed to find tool "clang.exe"` で止まる。
 * これは `tauri android build` が内部でやっていたことと同じ。
 */
/** 版名（27.2.12479018 のような形）を数値として比べる。 */
function compareVersions(a, b) {
  const parts = value => value.split('.').map(Number)
  const [left, right] = [parts(a), parts(b)]
  for (let i = 0; i < Math.max(left.length, right.length); i += 1) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0)
    if (diff) return diff
  }
  return 0
}

/** SDK の場所。環境変数が無ければ既定の置き場を見る。 */
function androidSdk() {
  const explicit = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT
  if (explicit) return explicit
  const home = process.env.LOCALAPPDATA ?? process.env.HOME
  const guess = home && join(home, process.platform === 'win32' ? 'Android' : 'Android', 'Sdk')
  return guess && existsSync(guess) ? guess : null
}

/**
 * NDK の場所。**環境変数が無くても探す。**
 *
 * 環境変数を User スコープに設定しても、**それ以前から開いている
 * ターミナルには伝わらない。** 新しいターミナルを開けば済む話だが、
 * ここで止まると Nuxt のビルドをやり直すことになるので、
 * SDK の下から一番新しい NDK を拾う。
 */
function findNdk() {
  const explicit = process.env.NDK_HOME || process.env.ANDROID_NDK_HOME
  if (explicit) return explicit
  const sdk = androidSdk()
  if (!sdk) return null
  const root = join(sdk, 'ndk')
  if (!existsSync(root)) return null
  const versions = readdirSync(root)
    .filter(name => existsSync(join(root, name, 'source.properties')))
    .sort(compareVersions)
  const latest = versions.at(-1)
  return latest ? join(root, latest) : null
}

/**
 * Gradle に渡す JDK。**NDK と同じ理由で、環境変数が無くても探す。**
 *
 * Android Studio 同梱の JBR は JDK 25 で、Gradle 8.14 が対応していない。
 * そのため別に置いた JDK 21 を使う。素の PATH に java は無い。
 */
function findJdk() {
  const explicit = process.env.JAVA_HOME
  if (explicit) return explicit
  const programs = process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Programs')
  if (!programs || !existsSync(programs)) return null
  const candidates = readdirSync(programs)
    .filter(name => /^jdk/i.test(name))
    .filter(name => existsSync(join(programs, name, 'bin', 'java.exe')))
    .sort()
  const found = candidates.at(-1)
  return found ? join(programs, found) : null
}

function androidToolchainEnv() {
  const ndk = findNdk()
  if (!ndk) {
    console.error('NDK が見つかりません。Android Studio の SDK Manager で NDK を入れるか、')
    console.error('NDK_HOME を設定してください。')
    process.exit(1)
  }
  const host = process.platform === 'win32' ? 'windows-x86_64'
    : process.platform === 'darwin' ? 'darwin-x86_64' : 'linux-x86_64'
  const bin = join(ndk, 'toolchains', 'llvm', 'prebuilt', host, 'bin')
  if (!existsSync(bin)) {
    console.error(`NDK のツールチェインが見つかりません: ${bin}`)
    process.exit(1)
  }
  const suffix = process.platform === 'win32' ? '.cmd' : ''
  const clang = join(bin, `aarch64-linux-android${MIN_SDK}-clang${suffix}`)
  return {
    PATH: `${bin}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}`,
    CC_aarch64_linux_android: clang,
    CXX_aarch64_linux_android: join(bin, `aarch64-linux-android${MIN_SDK}-clang++${suffix}`),
    AR_aarch64_linux_android: join(bin, 'llvm-ar' + (process.platform === 'win32' ? '.exe' : '')),
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER: clang
  }
}

function run(command, args, options = {}) {
  const shown = [command, ...args].join(' ')
  console.log(`\n$ ${shown}`)
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: { ...process.env, ...options.env }
  })
  if (result.status !== 0) {
    console.error(`\n失敗しました: ${shown}`)
    process.exit(result.status ?? 1)
  }
}

// 0. 道具が揃っているかを**先に**見る。ここで落ちれば数秒で済むが、
//    Nuxt と cargo を走らせたあとで落ちると数分を捨てることになる。
const toolchain = androidToolchainEnv()
const jdk = findJdk()
if (!jdk) {
  console.error('JDK が見つかりません。JAVA_HOME を設定してください。')
  console.error('（Android Studio 同梱の JBR は 25 で、Gradle 8.14 が対応していません）')
  process.exit(1)
}

// 1. 画面。`.so` に埋め込まれるので、これを先にやる。
run('pnpm', ['build'])

// 2. Rust。`custom-protocol` は CLI が渡しているのと同じ。
run('cargo', [
  'build',
  '--package', 'photo-curator',
  '--manifest-path', join(ROOT, 'src-tauri', 'Cargo.toml'),
  '--target', TARGET.rust,
  '--features', 'tauri/custom-protocol',
  '--lib',
  ...(release ? ['--release'] : [])
], { env: toolchain })

// 3. シンボリックリンクの代わりにコピー。
const built = join(ROOT, 'src-tauri', 'target', TARGET.rust, profileDir, TARGET.library)
if (!existsSync(built)) {
  console.error(`\nビルド結果が見つかりません: ${built}`)
  process.exit(1)
}
const jniDir = join(ANDROID, 'app', 'src', 'main', 'jniLibs', TARGET.abi)
mkdirSync(jniDir, { recursive: true })
copyFileSync(built, join(jniDir, TARGET.library))
console.log(`\n配置: ${TARGET.abi}/${TARGET.library} (${(statSync(built).size / 1048576).toFixed(1)} MB)`)

// 4. Gradle。Rust のタスクは済ませたので外す。
// cmd は cwd の実行ファイルを PATH から探さないので、絶対パスで渡す。
const gradlew = join(ANDROID, process.platform === 'win32' ? 'gradlew.bat' : 'gradlew')
run(gradlew, [
  `assemble${TARGET.arch}${profile}`,
  '-x', `rustBuild${TARGET.arch}${profile}`,
  '--console=plain'
], { cwd: ANDROID, env: { JAVA_HOME: jdk } })

// 出来上がりの場所を出す。`adb install -r <path>` にそのまま渡せる。
const outputs = join(ANDROID, 'app', 'build', 'outputs', 'apk')
const found = []
const walk = directory => {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) walk(path)
    else if (entry.name.endsWith('.apk')) found.push(path)
  }
}
if (existsSync(outputs)) walk(outputs)
console.log('\n=== APK ===')
for (const path of found) {
  console.log(`${path}  (${(statSync(path).size / 1048576).toFixed(1)} MB)`)
}
console.log('\n実機へ: adb install -r <上のパス>')
