/**
 * 一覧と選別対象の絞り込み・並べ替え。デスクトップでは SQL
 * （`photo_page_query` / `SELECTION_SEED_ORDER`）が担っている役割を、
 * ブラウザ側では**純粋な関数**として持つ。
 *
 * SQL のままだとテストできないが、ここに置けば Node 上で検証できる。
 */
import { MAX_RATING } from '~/types/photo'
import type { PhotoSort, SelectionSummary } from '~/types/photo'

/** 並べ替えと絞り込みに必要な最小限。 */
export interface QueryablePhoto {
  relativePath: string
  rating: number
  capturedAt: number | null
  isMissing?: boolean
}

/** 文字列の比較は SQLite の既定（バイト順）に寄せる。 */
const compareText = (left: string, right: string) => (left < right ? -1 : left > right ? 1 : 0)

/**
 * 一覧の並び。`rating` は星の高い順で、同点はファイル順にして安定させる。
 * それ以外はファイル順。
 */
export function comparePhotos<T extends QueryablePhoto>(sort: PhotoSort): (left: T, right: T) => number {
  if (sort === 'rating') {
    return (left, right) =>
      right.rating - left.rating || compareText(left.relativePath, right.relativePath)
  }
  return (left, right) => compareText(left.relativePath, right.relativePath)
}

/**
 * 選別に出す順。**撮影順**で、撮影日時の無い写真は末尾に回し、
 * その中はファイル順で安定させる。似た構図は撮影時刻が近いので、
 * この並びのままグループに切ると「同じ場面から 1 枚選ぶ」比較になる。
 */
export function compareByCaptureOrder<T extends QueryablePhoto>(left: T, right: T): number {
  const leftMissing = left.capturedAt === null ? 1 : 0
  const rightMissing = right.capturedAt === null ? 1 : 0
  if (leftMissing !== rightMissing) return leftMissing - rightMissing
  if (left.capturedAt !== null && right.capturedAt !== null && left.capturedAt !== right.capturedAt) {
    return left.capturedAt - right.capturedAt
  }
  return compareText(left.relativePath, right.relativePath)
}

/**
 * 絞り込みは**星ちょうど一致**だけ。星が選別状態そのものなので、
 * これ以外の区分を持たない。`rating` が null / undefined なら全件。
 * 見失った写真は常に除く。
 */
export function filterPhotos<T extends QueryablePhoto>(photos: T[], rating?: number | null): T[] {
  return photos.filter(photo => {
    if (photo.isMissing) return false
    return rating === null || rating === undefined ? true : photo.rating === rating
  })
}

/** 1 ページぶんを切り出す。`limit` は 1 件以上に丸める。 */
export function pagePhotos<T>(photos: T[], offset: number, limit: number): T[] {
  const start = Math.max(0, offset)
  return photos.slice(start, start + Math.max(1, limit))
}

/** 星ごとの枚数。`counts[n]` が★n の枚数。 */
export function summarizeRatings<T extends QueryablePhoto>(photos: T[]): SelectionSummary {
  const counts = new Array<number>(MAX_RATING + 1).fill(0)
  let total = 0
  for (const photo of photos) {
    if (photo.isMissing) continue
    const rating = Math.min(MAX_RATING, Math.max(0, photo.rating))
    counts[rating] = (counts[rating] ?? 0) + 1
    total += 1
  }
  return { counts, total }
}
