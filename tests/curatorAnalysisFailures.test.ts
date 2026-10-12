// @vitest-environment node
import { describe, expect, it } from 'vitest'
import type { AnalysisFailure } from '~/types/photo'
import { fakeDesktop, loadCuratorModule, project, settle } from './helpers/curatorHarness'

const failure = (name: string, kind: AnalysisFailure['kind'] = 'unsupported'): AnalysisFailure =>
  ({ relativePath: name, name, kind, reason: '壊れている', at: null })

describe('U58: 解析できなかった写真の警告', () => {
  it('プロジェクトを開くと件数を DB から取り、替えたら前の件数を持ち越さない（B6）', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop } = fakeDesktop({
      getAnalysisBacklog: async () => 1,
      getAnalysisFailures: async (id: string) => (id === 'A' ? [failure('a.jpg'), failure('b.jpg', 'transient')] : [])
    })
    const curator = createCurator(desktop)
    await curator.openProject(project('A'))
    await settle()
    expect(curator.analysisFailures.value).toBe(2)
    expect(curator.analysisFailureList.value.map(item => item.kind)).toEqual(['unsupported', 'transient'])

    const opening = curator.openProject(project('B'))
    // 開いた直後（読み込みの前）に、前のプロジェクトの件数・一覧が消えている。
    expect(curator.analysisFailures.value).toBe(0)
    expect(curator.analysisFailureList.value).toEqual([])
    await opening
    await settle()
    expect(curator.analysisFailures.value).toBe(0)
  })

  it('一覧のダイアログは開くときに読み直す', async () => {
    const { createCurator } = await loadCuratorModule()
    let list = [failure('a.jpg')]
    const { desktop } = fakeDesktop({ getAnalysisFailures: async () => list })
    const curator = createCurator(desktop)
    await curator.openProject(project('A'))
    await settle()
    list = [failure('a.jpg'), failure('c.jpg')]
    await curator.openAnalysisFailures()
    expect(curator.analysisFailuresDialog.value).toBe(true)
    expect(curator.analysisFailureList.value).toHaveLength(2)
  })

  it('全部が非対応で 0 枚でも、走査済み（ready）なら開くたびに走査を始めない', async () => {
    const { createCurator } = await loadCuratorModule()
    // importPhotos を持たない＝フォルダを走査できる環境（デスクトップ）。
    const { desktop, calls } = fakeDesktop({ importPhotos: undefined })
    const curator = createCurator(desktop)
    await curator.openProject(project('A', { photoCount: 0, status: 'ready' }))
    await settle()
    expect(calls.startProjectScan).toBeUndefined()
    await curator.openProject(project('B', { photoCount: 0, status: 'new' }))
    await settle()
    expect(calls.startProjectScan?.length).toBe(1)
  })
})
