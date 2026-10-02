/**
 * core（`lib/core.ts`）へ渡す写真の列を作る。**並べ替えと写しだけで、判断はしない。**
 *
 * core が見る写真の鍵は `relativePath`。画面が持つ写真の行（`Photo`、`id` は uuid）とは
 * `relativePath → Photo` の Map で対応させる。
 */
import type { PhotoRef } from '~/lib/core'
import type { Photo } from '~/types/photo'

/** 連写と見なす時間の窓（core の `BurstThreshold.window_ms`）。 */
export const BURST_WINDOW_MS = 4000
/** 連写のハッシュ値（dHash）の版。core は版が違うものを比べない。 */
export const D_HASH_VERSION = 2
/** 学習していないときに使う距離（core `learn_distance` の fallback）。 */
export const DEFAULT_BURST_DISTANCE = 9

const compareText = (left: string, right: string) => (left < right ? -1 : left > right ? 1 : 0)

/**
 * 撮影順。撮影時刻の無いものは最後。同じならファイル名順（それも同じなら
 * `relativePath` 順で安定させる）。
 */
export function compareForCore(left: Photo, right: Photo): number {
  const leftMissing = left.capturedAt === null ? 1 : 0
  const rightMissing = right.capturedAt === null ? 1 : 0
  if (leftMissing !== rightMissing) return leftMissing - rightMissing
  if (left.capturedAt !== null && right.capturedAt !== null && left.capturedAt !== right.capturedAt) {
    return left.capturedAt - right.capturedAt
  }
  return compareText(left.name, right.name) || compareText(left.relativePath, right.relativePath)
}

export function sortForCore(photos: Photo[]): Photo[] {
  return [...photos].sort(compareForCore)
}

export function toPhotoRef(photo: Photo): PhotoRef {
  return {
    relative_path: photo.relativePath,
    captured_at: photo.capturedAt,
    d_hash: photo.dHash,
    d_hash_version: D_HASH_VERSION
  }
}

/** 全写真の行と、core に渡す列（撮影順）と、対応表。 */
export interface CoreInputs {
  /** 撮影順の行。 */
  photos: Photo[]
  /** `photos` と同じ並びの core の入力。 */
  refs: PhotoRef[]
  byPath: Map<string, Photo>
  byId: Map<string, Photo>
}

export function buildCoreInputs(rows: Photo[]): CoreInputs {
  const photos = sortForCore(rows)
  return {
    photos,
    refs: photos.map(toPhotoRef),
    byPath: new Map(photos.map(photo => [photo.relativePath, photo])),
    byId: new Map(photos.map(photo => [photo.id, photo]))
  }
}

/**
 * 連写の候補（時間が近い隣どうし）の中で一番遠い距離。確認画面のスライダーの上限。
 * 候補が無ければ `fallback`。距離の計算（`core.hashDistance`）は呼び出し側から渡す。
 */
export function maxNeighborDistance(
  refs: PhotoRef[],
  windowMs: number,
  distance: (left: string, right: string) => number,
  fallback = 64
): number {
  let max = -1
  for (let index = 1; index < refs.length; index += 1) {
    const left = refs[index - 1]!
    const right = refs[index]!
    if (left.captured_at === null || right.captured_at === null) continue
    if (Math.abs(left.captured_at - right.captured_at) > windowMs) continue
    if (!left.d_hash || !right.d_hash) continue
    max = Math.max(max, distance(left.d_hash, right.d_hash))
  }
  return max < 0 ? fallback : max
}
