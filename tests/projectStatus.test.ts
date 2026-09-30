import { describe, expect, it } from 'vitest'
import { prepareProgress, projectStatus } from '~/utils/projectStatus'
import type { StatusFacts } from '~/utils/projectStatus'

type Over = Partial<Omit<StatusFacts, 'project'>> & { project?: Partial<StatusFacts['project']> }
const base = (over: Over = {}): StatusFacts => ({
  analysisBacklog: 0,
  displayBacklog: 0,
  session: null,
  keptCount: 0,
  ...over,
  project: { status: 'ready', photoCount: 40, sourceKind: 'folder', ...over.project }
})

describe('projectStatus の 5 状態', () => {
  it('準備中: 準備が終わっていない間は「n / 総数」', () => {
    const status = projectStatus(base({ analysisBacklog: 10, displayBacklog: 25 }))
    expect(status).toMatchObject({ state: 'preparing', statusText: '準備中（15 / 40）', actionLabel: '開始', actionEnabled: true })
  })

  it('準備中: 走査の間は総数が分からず「n / ?」で、開始は押せない', () => {
    const status = projectStatus(base({ project: { status: 'scanning', photoCount: 12 } }))
    expect(status).toMatchObject({ state: 'preparing', statusText: '準備中（12 / ?）', actionEnabled: false })
  })

  it('準備中: まだ 1 枚も無い', () => {
    const status = projectStatus(base({ project: { status: 'new', photoCount: 0 } }))
    expect(status).toMatchObject({ state: 'preparing', statusText: '準備中（0 / ?）', actionEnabled: false })
  })

  it('選別中: ★n を選別中・ROUND n', () => {
    const status = projectStatus(base({ session: { target_star: 2, round: 3, finished: false } }))
    expect(status).toMatchObject({ state: 'culling', statusText: '★2 を選別中・ROUND 3', actionLabel: '続ける' })
  })

  it('選別中: session があれば、未処理（backlog）が残っていても選別中（準備中に戻さない）', () => {
    const status = projectStatus(base({
      analysisBacklog: 10, displayBacklog: 25, session: { target_star: 2, round: 3, finished: false }
    }))
    expect(status).toMatchObject({ state: 'culling', statusText: '★2 を選別中・ROUND 3', actionLabel: '続ける' })
    const done = projectStatus(base({
      analysisBacklog: 10, session: { target_star: 0, round: 1, finished: true }, keptCount: 4
    }))
    expect(done.state).toBe('done')
  })

  it('選別完了: ★1 以上が n 枚', () => {
    const status = projectStatus(base({ session: { target_star: 0, round: 1, finished: true }, keptCount: 7 }))
    expect(status).toMatchObject({ state: 'done', statusText: '選別完了・★1 以上が 7 枚', actionLabel: '結果を見る' })
  })

  it('準備完了: 選別の途中が無い', () => {
    expect(projectStatus(base())).toMatchObject({ state: 'new', statusText: '準備完了', actionLabel: '開始', actionEnabled: true })
  })

  it('エラー: 理由を添え、再試行', () => {
    const amazon = projectStatus(base({ project: { status: 'missing', sourceKind: 'amazon' } }))
    expect(amazon).toMatchObject({ state: 'error', statusText: 'エラー（このリンクは削除されたか、無効です）', actionLabel: '再試行' })
    const folder = projectStatus(base({ project: { status: 'missing' } }))
    expect(folder.statusText).toBe('エラー（写真のフォルダが見つかりません）')
  })

  it('エラーは準備中や選別中より先に判定する', () => {
    const status = projectStatus(base({ project: { status: 'missing' }, analysisBacklog: 5, session: { target_star: 0, round: 1, finished: false } }))
    expect(status.state).toBe('error')
  })
})

describe('prepareProgress', () => {
  it('3 行の done / total', () => {
    const lines = prepareProgress(base({ analysisBacklog: 4, displayBacklog: 40 }))
    expect(lines.map(line => [line.done, line.total])).toEqual([[40, 40], [36, 40], [0, 40]])
  })

  it('走査中は総数が null', () => {
    const lines = prepareProgress(base({ project: { status: 'scanning', photoCount: 9 } }))
    expect(lines.every(line => line.total === null)).toBe(true)
  })

  it('未処理が総数を超えても負にならない', () => {
    const lines = prepareProgress(base({ analysisBacklog: 99 }))
    expect(lines[1]!.done).toBe(0)
  })
})
