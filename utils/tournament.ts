import type { BurstPair, SelectionSeed, SelectionSession, TournamentSettings } from '~/types/photo'
import { createThresholdState, sortPairsByDistance } from '~/utils/burstThreshold'

const nextRandom = (seed: number) => {
  let value = seed + 0x6D2B79F5
  value = Math.imul(value ^ (value >>> 15), value | 1)
  value ^= value + Math.imul(value ^ (value >>> 7), value | 61)
  return [(value ^ (value >>> 14)) >>> 0, value >>> 0] as const
}

export function seededShuffle<T>(items: T[], seed: number): { items: T[], seed: number } {
  const shuffled = [...items]
  let state = seed >>> 0
  for (let index = shuffled.length - 1; index > 0; index--) {
    const [random, nextSeed] = nextRandom(state)
    state = nextSeed
    const swapIndex = random % (index + 1)
    ;[shuffled[index], shuffled[swapIndex]] = [shuffled[swapIndex]!, shuffled[index]!]
  }
  return { items: shuffled, seed: state }
}

/**
 * 開始時の行き先を決める。
 * - 連写をまとめない設定 / 候補ペアが無い → そのままトーナメント
 * - 学習済みの閾値がある → 質問を飛ばして確認画面へ
 * - それ以外 → 閾値の学習から
 */
export function makeSession(
  projectId: string,
  photos: SelectionSeed[],
  settings: TournamentSettings,
  burstPairs: BurstPair[] = [],
  learnedThreshold: number | null = null,
  targetRating = 0
): SelectionSession {
  const seed = (Date.now() ^ Math.floor(Math.random() * 0xffffffff)) >>> 0
  const sorted = sortPairsByDistance(burstPairs)
  const usesBursts = settings.groupBursts && sorted.length > 0
  return {
    projectId,
    settings,
    candidates: photos.map(photo => photo.id),
    groups: [],
    groupIndex: 0,
    selectedInGroup: [],
    multiSelect: false,
    ratings: Object.fromEntries(photos.map(photo => [photo.id, photo.rating])),
    survivors: [],
    history: [],
    targetRating,
    burstMembers: {},
    round: 1,
    burstGroups: [],
    burstPairs: sorted,
    burstThresholdState: createThresholdState(sorted),
    burstThreshold: usesBursts ? learnedThreshold : null,
    stage: !usesBursts
      ? 'tournament'
      : learnedThreshold === null
        ? 'burst-threshold'
        : 'burst-preview',
    updatedAt: Date.now(),
    seed
  }
}

/**
 * 保存済みセッションを現在の形へ寄せる。閾値学習を入れる前に保存された JSON は
 * burstPairs を持たない。連写まわりだけ初期化し、rating と survivors は残す。
 */
export function normalizeSession(session: SelectionSession): SelectionSession {
  // 後から足したフィールドは、既存セッションでは欠けている。
  const patched = {
    ...session,
    history: session.history ?? [],
    targetRating: session.targetRating ?? 0,
    burstMembers: session.burstMembers ?? {}
  }
  if (Array.isArray(session.burstPairs) && session.burstThresholdState) return patched
  session = patched
  const stage = session.stage === 'tournament' || session.stage === 'result' ? session.stage : 'tournament'
  return {
    ...session,
    burstGroups: session.burstGroups ?? [],
    burstPairs: [],
    burstThresholdState: createThresholdState([]),
    burstThreshold: null,
    history: session.history ?? [],
    stage
  }
}

/**
 * まとめた連写を代表1枚に畳み込む。`session.burstMembers` を作り、
 * 代表でないメンバーを候補から外す。以降トーナメントには代表だけが出る。
 */
export function collapseBursts(session: SelectionSession) {
  const members: Record<string, string[]> = {}
  const hidden = new Set<string>()
  const inCandidates = new Set(session.candidates)
  for (const group of session.burstGroups) {
    // 候補に残っているメンバーだけでまとめ直す。前のラウンドで落ちた写真は含めない。
    const alive = group.photoIds.filter(id => inCandidates.has(id))
    if (alive.length < 2) continue
    const [representative, ...rest] = alive
    members[representative!] = alive
    for (const id of rest) hidden.add(id)
  }
  session.burstMembers = members
  session.candidates = session.candidates.filter(id => !hidden.has(id))
  return session
}

/** 代表写真を差し替える。まとめの中身は変えない。 */
export function setBurstRepresentative(session: SelectionSession, currentId: string, nextId: string) {
  const members = session.burstMembers[currentId]
  if (!members || !members.includes(nextId) || currentId === nextId) return session
  delete session.burstMembers[currentId]
  session.burstMembers[nextId] = [nextId, ...members.filter(id => id !== nextId)]

  const replace = (ids: string[]) => ids.map(id => (id === currentId ? nextId : id))
  session.candidates = replace(session.candidates)
  session.groups = session.groups.map(replace)
  session.selectedInGroup = replace(session.selectedInGroup)
  session.survivors = replace(session.survivors)
  return session
}

export function prepareRound(session: SelectionSession) {
  const shuffled = seededShuffle(session.candidates, session.seed ?? Date.now())
  session.seed = shuffled.seed
  const multiPhotoGroups: string[][] = []
  for (let index = 0; index < shuffled.items.length; index += session.settings.groupSize) {
    const group = shuffled.items.slice(index, index + session.settings.groupSize)
    if (group.length > 1) multiPhotoGroups.push(group)
    else session.survivors.push(...group)
  }
  session.groups = multiPhotoGroups
  session.groupIndex = 0
  session.selectedInGroup = []
  session.multiSelect = false
  session.history = []
  session.stage = 'tournament'
  return session
}

/**
 * 選別の途中で1グループの表示枚数を変える。
 *
 * 済んだグループはそのまま残し、**まだ見ていない写真だけ**を新しい枚数で
 * 詰め直す。`groupIndex` が動かないので、戻る履歴もそのまま使える。
 */
export function regroupRemaining(session: SelectionSession, groupSize: number) {
  const done = session.groups.slice(0, session.groupIndex)
  const remaining = session.groups.slice(session.groupIndex).flat()
  const chunks: string[][] = []
  for (let index = 0; index < remaining.length; index += groupSize) {
    const chunk = remaining.slice(index, index + groupSize)
    // 1枚だけになったグループは比較にならないのでそのまま通す。
    if (chunk.length > 1) chunks.push(chunk)
    else session.survivors.push(...chunk)
  }
  session.settings = { ...session.settings, groupSize }
  session.groups = [...done, ...chunks]
  session.selectedInGroup = []
  session.multiSelect = false
  return session
}

/**
 * 直前の1グループぶんの判断を取り消す。
 * survivors とレーティングを差分で戻すので、履歴は小さいまま。
 */
export function undoLastStep(session: SelectionSession): boolean {
  const step = session.history.pop()
  if (!step) return false
  for (const id of step.chosen) {
    const index = session.survivors.lastIndexOf(id)
    if (index >= 0) session.survivors.splice(index, 1)
    const rating = session.ratings[id] ?? 0
    session.ratings[id] = Math.max(0, rating - 1)
  }
  session.groupIndex = step.groupIndex
  session.selectedInGroup = []
  session.multiSelect = false
  session.stage = 'tournament'
  return true
}
