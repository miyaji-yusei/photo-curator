import { describe, expect, it } from 'vitest'
import type { BurstPair } from '~/types/photo'
import {
  DEFAULT_THRESHOLD_OPTIONS,
  applyAnswer,
  createThresholdState,
  inferThreshold,
  nextPair,
  shouldStop,
  skipPair,
  sortPairsByDistance
} from '~/utils/burstThreshold'

/**
 * 実プロジェクト271枚から観測した隣接ペア99組の距離
 * （docs/progress/bench/decode-pair-decisions.csv の full_pair_distance）。
 * 2〜38 に連続的に広がっており、固定値 14 が最も密な領域の中央に来る。
 */
const REAL_DISTANCES = [
  10, 13, 16, 20, 26, 16, 3, 9, 17, 27, 32, 15, 7, 19, 5, 22, 10, 5, 11, 31,
  5, 14, 23, 10, 10, 17, 5, 31, 4, 25, 19, 14, 24, 27, 10, 6, 7, 5, 5, 4,
  9, 8, 7, 8, 19, 34, 5, 7, 19, 21, 4, 5, 7, 3, 3, 9, 12, 8, 2, 26,
  6, 20, 5, 30, 26, 15, 3, 10, 34, 21, 18, 26, 28, 17, 25, 21, 33, 12, 14, 12,
  11, 4, 29, 29, 12, 23, 33, 31, 10, 6, 14, 16, 25, 17, 38, 37, 5, 27, 11
]

function makePairs(distances: number[]): BurstPair[] {
  return distances.map((distance, index) => ({
    id: `pair-${index}`,
    leftPhotoId: `photo-${index}`,
    rightPhotoId: `photo-${index + 1}`,
    distance,
    gapMs: 1000
  }))
}

/** 閾値 `truth` を持つ利用者を模した自動回答で、収束するまで質問を回す。 */
function runSearch(pairs: BurstPair[], truth: number, options = DEFAULT_THRESHOLD_OPTIONS) {
  const sorted = sortPairsByDistance(pairs)
  let state = createThresholdState(sorted)
  const asked: number[] = []
  while (!shouldStop(sorted, state, options)) {
    const pair = nextPair(sorted, state)
    if (!pair) break
    asked.push(pair.distance)
    state = applyAnswer(sorted, state, pair, pair.distance <= truth)
  }
  return { state, asked, threshold: inferThreshold(state.answers, Math.max(...pairs.map(p => p.distance))) }
}

describe('二分探索による閾値の学習', () => {
  it('実データの分布で 8 問以内に真の閾値へ収束する', () => {
    const pairs = makePairs(REAL_DISTANCES)
    for (const truth of [5, 8, 11, 14, 20, 27]) {
      const { asked, threshold } = runSearch(pairs, truth)
      expect(asked.length).toBeLessThanOrEqual(DEFAULT_THRESHOLD_OPTIONS.maxQuestions)
      // dHash のブレを踏まえ ±2 に入っていれば十分とする。
      expect(Math.abs(threshold - truth)).toBeLessThanOrEqual(2)
    }
  })

  it('下限の質問数を満たすまで打ち切らない', () => {
    const pairs = makePairs(REAL_DISTANCES)
    const { asked } = runSearch(pairs, 14)
    expect(asked.length).toBeGreaterThanOrEqual(DEFAULT_THRESHOLD_OPTIONS.minQuestions)
  })

  it('同じペアを二度出題しない', () => {
    const pairs = makePairs(REAL_DISTANCES)
    const sorted = sortPairsByDistance(pairs)
    let state = createThresholdState(sorted)
    const seen: string[] = []
    while (!shouldStop(sorted, state, DEFAULT_THRESHOLD_OPTIONS)) {
      const pair = nextPair(sorted, state)
      if (!pair) break
      expect(seen).not.toContain(pair.id)
      seen.push(pair.id)
      state = applyAnswer(sorted, state, pair, pair.distance <= 14)
    }
    expect(seen.length).toBeGreaterThan(0)
  })

  it('スキップしたペアは再提示されない', () => {
    const sorted = sortPairsByDistance(makePairs(REAL_DISTANCES))
    let state = createThresholdState(sorted)
    const first = nextPair(sorted, state)
    expect(first).not.toBeNull()
    state = skipPair(state, first!)
    const second = nextPair(sorted, state)
    expect(second).not.toBeNull()
    expect(second!.id).not.toBe(first!.id)
  })

  it('候補ペアが無ければ出題しない', () => {
    const sorted = sortPairsByDistance([])
    const state = createThresholdState(sorted)
    expect(nextPair(sorted, state)).toBeNull()
    expect(shouldStop(sorted, state)).toBe(true)
  })
})

describe('inferThreshold', () => {
  it('矛盾した回答でも誤分類が最小の閾値を返す', () => {
    // 距離 8 で「別々」、距離 12 で「まとめる」という食い違いを含む。
    const answers = [
      { pairId: 'a', distance: 3, grouped: true },
      { pairId: 'b', distance: 6, grouped: true },
      { pairId: 'c', distance: 8, grouped: false },
      { pairId: 'd', distance: 12, grouped: true },
      { pairId: 'e', distance: 20, grouped: false },
      { pairId: 'f', distance: 26, grouped: false }
    ]
    const threshold = inferThreshold(answers, 38)
    // どこで切っても1件は外れる。多数派を説明できる 12〜19 の範囲に収まること。
    expect(threshold).toBeGreaterThanOrEqual(12)
    expect(threshold).toBeLessThan(20)
    const errors = answers.filter(a => (a.distance <= threshold) !== a.grouped).length
    expect(errors).toBe(1)
  })

  it('すべて「まとめる」なら観測された最大距離まで許容する', () => {
    const answers = [3, 10, 22].map((distance, index) => ({ pairId: `p${index}`, distance, grouped: true }))
    expect(inferThreshold(answers, 38)).toBe(38)
  })

  it('すべて「別々」ならまとめない', () => {
    const answers = [3, 10, 22].map((distance, index) => ({ pairId: `p${index}`, distance, grouped: false }))
    expect(inferThreshold(answers, 38)).toBe(0)
  })

  it('回答が無ければ既定値へ退避する', () => {
    expect(inferThreshold([], 38, 14)).toBe(14)
  })

  it('矛盾の無い回答なら境界の内側に入る', () => {
    const answers = [
      { pairId: 'a', distance: 4, grouped: true },
      { pairId: 'b', distance: 9, grouped: true },
      { pairId: 'c', distance: 15, grouped: false },
      { pairId: 'd', distance: 24, grouped: false }
    ]
    const threshold = inferThreshold(answers, 38)
    expect(threshold).toBeGreaterThanOrEqual(9)
    expect(threshold).toBeLessThan(15)
  })
})
