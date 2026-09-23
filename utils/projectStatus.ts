// ホームのカードの5状態（設計 02 章）。**出所へ問い合わせない**。
// 端末に置いたもの（Project の集計・core の Session）だけから決める。
import type { Project } from '~/types/project'
import type { Session } from '~/lib/core'

export type CardState = 'new' | 'preparing' | 'culling' | 'done' | 'error'

export interface CardStatus {
  state: CardState
  statusText: string
  countText: string
  actionLabel: string
  actionEnabled: boolean
}

export function projectStatus(project: Project, session: Session | null): CardStatus {
  if (project.prepareWarning && project.scannedCount < project.photoCount) {
    return {
      state: 'error',
      statusText: project.prepareWarning,
      countText: `${project.photoCount} 枚`,
      actionLabel: '再試行',
      actionEnabled: true
    }
  }
  const scanning = project.scannedCount < project.photoCount || project.photoCount === 0
  const preparingDisplay = project.displayedCount < project.scannedCount
  if (scanning || preparingDisplay) {
    return {
      state: 'preparing',
      statusText: '準備中 · 表示用画像を作成',
      countText: `${project.displayedCount} / ${project.photoCount || project.scannedCount} 枚`,
      actionLabel: '開始',
      actionEnabled: !scanning && project.displayedCount > 0
    }
  }
  if (project.completedAt) {
    const kept = session ? Object.values(session.ratings).filter(r => r > 0).length : 0
    return {
      state: 'done',
      statusText: `選別完了 · ★1 以上が ${kept} 枚`,
      countText: `${project.photoCount} 枚`,
      actionLabel: '結果を見る',
      actionEnabled: true
    }
  }
  if (session) {
    const remaining = session.queue.length + session.current.length
    return {
      state: 'culling',
      statusText: `★${session.target_star} を選別中 · ROUND ${session.round}`,
      countText: `残り ${remaining} / ${project.photoCount} 枚`,
      actionLabel: '続ける',
      actionEnabled: true
    }
  }
  return {
    state: 'new',
    statusText: '準備完了',
    countText: `${project.photoCount} 枚`,
    actionLabel: '開始',
    actionEnabled: true
  }
}

export const STATE_COLOR: Record<CardState, string> = {
  new: 'grey',
  preparing: 'secondary',
  culling: 'primary',
  done: 'on-surface',
  error: 'error'
}
