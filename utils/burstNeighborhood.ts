/**
 * まとめを見直すときの「1 続きの写真」を、撮影順の全写真から切り出す。
 * **判断ではなく時刻で絞るだけ。**
 *
 * 選んだ写真が占める時間の幅（最小〜最大の撮影時刻）から前後 `windowMs` に
 * 入る写真を、渡された並びのまま返す。連写の判定に使えない写真
 * （撮影時刻か指紋が無い）は対象にしない。
 */
export interface NeighborhoodPhoto {
  relativePath: string
  capturedAt: number | null
  dHash: string | null
}

export function burstNeighborhood<T extends NeighborhoodPhoto>(
  photosInOrder: T[], selectedPaths: string[], windowMs = 4000
): T[] {
  if (!selectedPaths.length) return []
  const usable = photosInOrder.filter(photo => photo.capturedAt !== null && photo.dHash !== null)
  const target = new Set(selectedPaths)
  const times = usable.filter(photo => target.has(photo.relativePath)).map(photo => photo.capturedAt!)
  if (!times.length) return []
  const window = Math.max(0, windowMs)
  const low = Math.min(...times) - window
  const high = Math.max(...times) + window
  return usable.filter(photo => photo.capturedAt! >= low && photo.capturedAt! <= high)
}
