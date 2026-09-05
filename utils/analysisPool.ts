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

/** 同時に走らせる本数の上限。復号 1 枚ぶんのメモリを考えて低めに抑える。 */
const MAX_WORKERS = 3

export interface AnalysisJob {
  id: string
  file: File
}

export interface AnalysisPoolOptions {
  /** 1 枚終わるたびに呼ばれる。進捗表示と保存に使う。 */
  onResult: (id: string, analyzed: AnalyzedPhoto) => Promise<void> | void
  /** true を返すと以降を打ち切る。 */
  isCancelled?: () => boolean
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
function runOnWorker(worker: Worker, job: AnalysisJob): Promise<AnalyzedPhoto> {
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
    worker.onerror = () => finish({
      thumbnail: null,
        display: null,
        dHash: null, capturedAt: null,
      timestampSource: 'unknown', error: '解析中にエラーが発生しました。'
    })
    const request: AnalyzeWorkerRequest = { id: job.id, file: job.file }
    worker.postMessage(request)
  })
}

/**
 * 全部を解析する。並行数のぶんだけ走らせ、終わったものから順に `onResult` を呼ぶ。
 * 途中で `isCancelled` が true になったら、走っているぶんを終えてから止める。
 */
export async function analyzeAll(jobs: AnalysisJob[], options: AnalysisPoolOptions): Promise<void> {
  const lanes = Math.max(1, Math.min(MAX_WORKERS, jobs.length))
  const workers: (Worker | null)[] = Array.from({ length: lanes }, () => createWorker())
  let next = 0

  const consume = async (worker: Worker | null) => {
    while (next < jobs.length) {
      if (options.isCancelled?.()) return
      const job = jobs[next]!
      next += 1
      const analyzed = worker
        ? await runOnWorker(worker, job)
        : await analyzePhotoFile(job.file)
      await options.onResult(job.id, analyzed)
    }
  }

  try {
    await Promise.all(workers.map(worker => consume(worker)))
  } finally {
    for (const worker of workers) worker?.terminate()
  }
}
