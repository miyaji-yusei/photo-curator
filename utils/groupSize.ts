/**
 * 1 グループに並べる枚数の既定と上限。**環境で変える。**
 *
 * デスクトップは広い画面とキーボードがあるので 10 枚まで一度に見比べられるが、
 * iPad は画面が狭く指で選ぶため、10 枚だと 1 枚が小さくなりすぎて選べない。
 * largeGroups が無い環境（Web ピッカー等）は 2–4 枚（05 章の表）。既定は 4 枚（2×2）。
 */
export interface GroupSizeLimits {
  /** 選別を始めるときの枚数。 */
  default: number
  /** 変更できる上限。 */
  max: number
  /** 比較にならないので 2 枚が下限。 */
  min: number
}

export const DESKTOP_GROUP_SIZE: GroupSizeLimits = { default: 10, max: 10, min: 2 }
/** largeGroups が無い環境（Web ピッカー等）で使う枚数。2–4 枚（05 章の表）。 */
export const TOUCH_GROUP_SIZE: GroupSizeLimits = { default: 4, max: 4, min: 2 }

/**
 * どちらの枚数を使うか。**画面の広さの能力だけで決める**
 * （`capabilities.largeGroups`）。画面幅やポインタの種類で判定すると、
 * タッチ対応のノート PC で意図せず枚数が変わってしまう。
 */
export function groupSizeLimits(largeGroups: boolean): GroupSizeLimits {
  return largeGroups ? DESKTOP_GROUP_SIZE : TOUCH_GROUP_SIZE
}

/**
 * 枚数を範囲に収める。保存済みのセッションが上限より大きい枚数を
 * 持っていることがあるので、再開時にも通す。
 */
export function clampGroupSize(size: number, limits: GroupSizeLimits): number {
  if (!Number.isFinite(size)) return limits.default
  return Math.min(limits.max, Math.max(limits.min, Math.round(size)))
}
