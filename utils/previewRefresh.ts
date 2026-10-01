/**
 * 準備（背景の解析）の途中で、プロジェクト画面の格子を更新するときの計画（W11）。
 *
 * 以前は 3 秒ごとに「いま読んでいる枚数ぶん」を先頭から全部読み直していた（2,000 枚読んでいれば
 * 2,000 行のサムネイルを毎回読み、格子を全置換する）。サムネイルがまだ無い行だけを読み直して、
 * その場で置き換える。準備が終わったときの更新は従来どおり全件の読み直し。
 */
import type { Photo } from '~/types/photo'

export type PreviewRefreshPlan = { kind: 'full' } | { kind: 'ids', ids: string[] }

/**
 * 一覧が「先頭ページ分まだ読めていない」（総数が 0・先頭ページに満たない）ときは、行そのものが
 * 増えているかもしれないので全件の読み直し。そうでなければ、サムネイルの無い行の id だけ。
 */
export function previewRefreshPlan(photos: readonly Photo[], total: number, pageSize: number): PreviewRefreshPlan {
  if (total === 0 || photos.length < Math.min(pageSize, total)) return { kind: 'full' }
  return { kind: 'ids', ids: photos.filter(photo => !photo.thumbnailPath).map(photo => photo.id) }
}

/**
 * 読み直した行のうち、サムネイルが付いたものだけを同じ位置で置き換える。
 * 置き換えるものが無ければ null（一覧の参照を変えず、再描画しない）。
 */
export function withNewThumbnails(photos: readonly Photo[], fresh: readonly Photo[]): Photo[] | null {
  const ready = new Map<string, Photo>()
  for (const photo of fresh) if (photo.thumbnailPath) ready.set(photo.id, photo)
  if (!ready.size) return null
  let changed = false
  const next = photos.map(photo => {
    const found = photo.thumbnailPath ? undefined : ready.get(photo.id)
    if (!found) return photo
    changed = true
    return found
  })
  return changed ? next : null
}
