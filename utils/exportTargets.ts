import { collapseBursts } from '~/utils/collapseBursts'

/**
 * 書き出し（ZIP・CSV・フォルダ分け・XMP・共有）の対象を、結果の行から作る。
 *
 * 結果画面の 1 行は、連写なら**代表 1 枚**で、星はその組で一番高いもの。
 * 書き出しでは仲間も出す。ただし**連写の中身を選別したものは、選んだものだけ**。
 *
 * 規則は 1 つ: **行の星と同じ星の仲間だけ出す。**
 * - 中身を選別していない組は、仲間の星が代表と必ず揃っている（core の
 *   `shift_star`）ので、全員出る
 * - 選別した組は、選んだ分だけが 1 つ上がり、選ばなかった分は据え置き
 *   （BurstReviewView）なので、選んだものだけが行の星（＝一番高い星）と揃う
 * - 選んだ結果が全員になれば、また全員揃うので全員出る
 */
export interface ExportRow {
  /** 行に出している 1 枚。 */
  relativePath: string
  /** 行の星（連写ならその組で一番高い星）。 */
  rating: number
  /** 行に含まれる写真の全部（連写の仲間を含む。単独なら自分だけ）。 */
  mates: string[]
}

export interface ExportTarget {
  relativePath: string
  rating: number
  capturedAt: number | null
}

export function expandExportTargets(
  rows: ExportRow[],
  ratings: Record<string, number>,
  capturedAtOf: (relativePath: string) => number | null
): ExportTarget[] {
  const seen = new Set<string>()
  const out: ExportTarget[] = []
  for (const row of rows) {
    for (const path of row.mates) {
      if (seen.has(path)) continue
      const rating = ratings[path] ?? 0
      if (rating !== row.rating) continue
      seen.add(path)
      out.push({ relativePath: path, rating, capturedAt: capturedAtOf(path) })
    }
  }
  return out
}

export interface ExportablePhoto {
  relativePath: string
  rating: number
  capturedAt: number | null
}

/**
 * 星で絞った書き出しの対象。星ごとに、その星の写真を連写ごとに畳んだ行にし、
 * `expandExportTargets` で仲間まで広げる（＝その 1 枚自身の星がその星のものだけ）。
 * 星は大きい順に並べる。`stars` が空なら何も返さない。
 */
export function exportTargetsForStars(
  photos: readonly ExportablePhoto[],
  members: Record<string, string[]> | null | undefined,
  stars: readonly number[]
): ExportTarget[] {
  const ratings: Record<string, number> = {}
  const capturedAt = new Map<string, number | null>()
  for (const photo of photos) {
    ratings[photo.relativePath] = photo.rating
    capturedAt.set(photo.relativePath, photo.capturedAt)
  }
  const out: ExportTarget[] = []
  for (const star of [...new Set(stars)].sort((left, right) => right - left)) {
    const rows: ExportRow[] = collapseBursts(photos.filter(photo => photo.rating === star), members)
      .map(tile => ({ relativePath: tile.photo.relativePath, rating: star, mates: tile.mates }))
    out.push(...expandExportTargets(rows, ratings, path => capturedAt.get(path) ?? null))
  }
  return out
}
