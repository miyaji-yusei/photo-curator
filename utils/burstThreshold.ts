import type { BurstPair, PairAnswer, ThresholdState } from '~/types/photo'

export interface ThresholdOptions {
  /** これ未満では打ち切らない。少なすぎる回答から閾値を決めない。 */
  minQuestions: number
  /** これを超えて質問しない。利用者の手間の上限。 */
  maxQuestions: number
  /**
   * 探索を打ち切る区間幅。dHash はデコード方式によって ±1〜5 ビットぶれるため
   * （docs/progress/bench/decode-hamming-histogram.csv）、これより細かく詰めても
   * 精度は上がらない。質問だけが増える。
   */
  resolution: number
}

export const DEFAULT_THRESHOLD_OPTIONS: ThresholdOptions = {
  minQuestions: 5,
  maxQuestions: 8,
  resolution: 2
}

/** dHash は 64bit なので距離の上限は 64。 */
export const MAX_HASH_DISTANCE = 64

/** 距離の昇順に並べる。同距離は id で安定させ、出題順を再現可能にする。 */
export function sortPairsByDistance(pairs: BurstPair[]): BurstPair[] {
  return [...pairs].sort((a, b) => a.distance - b.distance || a.id.localeCompare(b.id))
}

export function createThresholdState(sorted: BurstPair[]): ThresholdState {
  return { lowIdx: 0, highIdx: sorted.length - 1, answers: [], skipped: [] }
}

function askedIds(state: ThresholdState): Set<string> {
  return new Set([...state.answers.map(answer => answer.pairId), ...state.skipped])
}

/**
 * 次に見せるペアを返す。探索区間の中央を選び、既出・スキップ済みなら中央から
 * 外側へずらして未出題を探す。区間に未出題が無ければ null。
 */
export function nextPair(sorted: BurstPair[], state: ThresholdState): BurstPair | null {
  if (state.lowIdx > state.highIdx) return null
  const used = askedIds(state)
  const middle = Math.floor((state.lowIdx + state.highIdx) / 2)
  for (let offset = 0; offset <= state.highIdx - state.lowIdx; offset += 1) {
    for (const index of [middle - offset, middle + offset]) {
      if (index < state.lowIdx || index > state.highIdx) continue
      const pair = sorted[index]
      if (pair && !used.has(pair.id)) return pair
    }
  }
  return null
}

/**
 * 回答を取り込んで探索区間を狭める。
 * まとめる → 閾値はこの距離以上なので下端を上げる。
 * 別々 → 閾値はこの距離未満なので上端を下げる。
 */
export function applyAnswer(
  sorted: BurstPair[],
  state: ThresholdState,
  pair: BurstPair,
  grouped: boolean
): ThresholdState {
  const index = sorted.findIndex(candidate => candidate.id === pair.id)
  const answers = [...state.answers, { pairId: pair.id, distance: pair.distance, grouped }]
  if (index < 0) return { ...state, answers }
  return {
    ...state,
    answers,
    lowIdx: grouped ? Math.max(state.lowIdx, index + 1) : state.lowIdx,
    highIdx: grouped ? state.highIdx : Math.min(state.highIdx, index - 1)
  }
}

/** 判断できないペアは学習に使わず、二度と出題しない。区間は動かさない。 */
export function skipPair(state: ThresholdState, pair: BurstPair): ThresholdState {
  return { ...state, skipped: [...state.skipped, pair.id] }
}

/** 回答から導かれる「まとめる側の上端」と「別々側の下端」。 */
function evidenceBounds(answers: PairAnswer[]): { low: number, high: number } {
  let low = -1
  let high = MAX_HASH_DISTANCE + 1
  for (const answer of answers) {
    if (answer.grouped) low = Math.max(low, answer.distance)
    else high = Math.min(high, answer.distance)
  }
  return { low, high }
}

export function shouldStop(
  sorted: BurstPair[],
  state: ThresholdState,
  options: ThresholdOptions = DEFAULT_THRESHOLD_OPTIONS
): boolean {
  if (state.answers.length >= options.maxQuestions) return true
  if (!nextPair(sorted, state)) return true
  if (state.answers.length < options.minQuestions) return false
  const { low, high } = evidenceBounds(state.answers)
  // 両側から挟めていないうちは幅を測れない。
  if (low < 0 || high > MAX_HASH_DISTANCE) return false
  return high - low <= options.resolution
}

/**
 * 回答の誤分類数が最小になるしきい値を選ぶ。
 *
 * 二分探索の区間をそのまま採用しないのは、利用者の回答が矛盾しうるため。
 * 「距離 8 で別々、距離 12 でまとめる」のような回答でも、この方式なら
 * 最も多くの回答を説明できる位置に落ち着く。
 *
 * 最小誤分類が並ぶ（＝どこで切っても同じ）区間は中央を取る。ただし全回答が
 * 片側に寄っている場合だけは中央ではなく端に寄せる。全部「まとめる」なら
 * まとめ切る、全部「別々」ならまとめない、が利用者の意図に沿う。
 */
export function inferThreshold(
  answers: PairAnswer[],
  maxDistance: number = MAX_HASH_DISTANCE,
  fallback = 14
): number {
  if (!answers.length) return fallback
  const upper = Math.max(0, Math.min(maxDistance, MAX_HASH_DISTANCE))
  if (answers.every(answer => answer.grouped)) return upper
  if (answers.every(answer => !answer.grouped)) return 0

  let best = Number.POSITIVE_INFINITY
  let plateau: number[] = []
  for (let threshold = 0; threshold <= upper; threshold += 1) {
    let errors = 0
    for (const answer of answers) {
      if (answer.distance <= threshold !== answer.grouped) errors += 1
    }
    if (errors < best) {
      best = errors
      plateau = [threshold]
    } else if (errors === best) {
      plateau.push(threshold)
    }
  }
  return plateau[Math.floor(plateau.length / 2)] ?? fallback
}

/** 質問の進捗表示用。上限に対して何問目か。 */
export function questionProgress(
  state: ThresholdState,
  options: ThresholdOptions = DEFAULT_THRESHOLD_OPTIONS
): { asked: number, max: number } {
  return { asked: state.answers.length, max: options.maxQuestions }
}
