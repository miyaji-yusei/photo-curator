/**
 * 選別の途中を保存する形（封筒）と、core の星を写真の行へ写す計算。
 * **判断は持たない。** 判断は `lib/core.ts`（core）が行う。
 */
import type { BurstAnswer, Session } from '~/lib/core'
import { sessionFromJson } from '~/lib/core'
import type { SelectionStage, TournamentSettings } from '~/types/photo'
import type { BurstQuestion } from '~/utils/burstQuestions'

/** 連写の学習の途中。 */
export interface LearningState {
  questions: BurstQuestion[]
  index: number
  answers: BurstAnswer[]
}

/**
 * 保存する 1 かたまり。`v` が無い旧版の形は読まない（旧データは引き継がない）。
 * `core` は core の Session をそのまま持つ。
 */
export interface SavedSelection {
  v: 2
  core: Session
  /** 画面の段。 */
  stage: SelectionStage
  settings: TournamentSettings
  multiSelect: boolean
  /** relativePath */
  selectedInGroup: string[]
  learning: LearningState | null
  burstDistance: number | null
  updatedAt: number
}

const STAGES: SelectionStage[] = [
  'settings', 'burst-threshold', 'burst-preview', 'tournament', 'result', 'burst-final'
]

export function serializeSavedSelection(selection: SavedSelection): string {
  return JSON.stringify(selection)
}

/**
 * 読み戻す。旧版の形（`v` が無い）・壊れた JSON・core が読めない Session は null。
 * **呼ぶ前に core を初期化しておくこと**（`init()` / `initWithBytes()`）。
 */
export function parseSavedSelection(json: string): SavedSelection | null {
  let raw: unknown
  try {
    raw = JSON.parse(json)
  } catch {
    return null
  }
  if (!raw || typeof raw !== 'object') return null
  const value = raw as Partial<SavedSelection> & { core?: unknown }
  if (value.v !== 2 || !value.core || typeof value.core !== 'object') return null
  const core = sessionFromJson(JSON.stringify(value.core))
  if (!core) return null
  const settings = value.settings
  if (!settings || typeof settings.groupSize !== 'number') return null
  const stage = STAGES.includes(value.stage as SelectionStage) ? value.stage as SelectionStage : 'tournament'
  const inGroup = new Set(core.current)
  return {
    v: 2,
    core,
    stage,
    settings: { groupSize: settings.groupSize, groupBursts: !!settings.groupBursts },
    multiSelect: !!value.multiSelect,
    selectedInGroup: Array.isArray(value.selectedInGroup)
      ? value.selectedInGroup.filter(path => inGroup.has(path))
      : [],
    learning: value.learning && Array.isArray(value.learning.questions)
      ? {
          questions: value.learning.questions,
          index: Number(value.learning.index) || 0,
          answers: Array.isArray(value.learning.answers) ? value.learning.answers : []
        }
      : null,
    burstDistance: typeof value.burstDistance === 'number' ? value.burstDistance : null,
    updatedAt: Number(value.updatedAt) || 0
  }
}

/**
 * core の Session が変わったとき、**変わった星だけ**を写真の行へ書く形にする。
 *
 * - 前後の `ratings` を比べ、両方にあって値が違うものだけ。
 * - `ratings` に無い写真（このラウンドの外）は触らない。
 * - 前に無かった鍵（新しく入った写真）は比べる相手が無いので書かない。
 * - `undo` のときも同じ関数を使う。
 */
/** 行へ書く星。鍵は relativePath（行の id への変換は呼び出し側）。 */
export interface RatingChange {
  relativePath: string
  rating: number
}

export function syncRatings(previous: Session | null, next: Session): RatingChange[] {
  if (!previous) return []
  const changes: RatingChange[] = []
  for (const [path, rating] of Object.entries(next.ratings)) {
    const before = previous.ratings[path]
    if (before === undefined || before === rating) continue
    changes.push({ relativePath: path, rating })
  }
  // core の ratings は HashMap 由来で順が不定。書く順を決めておく。
  return changes.sort((left, right) => (left.relativePath < right.relativePath ? -1 : left.relativePath > right.relativePath ? 1 : 0))
}

/**
 * 開いたときの自己修復。組を確定した直後に強制終了すると、行の星だけ書けて Session が前の組のまま、
 * またはその逆になりうる。Session（封筒）を正として、食い違う行を Session の星に合わせる。
 *
 * `syncRatings` と同じ仕組み（前を「行の星」、後を「Session」として差分を取る）。
 * Session の `ratings` に無い写真は触らない。返すのは行の id と星。
 */
export function healRatings(
  rows: ReadonlyArray<{ id: string, relativePath: string, rating: number }>,
  session: Session
): Array<{ id: string, rating: number }> {
  const before = { ratings: Object.fromEntries(rows.map(row => [row.relativePath, row.rating])) } as Session
  const idOf = new Map(rows.map(row => [row.relativePath, row.id]))
  const entries: Array<{ id: string, rating: number }> = []
  for (const change of syncRatings(before, session)) {
    const id = idOf.get(change.relativePath)
    if (id) entries.push({ id, rating: change.rating })
  }
  return entries
}
