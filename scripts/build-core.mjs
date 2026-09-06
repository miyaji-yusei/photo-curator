/**
 * core（Rust）を Android 向けに組み、`.so` と Kotlin のバインディングを
 * app-android へ置く。**手で運ぶと必ずずれるので、1 本の手順にする。**
 *
 * cargo-ndk は使わない。NDK のツールチェインを環境変数で cargo に渡せば
 * `--target` だけで組める（android-apk.mjs と同じやり方）。
 *
 * 使い方: node scripts/build-core.mjs [--debug]
 */
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'

const ROOT = resolve(import.meta.dirname, '..')
const CORE = join(ROOT, 'core')
const APP = join(ROOT, 'app-android', 'app', 'src', 'main')
const TARGET = 'aarch64-linux-android'
const ABI = 'arm64-v8a'
const LIB = 'libphoto_curator_core.so'
const MIN_SDK = 24

const release = !process.argv.includes('--debug')
const profileDir = release ? 'release' : 'debug'

function compareVersions(a, b) {
  const parts = value => value.split('.').map(Number)
  const [left, right] = [parts(a), parts(b)]
  for (let i = 0; i < Math.max(left.length, right.length); i += 1) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0)
    if (diff) return diff
  }
  return 0
}

/** NDK の場所。環境変数が無くても SDK の下から一番新しいものを拾う。 */
function findNdk() {
  const explicit = process.env.NDK_HOME || process.env.ANDROID_NDK_HOME
  if (explicit) return explicit
  const sdk = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT
    || (process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Android', 'Sdk'))
  if (!sdk || !existsSync(join(sdk, 'ndk'))) return null
  const versions = readdirSync(join(sdk, 'ndk'))
    .filter(name => existsSync(join(sdk, 'ndk', name, 'source.properties')))
    .sort(compareVersions)
  const latest = versions.at(-1)
  return latest ? join(sdk, 'ndk', latest) : null
}

const ndk = findNdk()
if (!ndk) {
  console.error('NDK が見つかりません。SDK Manager で入れるか NDK_HOME を設定してください。')
  process.exit(1)
}
const host = process.platform === 'win32' ? 'windows-x86_64'
  : process.platform === 'darwin' ? 'darwin-x86_64' : 'linux-x86_64'
const bin = join(ndk, 'toolchains', 'llvm', 'prebuilt', host, 'bin')
const suffix = process.platform === 'win32' ? '.cmd' : ''
const clang = join(bin, `aarch64-linux-android${MIN_SDK}-clang${suffix}`)

const env = {
  ...process.env,
  PATH: `${bin}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}`,
  CC_aarch64_linux_android: clang,
  AR_aarch64_linux_android: join(bin, 'llvm-ar' + (process.platform === 'win32' ? '.exe' : '')),
  CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER: clang
}

function run(command, args, options = {}) {
  console.log(`\n$ ${[command, ...args].join(' ')}`)
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? CORE, stdio: 'inherit',
    shell: process.platform === 'win32', env: options.env ?? env
  })
  if (result.status !== 0) {
    console.error('\n失敗しました')
    process.exit(result.status ?? 1)
  }
}

// 1. Android 向けに組む
run('cargo', ['build', '--target', TARGET, ...(release ? ['--release'] : [])])

// 2. Kotlin のバインディングを作る。**版のずれを避けるため同じクレートの bin から。**
run('cargo', ['run', '--bin', 'uniffi-bindgen', '--', 'generate', 'src/core.udl',
  '--language', 'kotlin', '--out-dir', 'bindings'], { env: process.env })

// 3. app-android へ置く
const built = join(CORE, 'target', TARGET, profileDir, LIB)
if (!existsSync(built)) {
  console.error(`ビルド結果が見つかりません: ${built}`)
  process.exit(1)
}
const jniDir = join(APP, 'jniLibs', ABI)
mkdirSync(jniDir, { recursive: true })
copyFileSync(built, join(jniDir, LIB))

const bindingsDir = join(APP, 'java', 'uniffi', 'core')
mkdirSync(bindingsDir, { recursive: true })
copyFileSync(join(CORE, 'bindings', 'uniffi', 'core', 'core.kt'), join(bindingsDir, 'core.kt'))

console.log(`\n置いた: ${ABI}/${LIB} (${(statSync(built).size / 1024).toFixed(0)} KB)`)
console.log(`置いた: uniffi/core/core.kt`)
