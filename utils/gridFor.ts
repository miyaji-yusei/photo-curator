// 選別画面の並べ方（行×列）を、写真を置く枠の形から決める純関数。
//
// 写真の縦横比（aspect）は呼び出し側が組の実際の値を渡す（省略時は 3:2）。各候補（行 r・列 c）で
// 1 マスに収まる写真の面積を出し、最も大きい並びを選ぶ。

/** 写真の縦横比（横 / 縦）の既定値。組の写真の実際の値が分かるまで（と、省略したとき）に使う。 */
export const PHOTO_ASPECT = 1.5
/** 最大の面積との差がこの割合未満なら、今の並びを保つ（窓を少し動かしても変わらないように）。 */
export const KEEP_TOLERANCE = 0.05

export interface GridShape {
  rows: number
  cols: number
}

/** 1 マスの大きさから、そこに収まる写真（3:2）の面積。 */
function photoArea(cellW: number, cellH: number, aspect: number) {
  if (cellW <= 0 || cellH <= 0) return 0
  return Math.min(cellW, cellH * aspect) * Math.min(cellH, cellW / aspect)
}

/**
 * count 枚を frameWidth × frameHeight の枠（マスの隙間 gap）に並べる行×列。
 * 候補は行 r = 1..count、列 c = ceil(count / r)。空きマス（r×c − count）が 1 行分（c）以上に
 * なる候補は捨てる。current があり、最大との差が 5% 未満なら current を返す。
 */
export function gridFor(
  count: number,
  frameWidth: number,
  frameHeight: number,
  gap: number,
  current?: GridShape | null,
  aspect: number = PHOTO_ASPECT
): GridShape {
  if (!(aspect > 0) || !Number.isFinite(aspect)) aspect = PHOTO_ASPECT
  if (!(count > 0)) return { rows: 1, cols: 1 }
  if (!(frameWidth > 0) || !(frameHeight > 0)) return { rows: 1, cols: count }

  const areaOf = (rows: number, cols: number) =>
    photoArea((frameWidth - gap * (cols - 1)) / cols, (frameHeight - gap * (rows - 1)) / rows, aspect)

  let best: GridShape = { rows: 1, cols: count }
  let bestArea = -1
  for (let rows = 1; rows <= count; rows++) {
    const cols = Math.ceil(count / rows)
    if (rows * cols - count >= cols) continue
    const area = areaOf(rows, cols)
    if (area > bestArea) {
      best = { rows, cols }
      bestArea = area
    }
  }

  // current が今の枚数で成り立つ並びで、差が小さければ保つ。
  if (
    current &&
    current.rows >= 1 && current.cols >= 1 &&
    current.rows * current.cols >= count &&
    current.rows * current.cols - count < current.cols &&
    bestArea > 0
  ) {
    const currentArea = areaOf(current.rows, current.cols)
    if (bestArea - currentArea < bestArea * KEEP_TOLERANCE) return { rows: current.rows, cols: current.cols }
  }
  return best
}
