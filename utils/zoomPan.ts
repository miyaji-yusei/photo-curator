/**
 * 拡大の画面の倍率と移動量の計算（Android の `Zoom.kt` の scale / offset に当たる）。
 * 画面に触らない純関数だけを置く。
 *
 * 移動量 (x, y) は「写真の中心が、枠の中心からどれだけずれているか」（px）。
 */
export const ZOOM_MIN = 1
export const ZOOM_MAX = 8

export interface ZoomView {
  scale: number
  x: number
  y: number
}

export const ZOOM_RESET: ZoomView = { scale: 1, x: 0, y: 0 }

export function clampScale(scale: number): number {
  if (!Number.isFinite(scale)) return ZOOM_MIN
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, scale))
}

/** 写真が枠の外へ消えない範囲。倍率 1 なら動かせない。 */
export function clampPan(view: ZoomView, width: number, height: number): ZoomView {
  const limitX = Math.max(0, (width * (view.scale - 1)) / 2)
  const limitY = Math.max(0, (height * (view.scale - 1)) / 2)
  return {
    scale: view.scale,
    x: Math.min(limitX, Math.max(-limitX, view.x)),
    y: Math.min(limitY, Math.max(-limitY, view.y))
  }
}

/** ホイールの deltaY から掛ける倍率。1 目盛り（100）で約 ×1.16。上（負）が拡大。 */
export function wheelFactor(deltaY: number): number {
  const clamped = Math.min(100, Math.max(-100, deltaY))
  return Math.exp(-clamped * 0.0015)
}

/**
 * マウスの位置を中心に倍率を掛ける。拡大の前後でマウスの下の点が動かない。
 * (mx, my) は枠の中心からの位置（px）。
 */
export function zoomAt(view: ZoomView, factor: number, mx: number, my: number, width: number, height: number): ZoomView {
  const scale = clampScale(view.scale * factor)
  if (scale === ZOOM_MIN) return { ...ZOOM_RESET }
  const ratio = scale / view.scale
  return clampPan(
    { scale, x: mx - (mx - view.x) * ratio, y: my - (my - view.y) * ratio },
    width,
    height
  )
}

/** ドラッグで動かす。倍率 1 では動かさない。 */
export function panBy(view: ZoomView, dx: number, dy: number, width: number, height: number): ZoomView {
  if (view.scale <= ZOOM_MIN) return { ...ZOOM_RESET }
  return clampPan({ scale: view.scale, x: view.x + dx, y: view.y + dy }, width, height)
}

/** 「2.5倍」。1 倍のときは出さない（空文字）。Android の表示に合わせる。 */
export function zoomLabel(scale: number): string {
  return scale > 1.01 ? `${scale.toFixed(1)}倍` : ''
}
