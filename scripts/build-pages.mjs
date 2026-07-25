/**
 * GitHub Pages 用のビルド。
 *
 * Pages は `https://<user>.github.io/<repo>/` のようにサブパスで配信されるので、
 * 資産の参照先を `/photo-curator/` 起点にする必要がある。一方 Tauri は
 * `tauri://localhost` の直下から読み、**出力先は両方とも `.output/public`**。
 * そのため `nuxt.config.ts` に固定値を書けず、環境変数で出し分ける。
 *
 * Windows の PowerShell には `VAR=x cmd` の書き方が無いため、
 * ここで環境変数を渡してから nuxt を起動する（依存は増やさない）。
 *
 * 配信先を変えるときは `--base=/other/` を渡す。
 */
import { spawn } from 'node:child_process'

const DEFAULT_BASE = '/photo-curator/'

const fromArgs = process.argv.slice(2)
  .find(argument => argument.startsWith('--base='))
  ?.slice('--base='.length)

let base = fromArgs ?? process.env.NUXT_APP_BASE_URL ?? DEFAULT_BASE
// Nuxt は前後のスラッシュを前提に組み立てる。揃えておく。
if (!base.startsWith('/')) base = `/${base}`
if (!base.endsWith('/')) base = `${base}/`

console.log(`[build-pages] base URL: ${base}`)

const child = spawn('nuxt', ['generate'], {
  stdio: 'inherit',
  shell: true,
  env: { ...process.env, NUXT_APP_BASE_URL: base }
})
child.on('close', code => process.exit(code ?? 1))
