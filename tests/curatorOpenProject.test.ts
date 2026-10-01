// @vitest-environment node
import { describe, expect, it } from 'vitest'
import * as core from '~/lib/core'
import type { SavedSelection } from '~/utils/selectionFlow'
import { deferred, fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

/** 区別できる封筒（`updatedAt` で見分ける）。 */
const envelope = (tag: number): SavedSelection => ({
  v: 2,
  core: core.startRound([], 4, 0, false, { window_ms: 4000, distance: 9, d_hash_version: 2 }, []),
  stage: 'tournament',
  settings: { groupSize: 4, groupBursts: false },
  multiSelect: true,
  selectedInGroup: [],
  learning: null,
  burstDistance: 9,
  updatedAt: tag
}) as SavedSelection

describe('W6: openProject の古い呼び出しが新しいプロジェクトの session を上書きしない', () => {
  it('A を開いた直後に B を開き、A の loadSession が B より後に返っても、session は B のもの', async () => {
    const { createCurator } = await loadCuratorModule()
    const a = project('A')
    const b = project('B')
    const sessionA = deferred<SavedSelection>()
    const saved: string[] = []
    const { desktop } = fakeDesktop({
      loadSession: (id: string) => (id === 'A' ? sessionA.promise : Promise.resolve(envelope(2))),
      getProjectPhotoPage: async (id: string) => ({ photos: photos(id, 3), total: 3 }),
      saveSession: async (id: string, value: SavedSelection) => { saved.push(`${id}:${value.updatedAt}`) }
    })
    const curator = createCurator(desktop)

    const openingA = curator.openProject(a)
    await settle()
    const openingB = curator.openProject(b)
    await openingB
    expect(curator.session.value?.updatedAt).toBe(2)

    sessionA.resolve(envelope(1)) // A の応答が遅れて届く
    await openingA
    await settle()

    expect(curator.activeProject.value?.id).toBe('B')
    expect(curator.session.value?.updatedAt).toBe(2)
    expect(curator.loading.value).toBe(false)
    expect(saved).toEqual([]) // B の行に A の封筒を書いていない
  })

  it('古い呼び出しの finally が、新しい呼び出しの loading を落とさない', async () => {
    const { createCurator } = await loadCuratorModule()
    const sessionA = deferred<SavedSelection | null>()
    const sessionB = deferred<SavedSelection | null>()
    const { desktop } = fakeDesktop({
      loadSession: (id: string) => (id === 'A' ? sessionA.promise : sessionB.promise)
    })
    const curator = createCurator(desktop)
    const openingA = curator.openProject(project('A'))
    await settle()
    const openingB = curator.openProject(project('B'))
    await settle()
    sessionA.resolve(null)
    await openingA
    await settle()
    expect(curator.loading.value).toBe(true) // B はまだ読んでいる
    sessionB.resolve(null)
    await openingB
    expect(curator.loading.value).toBe(false)
  })
})
