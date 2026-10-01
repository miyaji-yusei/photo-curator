// @vitest-environment node
import { describe, expect, it, vi } from 'vitest'
import { fakeDesktop, loadCuratorModule, project, settle } from './helpers/curatorHarness'

describe('W18: 読めなかったことを、読めた状態に見せない', () => {
  it('ホームの状態: 選別の集計が読めないプロジェクトは、状態を作らない（前に読めた値は残す）', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const { createCurator } = await loadCuratorModule()
    let broken = false
    const { desktop } = fakeDesktop({
      getSelectionSummary: async () => {
        if (broken) throw new Error('読めません')
        return { counts: [0, 0, 0, 0, 0, 0], total: 0 }
      }
    })
    const curator = createCurator(desktop)
    curator.projects.value = [project('A'), project('B')]
    const refresh = async () => {
      curator.view.value = 'project'
      await settle()
      curator.view.value = 'home'
      await settle()
      await settle()
    }
    await refresh()
    expect(Object.keys(curator.projectCards.value).sort()).toEqual(['A', 'B'])
    const before = curator.projectCards.value.A

    broken = true
    await refresh()
    // 以前は 0・null になって「選別なし」の状態が作られていた。今は前の値のまま（無ければ出さない）。
    expect(curator.projectCards.value.A).toBe(before)
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  it('ホームの状態: 初めから読めないなら、状態の行を出さない', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const { createCurator } = await loadCuratorModule()
    const { desktop } = fakeDesktop({ loadSession: async () => { throw new Error('壊れた') } })
    const curator = createCurator(desktop)
    curator.projects.value = [project('A')]
    curator.view.value = 'project'
    await settle()
    curator.view.value = 'home'
    await settle()
    await settle()
    expect(curator.projectCards.value.A).toBeUndefined()
    warn.mockRestore()
  })

  it('集計が読めないときは、黙って消さずに理由を出す', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const { createCurator } = await loadCuratorModule()
    const { desktop } = fakeDesktop({ getSelectionSummary: async () => { throw new Error('IDB が壊れています') } })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('A')
    await curator.loadSummary()
    expect(curator.selectionSummary.value).toBeNull()
    expect(curator.error.value).toContain('IDB が壊れています')
    warn.mockRestore()
  })

  it('中止が失敗したら、例外を投げず error に出す', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop } = fakeDesktop({ cancelProjectTask: async () => { throw new Error('止められません') } })
    const curator = createCurator(desktop)
    curator.analysisProgress.value = {
      projectId: 'A', task: 'background', phase: 'hashing', processed: 1, total: 2, message: '', warning: null, failed: 0
    }
    await expect(curator.cancelAnalysis()).resolves.toBeUndefined()
    expect(curator.error.value).toBe('止められません')
  })
})
