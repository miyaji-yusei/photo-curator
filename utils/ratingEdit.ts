/**
 * **人が星を直接決める手直し**だけを置く純関数。
 *
 * トーナメントの判断（確定・1 つ戻す・★5・組み直し・次のラウンド）は core が行う。
 * ここは、まとめの中の 1 枚に星を付ける・連写の見直し・星の一括移動のように、
 * 人が「この写真は何星」と決める操作だけ。`Session.ratings` を書き換えてよいのは
 * このファイルの関数だけ（呼び出し側で直接書かない）。
 *
 * 元の Session は変えず、新しい Session を返す。星は必ず 0..5 に丸める。
 */
import type { Session } from '~/lib/core'

export const MIN_STAR = 0
export const MAX_STAR = 5

export function clampStar(value: number): number {
  if (!Number.isFinite(value)) return MIN_STAR
  return Math.min(MAX_STAR, Math.max(MIN_STAR, Math.round(value)))
}

/** 1 枚の星を決める。まとまりの仲間には触らない。 */
export function setRating(session: Session, path: string, star: number): Session {
  return { ...session, ratings: { ...session.ratings, [path]: clampStar(star) } }
}

/** 何枚かの星を決める（値は絶対値）。 */
export function applyChanges(session: Session, changes: Record<string, number>): Session {
  const ratings = { ...session.ratings }
  for (const [path, star] of Object.entries(changes)) ratings[path] = clampStar(star)
  return { ...session, ratings }
}

/**
 * 連写の見直しの結果。**残す写真は +1、外した写真は −1**（0..5 に丸める）。
 * 通常の選別と違って下げるのは、ここが「星をそろえたあとの絞り込み」だから。
 * 戻り値は `applyChanges` に渡す絶対値。
 */
export function reviewChanges(
  session: Session, shownPaths: string[], keptPaths: string[]
): Record<string, number> {
  const kept = new Set(keptPaths)
  const changes: Record<string, number> = {}
  for (const path of shownPaths) {
    const star = session.ratings[path] ?? 0
    changes[path] = clampStar(kept.has(path) ? star + 1 : star - 1)
  }
  return changes
}

/** 星の一括移動。Session に無い写真は飛ばす。 */
export function moveRatings(session: Session, paths: string[], to: number): Session {
  const ratings = { ...session.ratings }
  const star = clampStar(to)
  for (const path of paths) {
    if (path in ratings) ratings[path] = star
  }
  return { ...session, ratings }
}

/** Session の中で、その星ちょうどの写真。 */
export function pathsWithRating(session: Session, star: number): string[] {
  return Object.entries(session.ratings)
    .filter(([, value]) => value === star)
    .map(([path]) => path)
}
