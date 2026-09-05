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

const release = process.argv.includes('--release')
const profile = release ? 'Release' : 'Debug'
const profileDir = release ? 'release' : 'debug'

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
])

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
const gradlew = process.platform === 'win32' ? 'gradlew.bat' : './gradlew'
run(gradlew, [
  `assemble${TARGET.arch}${profile}`,
  '-x', `rustBuild${TARGET.arch}${profile}`,
  '--console=plain'
], { cwd: ANDROID })

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
