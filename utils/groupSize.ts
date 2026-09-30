/**
 * 1 グループに並べる枚数の既定と上限。**環境で変える。**
 *
 * デスクトップは広い画面とキーボードがあるので 10 枚まで一度に見比べられるが、
 * iPad は画面が狭く指で選ぶため、10 枚だと 1 枚が小さくなりすぎて選べない。
 * 9 枚（3×3）を上限に、既定は 4 枚（2×2）にする。
 */
export interface GroupSizeLimits {
  /** 選別を始めるときの枚数。 */
  default: number
  /** 変更できる上限。 */
  max: number
  /** 比較にならないので 2 枚が下限。 */
  min: number
}

/**
 * スライドショー選別は「1 グループが 1 枚」で表す（セッションの枚数設定と同じ仕組み）。
 * 中身のモデル（core の Session）は変えない。
 */
export const SLIDESHOW_GROUP_SIZE = 1
export const isSlideshowSize = (size: number) => size === SLIDESHOW_GROUP_SIZE

export const DESKTOP_GROUP_SIZE: GroupSizeLimits = { default: 10, max: 10, min: 2 }
/** iPad などブラウザで使うときの枚数。3×3 までに抑える。 */
export const TOUCH_GROUP_SIZE: GroupSizeLimits = { default: 4, max: 9, min: 2 }

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

/**
 * トーナメントの枚数スライダーに出す値。スライドショー（1 枚）のセッションでは
 * スライダーの下限（2 枚）を下回るので、既定の枚数に落とす。
 */
export function tournamentGroupSize(size: number, limits: GroupSizeLimits): number {
  return isSlideshowSize(size) ? limits.default : clampGroupSize(size, limits)
}
