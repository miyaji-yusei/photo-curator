// 連写の基準を学習する質問（burst-threshold）を組み立てる。
// 「この2枚は同じ連写ですか」を最大8問。距離順に並べて等間隔に拾い、
// 境目のあたりを広く薄く聞く（設計 04 章）。
import type { PhotoRef } from '~/lib/core'
import { hashDistance } from '~/lib/core'

export interface BurstQuestion {
  left: PhotoRef
  right: PhotoRef
  distance: number
}

const MAX_QUESTIONS = 8
/** 学習に使うのは、時間が近い（連写の候補になり得る）隣どうしだけ。 */
const WINDOW_MS = 4000

export function buildBurstQuestions(photosInOrder: PhotoRef[]): BurstQuestion[] {
  const candidates: BurstQuestion[] = []
  for (let index = 1; index < photosInOrder.length; index += 1) {
    const left = photosInOrder[index - 1]!
    const right = photosInOrder[index]!
    if (left.captured_at === null || right.captured_at === null) continue
    if (Math.abs(left.captured_at - right.captured_at) > WINDOW_MS) continue
    if (!left.d_hash || !right.d_hash) continue
    const distance = hashDistance(left.d_hash, right.d_hash)
    candidates.push({ left, right, distance })
  }
  candidates.sort((a, b) => a.distance - b.distance)
  if (candidates.length <= MAX_QUESTIONS) return candidates

  // 等間隔に拾う。
  const picked: BurstQuestion[] = []
  const step = (candidates.length - 1) / (MAX_QUESTIONS - 1)
  for (let i = 0; i < MAX_QUESTIONS; i += 1) {
    picked.push(candidates[Math.round(i * step)]!)
  }
  return picked
}
