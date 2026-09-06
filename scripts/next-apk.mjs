/**
 * app-android（作り直し版）を組んで端末に入れる。
 *
 * **JDK と SDK を自分で探す。** 素の PATH に java は無く、Android Studio
 * 同梱の JBR は 25 で Gradle 8.14 が受け付けない。毎回 JAVA_HOME を手で
 * 置く運用にすると、置き忘れた回だけ失敗する。
 */
import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)))
const ANDROID = join(ROOT, 'app-android')

function findJdk() {
  if (process.env.JAVA_HOME && existsSync(join(process.env.JAVA_HOME, 'bin', 'java.exe'))) {
    return process.env.JAVA_HOME
  }
  const programs = process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Programs')
  if (!programs || !existsSync(programs)) return null
  const found = readdirSync(programs)
    .filter(name => /^jdk/i.test(name))
    .filter(name => existsSync(join(programs, name, 'bin', 'java.exe')))
    .sort()
    .at(-1)
  return found ? join(programs, found) : null
}

function findSdk() {
  const explicit = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT
  if (explicit && existsSync(explicit)) return explicit
  const usual = process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Android', 'Sdk')
  return usual && existsSync(usual) ? usual : null
}

function run(command, args, options = {}) {
  console.log('\n$ ' + command + ' ' + args.join(' '))
  const result = spawnSync(command, args, { stdio: 'inherit', shell: true, ...options })
  if (result.status !== 0) process.exit(result.status ?? 1)
}

const jdk = findJdk()
if (!jdk) {
  console.error('JDK が見つかりません。JAVA_HOME を設定してください。')
  console.error('（Android Studio 同梱の JBR は 25 で、Gradle 8.14 が対応していません）')
  process.exit(1)
}
const sdk = findSdk()
if (!sdk) {
  console.error('Android SDK が見つかりません。ANDROID_HOME を設定してください。')
  process.exit(1)
}
console.log('JDK: ' + jdk)
console.log('SDK: ' + sdk)

// core を先に組む。**Kotlin 側だけ新しい状態で入れない。**
// 型が合わないまま動くと、原因が UI に見えてしまう。
run(process.execPath, [join(ROOT, 'scripts', 'build-core.mjs')])

run(join(ANDROID, 'gradlew.bat'), ['assembleDebug'], {
  cwd: ANDROID,
  env: { ...process.env, JAVA_HOME: jdk, ANDROID_HOME: sdk, ANDROID_SDK_ROOT: sdk }
})

const apk = join(ANDROID, 'app', 'build', 'outputs', 'apk', 'debug', 'app-debug.apk')
if (!process.argv.includes('--no-install')) {
  run(join(sdk, 'platform-tools', 'adb.exe'), ['install', '-r', apk])
}
console.log('\nできました: ' + apk)
