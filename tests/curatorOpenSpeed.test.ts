// @vitest-environment node
import { describe, expect, it } from 'vitest'
import { deferred, fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

const cardSamples = (calls: Record<string, unknown[][]>) =>
  (calls.getProjectPhotoPage ?? []).filter(args => args[1] === 0 && args[2] === 1).length

describe('W4: ホームからプロジェクトを開くとき、全プロジェクトの状態を読み直さない', () => {
  it('home -> project では見本の読みが 0 回、選別から project へ戻るときは読み直す', async () => {
    const { createCurator } = await loadCuratorModule()
    const list = [project('A'), project('B'), project('C')]
    const { desktop, calls } = fakeDesktop({
      getProjectPhotoPage: async (id: string) => ({ photos: photos(id, 3), total: 3 })
    })
    const curator = createCurator(desktop)
    curator.projects.value = list
    await curator.openProject(list[0]!)
    await settle()
    expect(cardSamples(calls)).toBe(0) // 以前は 3 プロジェクトぶん（3 回）

    curator.view.value = 'tournament'
    await settle()
    curator.view.value = 'project' // 中断して戻る
    await settle()
    expect(cardSamples(calls)).toBe(3)
  })
})

describe('W9: プロジェクトを開く処理の並列化', () => {
  it('プレビューの読みは、サイドカーの確認の完了を待たずに始まる', async () => {
    const { createCurator } = await loadCuratorModule()
    const sidecar = deferred<'none'>()
    const { desktop, calls } = fakeDesktop({
      sidecarSupported: () => sidecar.promise,
      getProjectPhotoPage: async (id: string) => ({ photos: photos(id, 3), total: 3 })
    })
    const curator = createCurator(desktop)
    const opening = curator.openProject(project('A'))
    await settle()
    expect(calls.sidecarSupported?.length).toBe(1)
    expect(calls.getProjectPhotoPage?.length).toBe(1) // サイドカーの確認はまだ終わっていない
    expect(curator.previewPhotos.value).toHaveLength(3)
    sidecar.resolve('none')
    await opening
    expect(curator.previewPhotos.value).toHaveLength(3)
  })

  it('session の読みはサイドカーの確認のあと（T8 の順）', async () => {
    const { createCurator } = await loadCuratorModule()
    const sidecar = deferred<'none'>()
    const { desktop, calls } = fakeDesktop({ sidecarSupported: () => sidecar.promise })
    const curator = createCurator(desktop)
    const opening = curator.openProject(project('A'))
    await settle()
    expect(calls.loadSession ?? []).toHaveLength(0)
    sidecar.resolve('none')
    await opening
    expect(calls.loadSession).toHaveLength(1)
  })

  it('未解析数・表示用の状態・集計は互いを待たずに読む', async () => {
    const { createCurator } = await loadCuratorModule()
    const backlog = deferred<number>()
    const { desktop, calls } = fakeDesktop({
      getAnalysisBacklog: () => backlog.promise
    })
    const curator = createCurator(desktop)
    const opening = curator.openProject(project('A'))
    await settle()
    // 未解析数の応答が来ていなくても、表示用の状態と集計はもう読み始めている。
    expect(calls.getDisplaySettings?.length).toBe(1)
    expect(calls.getSelectionSummary?.length).toBe(1)
    backlog.resolve(0)
    await opening
    expect(curator.loading.value).toBe(false)
  })
})
