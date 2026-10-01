/**
 * 1 本ずつ・入れた順に走らせる待ち行列。**入れるのは待たない。**
 *
 * `saveQueue`（封筒を最新の 1 つに畳む）と違い、入れたものは全部走らせる。選別の 1 タップごとの
 * 「行の星の書き込み」に使う（W1）。順序が入れ替わらないこと（前の書き込みが終わってから次を書く。
 * 「1 つ戻す」の書き込みが、戻される前の書き込みを追い越さない）が条件。
 *
 * - `enqueue(task)`: 待たずに戻る。`task` の失敗は `onError` へ渡して続ける
 * - `flush()`: 入っているもの（待っている間に足されたものも）が全部終わるまで待つ
 * - `afterDrain`: 行き先が空になったときに 1 回走る（集計の読み直しなど。連続して入れたぶんは 1 回に畳む）。
 *   `flush()` はこれの終わりまで待つ
 */
export interface SerialQueue {
  enqueue: (task: () => Promise<void>) => void
  flush: () => Promise<void>
  readonly busy: boolean
}

export function createSerialQueue(
  onError: (cause: unknown) => void,
  afterDrain?: () => Promise<void>
): SerialQueue {
  let tail: Promise<void> = Promise.resolve()
  let pending = 0
  return {
    enqueue(task) {
      pending += 1
      tail = tail.then(async () => {
        try {
          await task()
        } catch (cause) {
          onError(cause)
        }
        pending -= 1
        if (pending === 0 && afterDrain) {
          try {
            await afterDrain()
          } catch (cause) {
            onError(cause)
          }
        }
      })
    },
    async flush() {
      let seen: Promise<void>
      do {
        seen = tail
        await seen
      } while (seen !== tail)
    },
    get busy() {
      return pending > 0
    }
  }
}
