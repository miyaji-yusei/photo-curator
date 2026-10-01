/**
 * 仮想化した格子で描く行の範囲（前後の余分を含む）。`VirtualPhotoGrid` が使う。
 * `first` と `last` は別々の computed にして使う（オブジェクトで返すと、毎フレーム新しい参照になり、
 * 行が変わっていなくても描画が走る。W14）。
 */
export interface RangeInput {
  rows: number
  scrolled: number
  viewport: number
  stride: number
  overscan: number
}

export function firstRow({ rows, scrolled, stride, overscan }: RangeInput): number {
  if (!rows) return 0
  const top = Math.max(0, Math.floor(scrolled / stride))
  return Math.max(0, Math.min(rows - 1, top) - overscan)
}

export function lastRow({ rows, scrolled, viewport, stride, overscan }: RangeInput): number {
  if (!rows) return -1
  const bottom = Math.max(0, Math.floor((scrolled + viewport) / stride))
  return Math.min(rows - 1, bottom + overscan)
}
