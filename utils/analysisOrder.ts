/**
 * 準備（解析）の処理順。選別は撮影時刻の昇順（古い順）で進むので、準備もその順で行う。
 * 撮影時刻が分かっているものを昇順（同時刻は相対パス）、まだ分からないものは最後に
 * **渡された順（取り込み順）のまま**並べる。PC の `ORDER BY captured_at IS NULL, captured_at, path`
 * と同じ規則（撮影時刻は解析の EXIF 読みで初めて分かることが多いので、分からないものは取り込み順）。
 * 元の配列は変えない。
 */
export interface OrderKey {
  capturedAt: number | null
  relativePath: string
}

export function orderForAnalysis<T>(items: readonly T[], keyOf: (item: T) => OrderKey): T[] {
  const known: { item: T, key: OrderKey }[] = []
  const unknown: T[] = []
  for (const item of items) {
    const key = keyOf(item)
    if (key.capturedAt === null) unknown.push(item)
    else known.push({ item, key })
  }
  known.sort((a, b) => {
    if (a.key.capturedAt !== b.key.capturedAt) return a.key.capturedAt! - b.key.capturedAt!
    return a.key.relativePath < b.key.relativePath ? -1 : a.key.relativePath > b.key.relativePath ? 1 : 0
  })
  return [...known.map(entry => entry.item), ...unknown]
}
