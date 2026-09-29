/**
 * 選別の途中（封筒）を書く、直列の待ち行列。**画面は保存を待たない。**
 *
 * - `enqueue(projectId, 封筒)`: 同じプロジェクトの未書き込みは**最新の 1 つだけ**残す（書き込み中のものは邪魔しない）
 * - 書き込みは 1 本ずつ。書いた順が入れ替わらない（前の書き込みが終わってから次を書く）
 * - `flush()`: 未書き込みも書き込み中も、全部書き終わるまで待つ
 * - 失敗したら `onError` へ渡す（画面の `error`）。その 1 件は捨てて、続きを書く
 *
 * 封筒は呼び出し側が「その時点の写し」を渡す（あとから書き換わらないように）。
 */
export interface SaveQueue<T> {
  enqueue: (projectId: string, envelope: T | null) => void
  flush: () => Promise<void>
  /** 未書き込み・書き込み中のものがあるか。 */
  readonly busy: boolean
}

export function createSaveQueue<T>(
  write: (projectId: string, envelope: T | null) => Promise<void>,
  onError: (cause: unknown) => void
): SaveQueue<T> {
  /** 挿入の順（Map は順序を保つ）。同じプロジェクトを入れ直すと最新の値に替わる（順は最初のまま）。 */
  const pending = new Map<string, T | null>()
  let running: Promise<void> | null = null

  async function drain() {
    while (pending.size) {
      const [projectId, envelope] = pending.entries().next().value as [string, T | null]
      pending.delete(projectId)
      try {
        await write(projectId, envelope)
      } catch (cause) {
        onError(cause)
      }
    }
  }

  function start() {
    if (running) return
    running = drain().finally(() => { running = null })
  }

  return {
    enqueue(projectId, envelope) {
      pending.set(projectId, envelope)
      start()
    },
    async flush() {
      // 書いている間に足されたものも待つ。
      while (running) await running
    },
    get busy() {
      return running !== null || pending.size > 0
    }
  }
}
