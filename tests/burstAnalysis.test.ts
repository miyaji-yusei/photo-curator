import { describe, expect, it } from 'vitest'
import { BURST_WINDOW_MS, buildBurstGroups, buildBurstPairs, isEligiblePair } from '~/utils/burstAnalysis'
import type { BurstEntry } from '~/utils/burstAnalysis'
import type { TimestampSource } from '~/utils/captureTime'

/** dHash を 16 桁に整える小道具。下位ビットだけを使って距離を作る。 */
const hash = (bits: number) => bits.toString(16).padStart(16, '0')

const entry = (
  id: string, capturedAt: number, bits = 0, source: TimestampSource = 'exif_original'
): BurstEntry => ({ id, capturedAt, dHash: hash(bits), source })

describe('isEligiblePair', () => {
  it('時間の窓に収まっていれば候補', () => {
    expect(isEligiblePair(entry('a', 0), entry('b', BURST_WINDOW_MS))).toBe(true)
    expect(isEligiblePair(entry('a', 0), entry('b', BURST_WINDOW_MS + 1))).toBe(false)
  })

  it('順序が逆の組は候補にしない', () => {
    expect(isEligiblePair(entry('a', 1000), entry('b', 0))).toBe(false)
  })

  it('両方が更新時刻しか根拠を持たない組は落とす', () => {
    // コピーや展開で mtime は簡単に揃う。ここを通すと取り込んだぶんが
    // 丸ごと候補になってしまう。
    expect(isEligiblePair(entry('a', 0, 0, 'filesystem_mtime'), entry('b', 100, 0, 'filesystem_mtime'))).toBe(false)
    expect(isEligiblePair(entry('a', 0, 0, 'unknown'), entry('b', 100, 0, 'unknown'))).toBe(false)
  })

  it('片方でも強い根拠があれば候補にする', () => {
    expect(isEligiblePair(entry('a', 0, 0, 'filesystem_mtime'), entry('b', 100, 0, 'exif_original'))).toBe(true)
    expect(isEligiblePair(entry('a', 0, 0, 'filename_inferred'), entry('b', 100, 0, 'unknown'))).toBe(true)
  })
})

describe('buildBurstPairs', () => {
  it('隣り合う組だけを出し、距離と間隔を持たせる', () => {
    const pairs = buildBurstPairs([entry('a', 0, 0b0011), entry('b', 500, 0b0001)])
    expect(pairs).toHaveLength(1)
    expect(pairs[0]).toMatchObject({
      id: 'a:b', leftPhotoId: 'a', rightPhotoId: 'b', distance: 1, gapMs: 500
    })
  })

  it('距離では足切りしない（どこで切るかを決めるのが学習の仕事）', () => {
    // 距離 64 の真逆の組でも出題候補に残す。
    const pairs = buildBurstPairs([
      { id: 'a', capturedAt: 0, dHash: '0000000000000000', source: 'exif_original' },
      { id: 'b', capturedAt: 500, dHash: 'ffffffffffffffff', source: 'exif_original' }
    ])
    expect(pairs).toHaveLength(1)
    expect(pairs[0]!.distance).toBe(64)
  })

  it('窓の外の組は出さない', () => {
    const pairs = buildBurstPairs([entry('a', 0), entry('b', BURST_WINDOW_MS + 1), entry('c', BURST_WINDOW_MS + 2)])
    expect(pairs.map(pair => pair.id)).toEqual(['b:c'])
  })

  it('1 枚以下では組が作れない', () => {
    expect(buildBurstPairs([])).toEqual([])
    expect(buildBurstPairs([entry('a', 0)])).toEqual([])
  })
})

describe('buildBurstGroups', () => {
  it('連続するペアがすべて閾値を満たす区間を 1 グループにする', () => {
    const groups = buildBurstGroups([
      entry('a', 0, 0b0000), entry('b', 500, 0b0001), entry('c', 1000, 0b0011)
    ], 2)
    expect(groups).toHaveLength(1)
    expect(groups[0]!.photoIds).toEqual(['a', 'b', 'c'])
    expect(groups[0]!.capturedSpanMs).toBe(1000)
  })

  it('1 枚目はまとめず 2 と 3 枚目だけまとめる形も表せる', () => {
    // a と b は遠く、b と c は近い。区間の切れ目がペア単位で決まる。
    const groups = buildBurstGroups([
      entry('a', 0, 0b11111111), entry('b', 500, 0b0000), entry('c', 1000, 0b0001)
    ], 2)
    expect(groups).toHaveLength(1)
    expect(groups[0]!.photoIds).toEqual(['b', 'c'])
  })

  it('1 枚だけの区間はまとめない', () => {
    const groups = buildBurstGroups([
      entry('a', 0, 0b0000), entry('b', 60_000, 0b0000), entry('c', 120_000, 0b0000)
    ], 2)
    expect(groups).toEqual([])
  })

  it('閾値を上げるとまとまり、下げると分かれる', () => {
    const entries = [entry('a', 0, 0b0000), entry('b', 500, 0b1111)]
    expect(buildBurstGroups(entries, 4)).toHaveLength(1)
    expect(buildBurstGroups(entries, 3)).toHaveLength(0)
  })

  it('時間が離れていれば距離が近くても分ける', () => {
    const entries = [entry('a', 0, 0), entry('b', BURST_WINDOW_MS + 1, 0)]
    expect(buildBurstGroups(entries, 64)).toHaveLength(0)
  })

  it('類似度は代表からの平均距離から出す', () => {
    // 距離 0 が並べば 100%。
    expect(buildBurstGroups([entry('a', 0, 0), entry('b', 100, 0)], 2)[0]!.similarity).toBe(100)
    // 全ビット違う組は 0%。
    const opposite = buildBurstGroups([
      { id: 'a', capturedAt: 0, dHash: '0000000000000000', source: 'exif_original' },
      { id: 'b', capturedAt: 100, dHash: 'ffffffffffffffff', source: 'exif_original' }
    ], 64)
    expect(opposite[0]!.similarity).toBe(0)
  })

  it('最後の区間も取りこぼさない', () => {
    const groups = buildBurstGroups([
      entry('a', 0, 0), entry('b', 60_000, 0), entry('c', 60_500, 0)
    ], 2)
    expect(groups).toHaveLength(1)
    expect(groups[0]!.photoIds).toEqual(['b', 'c'])
  })
})
