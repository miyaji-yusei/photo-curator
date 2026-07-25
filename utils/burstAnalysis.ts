/**
 * 連写の候補ペアとまとめを組む。デスクトップ（Rust の `build_burst_pairs` /
 * `build_burst_groups`）と同じ規則で、ブラウザ側でも同じ結果になるようにしてある。
 */
import type { BurstGroup, BurstPair } from '~/types/photo'
import type { TimestampSource } from '~/utils/captureTime'
import { isWeakSource } from '~/utils/captureTime'
import { hammingDistance } from '~/utils/dhash'

/** 連写と見なす時間の窓。 */
export const BURST_WINDOW_MS = 4_000
/** 学習前に使う既定のまとめ閾値。 */
export const HASH_DISTANCE_LIMIT = 14

/** 連写判定に必要な最小限。**撮影時刻の昇順**で渡す。 */
export interface BurstEntry {
  id: string
  capturedAt: number
  dHash: string
  source: TimestampSource
}

/**
 * 隣り合う 2 枚が候補になりうるか。
 *
 * 更新時刻しか根拠が無い組は落とす。コピーやダウンロードで簡単に揃うので、
 * ここを通すと一括取り込みしたぶんが丸ごと候補になってしまう。
 */
export function isEligiblePair(left: BurstEntry, right: BurstEntry, windowMs = BURST_WINDOW_MS) {
  const delta = right.capturedAt - left.capturedAt
  if (delta < 0 || delta > windowMs) return false
  if (isWeakSource(left.source) && isWeakSource(right.source)) return false
  return true
}

/**
 * 閾値学習の出題元。**距離での足切りはしない**
 * （どこで切るかを決めるのが学習の仕事なので、ここで絞ると学習できない）。
 */
export function buildBurstPairs(entries: BurstEntry[]): BurstPair[] {
  const pairs: BurstPair[] = []
  for (let index = 0; index + 1 < entries.length; index += 1) {
    const left = entries[index]!
    const right = entries[index + 1]!
    if (!isEligiblePair(left, right)) continue
    pairs.push({
      id: `${left.id}:${right.id}`,
      leftPhotoId: left.id,
      rightPhotoId: right.id,
      distance: hammingDistance(left.dHash, right.dHash),
      gapMs: right.capturedAt - left.capturedAt
    })
  }
  return pairs
}

/**
 * 閾値を当てて連写をまとめる。**連続するペアがすべて閾値を満たす区間**が 1 グループ。
 * これにより「1 枚目はまとめないが 2 と 3 枚目はまとめる」もそのまま表せる。
 * 1 枚だけの区間はまとめない。
 */
export function buildBurstGroups(entries: BurstEntry[], threshold: number): BurstGroup[] {
  const groups: BurstGroup[] = []
  let current: BurstEntry[] = []

  const flush = () => {
    if (current.length < 2) {
      current = []
      return
    }
    const first = current[0]!
    const last = current[current.length - 1]!
    const average = current
      .slice(1)
      .reduce((total, entry) => total + hammingDistance(first.dHash, entry.dHash), 0) /
      (current.length - 1)
    groups.push({
      id: `${first.id}:${last.id}`,
      photoIds: current.map(entry => entry.id),
      capturedSpanMs: last.capturedAt - first.capturedAt,
      similarity: Math.round(Math.max(0, 1 - average / 64) * 100),
      accepted: null
    })
    current = []
  }

  for (const entry of entries) {
    const previous = current[current.length - 1]
    const isNear = previous !== undefined &&
      entry.capturedAt - previous.capturedAt <= BURST_WINDOW_MS &&
      hammingDistance(entry.dHash, previous.dHash) <= threshold
    if (current.length > 0 && !isNear) flush()
    current.push(entry)
  }
  flush()
  return groups
}
