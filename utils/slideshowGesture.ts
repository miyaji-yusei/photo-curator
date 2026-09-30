/**
 * スライドショー選別の操作の判定。**見た目と入力の解釈だけ**で、残す・落とすの判断は
 * ここでしない（決まった操作は core の `advance`・`keepAndTop` へ渡す）。
 *
 * 操作: 左＝落とす／右＝残す／上＝★5 で確定。下は使わない。
 */
export type SlideDecision = 'drop' | 'keep' | 'top'

/** これ未満の動きはドラッグでなくクリック（タップ）として扱う。 */
export const TAP_SLOP = 8

/** 決定になるドラッグ量（px）。幅の 18%、ただし 60〜160px の範囲。 */
export function decideThreshold(width: number): number {
  if (!Number.isFinite(width)) return 60
  return Math.min(160, Math.max(60, width * 0.18))
}

export function isTap(dx: number, dy: number): boolean {
  return Math.hypot(dx, dy) < TAP_SLOP
}

/** ドラッグを離したときの判定。量が足りない・下向きなら null（元へ戻す）。 */
export function judgeDrag(dx: number, dy: number, width: number): SlideDecision | null {
  const threshold = decideThreshold(width)
  const ax = Math.abs(dx)
  const ay = Math.abs(dy)
  if (dy < 0 && ay > ax && ay >= threshold) return 'top'
  if (ax >= threshold && ax >= ay) return dx < 0 ? 'drop' : 'keep'
  return null
}

export interface DragFeedback {
  /** いま出す色の膜・アイコン・文字。 */
  direction: SlideDecision | null
  /** 0〜1。決定になるドラッグ量で 1。 */
  strength: number
  /** 写真の回転（度）。 */
  rotation: number
}

/** ドラッグ中の見た目。決定の向きと同じ規則（縦が優位で上向きなら「上」、横が優位なら左右）。 */
export function dragFeedback(dx: number, dy: number, width: number): DragFeedback {
  const threshold = decideThreshold(width)
  const ax = Math.abs(dx)
  const ay = Math.abs(dy)
  const span = Math.max(1, width)
  const rotation = Math.max(-15, Math.min(15, (dx / span) * 30))
  if (dy < 0 && ay > ax) return { direction: 'top', strength: Math.min(1, ay / threshold), rotation }
  if (ax > 0 && ax >= ay) return { direction: dx < 0 ? 'drop' : 'keep', strength: Math.min(1, ax / threshold), rotation }
  return { direction: null, strength: 0, rotation }
}

/** 画面の左半分のクリックは「落とす」、右半分は「残す」。 */
export function tapDecision(clientX: number, left: number, width: number): SlideDecision {
  return clientX < left + width / 2 ? 'drop' : 'keep'
}

export interface KeyLike {
  key: string
  code?: string
  ctrlKey?: boolean
  metaKey?: boolean
  shiftKey?: boolean
  altKey?: boolean
}

/** `1`＝落とす・`2`＝残す。修飾キー付きは別の操作（拡大など）なので拾わない。 */
export function slideKeyDecision(event: KeyLike): SlideDecision | null {
  if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return null
  const digit = /^(Digit|Numpad)(\d)$/.exec(event.code ?? '')?.[2] ?? event.key
  if (digit === '1') return 'drop'
  if (digit === '2') return 'keep'
  return null
}

/** 決定したときに写真が飛んでいく先（枠の外）。 */
export function flyTarget(decision: SlideDecision, width: number, height: number) {
  if (decision === 'top') return { x: 0, y: -height * 1.2, rotation: 0 }
  const sign = decision === 'drop' ? -1 : 1
  return { x: sign * width * 1.2, y: 0, rotation: sign * 20 }
}

/** 縦横比を保って枠に収まる最大の大きさ（枠より小さい写真は拡大する）。 */
export function fitContain(naturalWidth: number, naturalHeight: number, frameWidth: number, frameHeight: number) {
  if (!(naturalWidth > 0) || !(naturalHeight > 0) || !(frameWidth > 0) || !(frameHeight > 0)) {
    return { width: Math.max(0, frameWidth), height: Math.max(0, frameHeight) }
  }
  const scale = Math.min(frameWidth / naturalWidth, frameHeight / naturalHeight)
  return { width: naturalWidth * scale, height: naturalHeight * scale }
}
