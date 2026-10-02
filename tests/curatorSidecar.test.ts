// @vitest-environment node
// U34: サイドカーを書く・確かめるタイミング（設計書 §4.5）を、画面なしの curator で確かめる。
// 判断そのものは tests/sidecarSync.test.mjs（core の sidecarPlan）。
import { describe, expect, it } from 'vitest'
import * as core from '~/lib/core'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { Photo } from '~/types/photo'
import type { SidecarState } from '~/composables/photoBackend'
import { fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

const ANDROID = 'zzzzzzzz-1111-2222-3333-444444444444'

/** 写真 `count` 枚・書けるサイドカー（メモリ）を持つ偽の desktop。 */
function sidecarDesktop(count: number) {
  const all = photos('P', count)
  const byId = new Map(all.map(photo => [photo.id, photo]))
  const nas = new Map<string, string>()
  let saved: SavedSelection | null = null
  let state: SidecarState = { seenAt: 0, seenBy: '', localChanged: false }
  const fake = fakeDesktop({
    getCoreInputs: async () => all.map(photo => ({ ...photo })),
    getPhotosByIds: async (_id: string, ids: string[]) => ids.map(id => byId.get(id)).filter((p): p is Photo => !!p),
    getProjectPhotoPage: async () => ({ photos: all.slice(0, 3), total: count }),
    listProjects: async () => [project('P', { photoCount: count })],
    saveSelectionResults: async (_id: string, entries: { id: string, rating: number }[]) => {
      for (const entry of entries) {
        const photo = byId.get(entry.id)
        if (photo) photo.rating = entry.rating
      }
    },
    resetSelectionResults: async () => { for (const photo of all) photo.rating = 0 },
    loadSession: async () => saved,
    saveSession: async (_id: string, value: SavedSelection | null) => { saved = value },
    sidecarSupported: async () => 'readwrite',
    readSidecar: async () => nas.get('catalog.json') ?? null,
    writeSidecar: async (_id: string, json: string, name = 'catalog.json') => { nas.set(name, json) },
    writeSidecarChecked: async (_id: string, json: string, expected: string | null) => {
      if ((nas.get('catalog.json') ?? null) !== expected) return 'changed'
      nas.set('catalog.json', json)
      return 'written'
    },
    loadSidecarState: async () => ({ ...state }),
    saveSidecarState: async (_id: string, next: SidecarState) => { state = { ...next } },
    deviceIdentity: async () => ({ id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee', name: 'この PC' })
  })
  return { ...fake, all, nas, saved: () => saved }
}

describe('U34: サイドカーを書く・確かめるタイミング', () => {
  it('ラウンドが終わったら、変更を書く（待たずに）', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop, nas } = sidecarDesktop(2)
    const curator = createCurator(desktop)
    await curator.openProject(project('P', { photoCount: 2 }))
    curator.pendingTournamentSettings.value = { groupSize: 2, groupBursts: false }
    await curator.finishTournamentStart()
    await settle()
    expect(nas.has('catalog.json')).toBe(false) // 判断 0 件（未着手）は書かない

    const first = curator.currentGroup.value[0]!
    await curator.toggleChoice(first) // 1 組だけなので、これでラウンドが終わる
    for (let index = 0; index < 5; index++) await settle()

    expect(curator.session.value?.core.finished).toBe(true)
    const written = core.sidecarFromJson(nas.get('catalog.json')!)
    expect(written?.sessions.tournament?.finished).toBe(true)
    expect(written?.keyBase).toBe('folder')
  })

  it('「選別を開始」の前に確かめ、別の端末が進めていれば取り込んで始めない（星を 0 にしない）', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop, nas, calls, all } = sidecarDesktop(4)
    const curator = createCurator(desktop)
    await curator.openProject(project('P', { photoCount: 4 }))
    // プロジェクトの画面にいる間に、Android が選別を進めて書いた。
    const refs = all.map(photo => ({ relative_path: photo.relativePath, captured_at: photo.capturedAt, d_hash: null, d_hash_version: 2 }))
    const threshold = { window_ms: 4000, distance: 9, d_hash_version: 2 }
    const session = core.advance(core.startRound(refs, 2, 0, false, threshold, []), ['IMG_1.JPG'])
    nas.set('catalog.json', core.sidecarToJson({
      version: 2, updatedAt: 5, updatedBy: ANDROID, updatedByName: 'Pixel', writeId: 'w-a1', keyBase: 'folder',
      photos: {}, burstOverrides: [], sessions: { tournament: session }
    }))

    curator.view.value = 'settings'
    await curator.beginTournament()
    await settle()

    expect(curator.view.value).toBe('project')
    expect(calls.resetSelectionResults).toBeUndefined()
    expect(curator.session.value?.core.history).toHaveLength(1)
    expect(all.find(photo => photo.relativePath === 'IMG_1.JPG')?.rating).toBe(1)
    expect(curator.sidecarNotice.value).toContain('Pixel')
  })

  it('両方とも進んでいて違えば、「選別を開始」はダイアログを出して止まる', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop, nas, calls, all } = sidecarDesktop(4)
    all[0]!.rating = 2 // この端末にも判断がある（行の星）
    const curator = createCurator(desktop)
    await curator.openProject(project('P', { photoCount: 4 }))
    nas.set('catalog.json', core.sidecarToJson({
      version: 2, updatedAt: 5, updatedBy: ANDROID, updatedByName: 'Pixel', writeId: 'w-a1', keyBase: 'folder',
      photos: { 'IMG_3.JPG': { rating: 1 } }, burstOverrides: [], sessions: {}
    }))
    curator.view.value = 'settings'
    await curator.beginTournament()
    expect(curator.sidecarClash.value?.theirsName).toBe('Pixel')
    expect(curator.view.value).toBe('project')
    expect(calls.resetSelectionResults).toBeUndefined()
  })
})

describe('U48: プロジェクトの設定（同名の JPEG と RAW）をサイドカーで同期する', () => {
  /** sidecarDesktop に、プロジェクトの設定（値と切り替えた時刻）を持たせる。 */
  function withPair(count: number) {
    const fake = sidecarDesktop(count)
    const pair = { value: true, at: 0 }
    const desktop = new Proxy(fake.desktop, {
      get(target, key: string) {
        if (key === 'listProjects') {
          return async () => [project('P', { photoCount: count, pairRawJpeg: pair.value, pairRawJpegAt: pair.at })]
        }
        if (key === 'saveProjectPairRaw') {
          return async (_id: string, enabled: boolean, at?: number) => {
            pair.value = enabled
            pair.at = at ?? Date.now()
            return enabled
          }
        }
        return Reflect.get(target, key)
      }
    })
    return { ...fake, desktop, pair }
  }

  it('画面で切り替えたら、選別していなくても設定をサイドカーに書く（区切りの自動の書き込み）', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop, nas, pair } = withPair(2)
    const curator = createCurator(desktop)
    await curator.openProject(project('P', { photoCount: 2 }))
    expect(nas.has('catalog.json')).toBe(false)
    await curator.setPairRawJpeg(false)
    for (let index = 0; index < 5; index++) await settle()
    expect(pair.value).toBe(false)
    expect(pair.at).toBeGreaterThan(0)
    const written = core.sidecarFromJson(nas.get('catalog.json')!)
    expect(written?.settings?.pairRawJpeg).toEqual({ value: false, at: pair.at })
    expect(curator.activeProject.value?.pairRawJpeg).toBe(false)
  })

  it('開いたとき、ほかの端末で新しく切り替えた設定を取り込み、再読み込みを促す（自動では走査しない）', async () => {
    const { createCurator } = await loadCuratorModule()
    const { desktop, nas, pair, calls } = withPair(2)
    nas.set('catalog.json', core.sidecarToJson({
      version: 2, updatedAt: 5, updatedBy: ANDROID, updatedByName: 'Pixel', writeId: 'w-a1', keyBase: 'folder',
      photos: {}, burstOverrides: [], sessions: {}, settings: { pairRawJpeg: { value: false, at: 1_790_955_613_101 } }
    }))
    const curator = createCurator(desktop)
    await curator.openProject(project('P', { photoCount: 2 }))
    for (let index = 0; index < 5; index++) await settle()
    expect(pair).toEqual({ value: false, at: 1_790_955_613_101 })
    expect(curator.activeProject.value?.pairRawJpeg).toBe(false)
    expect(curator.sidecarNotice.value).toContain('「同名の JPEG と RAW を 1 枚として扱う」をオフにしました')
    expect(curator.sidecarNotice.value).toContain('「写真を再読み込み」を押してください')
    expect(calls.startProjectScan).toBeUndefined()
    expect(curator.sidecarClash.value).toBeNull()
  })
})
