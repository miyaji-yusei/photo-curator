import type {
  BurstPair, SelectionResult, SelectionSeed, SelectionSession, TournamentSettings
} from '~/types/photo'
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
    version: SESSION_VERSION,
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
    burstSettled: [],
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
/**
 * いまのセッションの形の版。**形を変えたら上げる。**
 * 上げると、それ以前に保存されたセッションは読まずに捨てられる。
 */
export const SESSION_VERSION = 2

/** 読めるセッションか。版が合わなければ捨てる。 */
export function isCurrentSession(session: SelectionSession | null): boolean {
  return !!session && session.version === SESSION_VERSION
}

export function normalizeSession(session: SelectionSession): SelectionSession {
  // 後から足したフィールドは、既存セッションでは欠けている。
  const patched = {
    ...session,
    history: session.history ?? [],
    targetRating: session.targetRating ?? 0,
    burstMembers: session.burstMembers ?? {},
    burstSettled: session.burstSettled ?? []
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

/**
 * 候補を `groupSize` ずつ切ってラウンドを組む。
 *
 * **並べ替えはしない。** `candidates` は `get_selection_seed` が撮影順で返したもので、
 * その順序自体が「似た構図を隣り合わせる」という意味を持っている。
 * 以前はここで `seededShuffle` を掛けていたため、同じ場面の写真が別々のグループに
 * ばらけて比較にならなかった。
 *
 * 2ラウンド目以降も順序は保たれる。`survivors` はグループ順に push され、
 * `groups` は `candidates` 順に切られるため。
 */
export function prepareRound(session: SelectionSession) {
  const multiPhotoGroups: string[][] = []
  for (let index = 0; index < session.candidates.length; index += session.settings.groupSize) {
    const group = session.candidates.slice(index, index + session.settings.groupSize)
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
 * グループを確定するときに通す写真を決める。
 *
 * 明示的に選んだ写真（`selectedInGroup`）に加え、そのグループで★5に
 * **確定**した写真も必ず通す。確定は複数枚選択中にトグルで付けられるので、
 * 選択とは別に拾わないと、確定した写真が「選択なしで次へ」で落ちてしまう。
 */
export function resolveChosen(
  selectedInGroup: string[],
  group: string[],
  ratings: Record<string, number>,
  maxRating: number
): string[] {
  const confirmed = group.filter(id => (ratings[id] ?? 0) >= maxRating)
  return [...new Set([...selectedInGroup, ...confirmed])]
}

/**
 * 代表に付いた星を、まとめられた仲間にも配る。**変更した写真の id を返す。**
 *
 * 代表しか選別に出ないので、これをしないと仲間は選別前の星のまま取り残され、
 * 結果一覧では「見て落とした写真」と見分けが付かなくなる。
 *
 * 星は `+1` ではなく**代表と同じ値**にする。代表が★5で確定されたときは
 * `+1` では追いつかないため。
 *
 * **`burstSettled` に入っている写真は飛ばす。** まとめの中で★5に確定した
 * ものや、明らかに脱落として下げたものまで代表の星に揃えてしまうと、
 * わざわざ手で付けた判断がグループ確定の瞬間に消える。
 */
export function spreadBurstRatings(session: SelectionSession, chosen: string[]): string[] {
  const settled = new Set(session.burstSettled ?? [])
  const spread: string[] = []
  for (const id of chosen) {
    const members = session.burstMembers[id]
    if (!members) continue
    const rating = session.ratings[id] ?? 0
    for (const member of members) {
      if (member === id || settled.has(member)) continue
      session.ratings[member] = rating
      spread.push(member)
    }
  }
  return spread
}

/**
 * まとめから外した写真を、このラウンドの**まだ見ていない分**に混ぜる。
 *
 * `regroupRemaining` との違いは起点。あちらは `groupIndex` から詰め直すので
 * **いま見ているグループも並び替わる**。まとめを直している最中に目の前の
 * グループが変わってしまうため、ここでは次のグループから詰め直す。
 *
 * 外した写真は先頭に置く。撮影順では代表のすぐ隣にあり、その代表は今見ている
 * グループに居るので、次に見るのが自然な位置になる。
 */
export function insertIntoUpcoming(session: SelectionSession, ids: string[]): SelectionSession {
  const known = new Set(session.groups.flat())
  const added = ids.filter(id => !known.has(id))
  if (!added.length) return session

  const kept = session.groups.slice(0, session.groupIndex + 1)
  const pool = [...added, ...session.groups.slice(session.groupIndex + 1).flat()]
  const chunks: string[][] = []
  for (let index = 0; index < pool.length; index += session.settings.groupSize) {
    const chunk = pool.slice(index, index + session.settings.groupSize)
    // 1枚だけになったグループは比較にならないのでそのまま通す（prepareRound と同じ）。
    if (chunk.length > 1) chunks.push(chunk)
    else session.survivors.push(...chunk)
  }
  session.groups = [...kept, ...chunks]
  // `collapseBursts` が候補から外していたので、戻しておく。
  const inCandidates = new Set(session.candidates)
  session.candidates = [...session.candidates, ...added.filter(id => !inCandidates.has(id))]
  return session
}

/**
 * 連写の見直しで星を上げ下げする。**残す写真は+1、外した写真は−1。**
 *
 * 通常の選別（外しても下げない）とは意図的に変えてある。連写は星をそろえた
 * あとの絞り込みなので、下げられないとまとめの全員が高い星のまま残ってしまう。
 */
export function reviewBurstRatings(
  photos: { id: string, rating: number }[],
  keptIds: string[],
  maxRating: number
): SelectionResult[] {
  const kept = new Set(keptIds)
  return photos.map(photo => ({
    id: photo.id,
    rating: kept.has(photo.id)
      ? Math.min(maxRating, photo.rating + 1)
      : Math.max(0, photo.rating - 1)
  }))
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
