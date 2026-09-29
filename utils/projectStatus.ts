// ホームの行・サイドバーの点・プロジェクトの画面の準備の進みが使う、プロジェクトの状態。
// **出所（網）へ問い合わせない。** 端末に置いたもの（Project の行・未処理の数・選別の途中）だけから決める。
// 文言は新版の `projectStatus`（設計 02 章の 5 状態）と同じ。
import type { Session } from '~/lib/core'
import type { Project } from '~/types/photo'

export type CardState = 'new' | 'preparing' | 'culling' | 'done' | 'error'

export interface CardStatus {
  state: CardState
  /** 名前の下の 1 行。 */
  statusText: string
  /** 次の一手のボタン。 */
  actionLabel: string
  actionEnabled: boolean
}

/** 状態の判定に要る、端末にある値。 */
export interface StatusFacts {
  project: Pick<Project, 'status' | 'photoCount' | 'sourceKind'>
  /** まだ撮影時刻・サムネイルの解析が要る枚数。 */
  analysisBacklog: number
  /** まだ表示用画像が要る枚数。 */
  displayBacklog: number
  /** 選別の途中（core の Session）。無ければ null。 */
  session: Pick<Session, 'target_star' | 'round' | 'finished'> | null
  /** ★1 以上の枚数（行の星から数える）。 */
  keptCount: number
}

export function projectStatus(facts: StatusFacts): CardStatus {
  const { project, session } = facts
  if (project.status === 'missing') {
    const reason = project.sourceKind === 'amazon'
      ? 'このリンクは削除されたか、無効です'
      : '写真のフォルダが見つかりません'
    return { state: 'error', statusText: `エラー（${reason}）`, actionLabel: '再試行', actionEnabled: true }
  }
  const progress = prepareProgress(facts)
  if (!progress.every(line => line.total !== null && line.done >= line.total) || project.photoCount === 0) {
    // 走査中は走査の行、そのあとは一番遅れている行。
    const slowest = progress[0]!.total === null
      ? progress[0]!
      : progress.reduce((low, line) => (line.done < low.done ? line : low), progress[0]!)
    return {
      state: 'preparing',
      statusText: `準備中（${slowest.done.toLocaleString()} / ${slowest.total === null ? '?' : slowest.total.toLocaleString()}）`,
      actionLabel: '開始',
      actionEnabled: project.photoCount > 0 && project.status !== 'scanning'
    }
  }
  if (session?.finished) {
    return {
      state: 'done',
      statusText: `選別完了・★1 以上が ${facts.keptCount.toLocaleString()} 枚`,
      actionLabel: '結果を見る',
      actionEnabled: true
    }
  }
  if (session) {
    return {
      state: 'culling',
      statusText: `★${session.target_star} を選別中・ROUND ${session.round}`,
      actionLabel: '続ける',
      actionEnabled: true
    }
  }
  return { state: 'new', statusText: '準備完了', actionLabel: '開始', actionEnabled: true }
}

export interface PrepareLine {
  label: string
  done: number
  /** 総数が分からない間（走査中）は null（画面は `?`）。 */
  total: number | null
}

/**
 * 準備の進み 3 行（写真の走査・撮影時刻とサムネイル・表示用画像）。
 * 走査中の枚数は、Project の行の枚数を読んだ分とみなす（総数は分からない）。
 */
export function prepareProgress(facts: StatusFacts): PrepareLine[] {
  const { project } = facts
  const count = Math.max(0, project.photoCount)
  const scanning = project.status === 'scanning' || project.status === 'new'
  const total = scanning ? null : count
  return [
    { label: '写真の走査', done: count, total },
    { label: '撮影時刻とサムネイル', done: clampDone(count - facts.analysisBacklog, count), total },
    { label: '表示用画像', done: clampDone(count - facts.displayBacklog, count), total }
  ]
}

const clampDone = (done: number, count: number) => Math.min(count, Math.max(0, done))

/** 点の色（Vuetify の色名）。文言は付けない。 */
export const STATE_COLOR: Record<CardState, string> = {
  new: 'grey',
  preparing: 'secondary',
  culling: 'primary',
  done: 'success',
  error: 'error'
}
