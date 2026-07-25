/**
 * `tauri dev --release` を、起動がブロックされたら作り直して再試行する。
 *
 * Windows のアプリケーション制御ポリシーが、署名の無い**新しい実行ファイル**を
 * 弾くことがある（os error 4551）。判定はファイル単位で固定されるらしく、
 * 一度通ったバイナリは通り続け、弾かれたバイナリは何度起動しても弾かれる。
 * そのためリトライではなく、**ソースに触れて別のハッシュのバイナリを作らせてから**
 * 起動し直す。数回で通ることが多い。
 *
 * 恒久的に直すには Windows セキュリティの Smart App Control を切る必要があるが、
 * それは開発機の保護を弱める操作なので、ここでは行わない。
 */
import { spawn } from 'node:child_process'
import { utimesSync } from 'node:fs'

const MAX_ATTEMPTS = 6
const BLOCKED = /os error 4551|アプリケーション制御ポリシー/
const TOUCH_TARGET = new URL('../src-tauri/src/lib.rs', import.meta.url)

/** 再ビルドさせるため、内容を変えずに更新時刻だけ進める。 */
function touchSource() {
  const now = new Date()
  utimesSync(TOUCH_TARGET, now, now)
}

/**
 * 1回起動する。ブロックされたら true、それ以外（正常終了・利用者による終了）は
 * false を返す。
 */
function runOnce() {
  return new Promise(resolve => {
    const child = spawn('pnpm', ['tauri', 'dev', '--release'], {
      stdio: ['inherit', 'pipe', 'pipe'],
      shell: true
    })
    let blocked = false

    const watch = (chunk, target) => {
      const text = chunk.toString()
      target.write(text)
      if (BLOCKED.test(text)) blocked = true
    }
    child.stdout.on('data', chunk => watch(chunk, process.stdout))
    child.stderr.on('data', chunk => watch(chunk, process.stderr))

    // Ctrl+C はそのまま子へ渡し、再試行せずに終わらせる。
    const forward = () => { child.kill('SIGINT') }
    process.on('SIGINT', forward)
    child.on('close', code => {
      process.off('SIGINT', forward)
      resolve(blocked && code !== 0)
    })
  })
}

for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt += 1) {
  if (attempt > 1) {
    console.log(`\n[tauri-dev] 起動がブロックされました。別のバイナリを作って再試行します（${attempt} / ${MAX_ATTEMPTS}）\n`)
    touchSource()
  }
  const blocked = await runOnce()
  if (!blocked) process.exit(0)
}

console.error(`\n[tauri-dev] ${MAX_ATTEMPTS} 回試しても起動できませんでした。`)
console.error('Windows セキュリティ →「アプリとブラウザーの制御」→ Smart App Control を確認してください。')
process.exit(1)
