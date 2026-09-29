/**
 * Amazon の画像を並列で取る小さなプール（指紋作り・ZIP で共通）。
 *
 * **最初の `giveUpAfter` 件がぜんぶ取れなかったら打ち切る。** 中継のサーバーが無い
 * （静的配信）ときは、何枚叩いても取れない。4,000 枚ぶん空振りで叩かないための決め。
 * どちらの環境かは、事前に調べず、呼んだ成否で決める。
 */
export interface FetchPoolOptions<T> {
  items: T[]
  concurrency: number
  /** 最初のこの件数が全部失敗したら打ち切る。省略は打ち切らない。 */
  giveUpAfter?: number
  isCancelled?: () => boolean
  /** 取れなければ null（例外は投げない前提。投げたら失敗として数える）。 */
  fetch: (item: T) => Promise<Blob | null>
  /** 1 件ごと（取れたかどうかに関わらず）。 */
  onResult: (item: T, blob: Blob | null) => Promise<void> | void
}

export interface FetchPoolReport {
  attempted: number
  succeeded: number
  gaveUp: boolean
}

export async function fetchPool<T>(options: FetchPoolOptions<T>): Promise<FetchPoolReport> {
  const { items, concurrency, giveUpAfter, isCancelled, fetch, onResult } = options
  let attempted = 0
  let succeeded = 0

  const run = async (item: T) => {
    let blob: Blob | null = null
    try {
      blob = await fetch(item)
    } catch {
      blob = null
    }
    attempted += 1
    if (blob) succeeded += 1
    await onResult(item, blob)
  }

  // 先頭の `giveUpAfter` 件は、まとめて投げて結果を待つ。**1 件も取れなければ、ここで止める**
  // （5 件目以降は投げない）。
  const probe = giveUpAfter === undefined ? 0 : Math.min(giveUpAfter, items.length)
  if (probe > 0) {
    await Promise.all(items.slice(0, probe).map(run))
    if (succeeded === 0) return { attempted, succeeded, gaveUp: true }
  }

  let next = probe
  const worker = async () => {
    while (next < items.length && !isCancelled?.()) {
      const item = items[next]!
      next += 1
      await run(item)
    }
  }
  await Promise.all(Array.from({ length: Math.max(1, concurrency) }, worker))
  return { attempted, succeeded, gaveUp: false }
}
