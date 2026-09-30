/**
 * 解析を数枚ずつ並行して回す。
 *
 * ワーカーが使えればそちらへ、使えなければメインスレッドで同じ関数を呼ぶ。
 * 並行数を欲張らないのは、HEIC の全復号が 1 枚で数十 MB になり、
 * iPad で同時に何枚も抱えるとメモリ不足で落ちるため。
 */
import type { AnalyzedPhoto } from '~/utils/analyzePhoto'
import { analyzePhotoFile } from '~/utils/analyzePhoto'
import type { AnalyzeWorkerRequest, AnalyzeWorkerResponse } from '~/workers/analyze'

/** 同時に走らせる本数の既定。復号 1 枚ぶんのメモリを考えて低めに抑える。 */
export const DEFAULT_WORKERS = 3

/**
 * 環境に合う本数。フォルダを選べるブラウザ（PC の Chrome・Edge）は 3 本、
 * それ以外（iPad の Safari など。HEIC の復号 1 枚で数十 MB 使う）は 2 本に絞る。
 * `largeGroups` はブラウザでは常に偽なので、ここでは使わない（10章 T7 の保留を 2026-09-30 に決めた）。
 */
export const workersFor = (hasDirectoryPicker: boolean) => (hasDirectoryPicker ? DEFAULT_WORKERS : 2)

export interface AnalysisJob {
  id: string
  /** 手元にあるファイル。無ければ `load` が出所から読む（走る直前に 1 枚ずつ）。 */
  file?: File
  load?: () => Promise<File>
}

export interface AnalysisPoolOptions {
  /** 1 枚終わるたびに呼ばれる。進捗表示と保存に使う。 */
  onResult: (id: string, analyzed: AnalyzedPhoto) => Promise<void> | void
  /** true を返すと以降を打ち切る。 */
  isCancelled?: () => boolean
  /** 同時に走らせる本数の上限。 */
  workers?: number
}

function createWorker(): Worker | null {
  try {
    if (typeof Worker === 'undefined') return null
    // `import.meta.url` 起点にすると、GitHub Pages のサブパス配信でも
    // Vite が正しい URL を埋め込む。
    return new Worker(new URL('../workers/analyze.ts', import.meta.url), { type: 'module' })
  } catch {
    return null
  }
}

/** 1 枚をワーカーに投げて結果を待つ。 */
function runOnWorker(worker: Worker, job: AnalysisJob, file: File): Promise<AnalyzedPhoto> {
  return new Promise(resolve => {
    const finish = (analyzed: AnalyzedPhoto) => {
      worker.onmessage = null
      worker.onerror = null
      resolve(analyzed)
    }
    worker.onmessage = (event: MessageEvent<AnalyzeWorkerResponse>) => {
      const { id: _id, ...analyzed } = event.data
      finish(analyzed)
    }
    worker.onerror = () => finish(failed('解析中にエラーが発生しました。'))
    const request: AnalyzeWorkerRequest = { id: job.id, file }
    worker.postMessage(request)
  })
}

const failed = (error: string): AnalyzedPhoto => ({
  thumbnail: null, display: null, dHash: null, capturedAt: null, timestampSource: 'unknown', error
})

/** 1 枚を読む。読めなかったときは理由を返す（例外は投げない）。 */
async function loadFile(job: AnalysisJob): Promise<File | string> {
  try {
    const loaded = job.file ?? await job.load?.()
    return loaded ?? '写真を読み込めませんでした。'
  } catch (cause) {
    return cause instanceof Error ? cause.message : '写真を読み込めませんでした。'
  }
}

/** 読んだ 1 枚を解析する。 */
async function analyzeOne(worker: Worker | null, job: AnalysisJob, file: File | string): Promise<AnalyzedPhoto> {
  if (typeof file === 'string') return failed(file)
  return worker ? runOnWorker(worker, job, file) : analyzePhotoFile(file)
}

/**
 * 全部を解析する。並行数のぶんだけ走らせ、終わったものから順に `onResult` を呼ぶ。
 * 途中で `isCancelled` が true になったら、走っているぶんを終えてから止める。
 *
 * **次の 1 枚は、いまの 1 枚を解析している間に読んでおく**（出所が HTTP・フォルダのとき、
 * 読み込みを待つ間ワーカーが遊ばないように）。
 */
export async function analyzeAll(jobs: AnalysisJob[], options: AnalysisPoolOptions): Promise<void> {
  const lanes = Math.max(1, Math.min(options.workers ?? DEFAULT_WORKERS, jobs.length))
  const workers: (Worker | null)[] = Array.from({ length: lanes }, () => createWorker())
  let next = 0

  const take = () => {
    if (next >= jobs.length || options.isCancelled?.()) return null
    const job = jobs[next]!
    next += 1
    return { job, file: loadFile(job) }
  }

  const consume = async (worker: Worker | null) => {
    let current = take()
    while (current) {
      const upcoming = take()
      const analyzed = await analyzeOne(worker, current.job, await current.file)
      await options.onResult(current.job.id, analyzed)
      current = options.isCancelled?.() ? null : upcoming
    }
  }

  try {
    await Promise.all(workers.map(worker => consume(worker)))
  } finally {
    for (const worker of workers) worker?.terminate()
  }
}
