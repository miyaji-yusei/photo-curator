// @vitest-environment node
import { describe, expect, it } from 'vitest'
import type { Photo, PhotoPage } from '~/types/photo'
import { deferred, fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

const rated = (projectId: string, rating: number, count: number): Photo[] =>
  photos(projectId, count).map(photo => ({ ...photo, id: `${projectId}-r${rating}-${photo.id}`, rating }))

describe('W7: 一覧の読み込みは古い応答を捨てる', () => {
  it('結果: 星 5 の応答が遅れて、星 4 に切り替えたあとに届いても、星 4 だけが並ぶ', async () => {
    const { createCurator } = await loadCuratorModule()
    const slow = deferred<PhotoPage>()
    const { desktop } = fakeDesktop({
      getProjectPhotoPage: (_id: string, _offset: number, _limit: number, rating: number | null) =>
        rating === 5 ? slow.promise : Promise.resolve({ photos: rated('P', 4, 3), total: 3 })
    })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')

    const first = curator.selectResultsRating(5)
    await settle()
    await curator.selectResultsRating(4)
    slow.resolve({ photos: rated('P', 5, 2), total: 2 })
    await first
    await settle()

    expect(curator.resultsPhotos.value.map(photo => photo.rating)).toEqual([4, 4, 4])
    expect(curator.resultsOffset.value).toBe(3)
    expect(curator.resultsTotal.value).toBe(3)
    expect(curator.resultsBusy.value).toBe(false)
  })

  it('結果: 並べ替えを切り替えて reset したあと、前の「続きを読む」の応答は捨てる', async () => {
    const { createCurator } = await loadCuratorModule()
    const more = deferred<PhotoPage>()
    let call = 0
    const { desktop } = fakeDesktop({
      getProjectPhotoPage: () => {
        call += 1
        if (call === 1) return Promise.resolve({ photos: rated('P', 3, 2), total: 4 })
        if (call === 2) return more.promise // 続き（遅れる）
        return Promise.resolve({ photos: rated('P', 3, 2), total: 4 }) // reset
      }
    })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')
    await curator.loadResultsPage(true)
    const loadingMore = curator.loadResultsPage()
    await settle()
    await curator.loadResultsPage(true)
    more.resolve({ photos: rated('P', 3, 2).map(photo => ({ ...photo, id: `late-${photo.id}` })), total: 4 })
    await loadingMore
    await settle()
    expect(curator.resultsPhotos.value.map(photo => photo.id).some(id => id.startsWith('late-'))).toBe(false)
    expect(curator.resultsPhotos.value).toHaveLength(2)
    expect(curator.resultsOffset.value).toBe(2)
  })

  it('移動: 星 5 の応答が遅れて、星 4 の読み込みのあとに届いても、星 4 だけが並ぶ', async () => {
    const { createCurator } = await loadCuratorModule()
    const slow = deferred<PhotoPage>()
    const { desktop } = fakeDesktop({
      getProjectPhotoPage: (_id: string, _offset: number, _limit: number, rating: number | null) =>
        rating === 5 ? slow.promise : Promise.resolve({ photos: rated('P', 4, 3), total: 3 })
    })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')
    curator.openMoveDialog(5)
    await settle()
    curator.openMoveDialog(4)
    await settle()
    slow.resolve({ photos: rated('P', 5, 2), total: 2 })
    await settle()
    expect(curator.movePhotos.value.map(photo => photo.rating)).toEqual([4, 4, 4])
    expect(curator.moveOffset.value).toBe(3)
    expect(curator.moveBusy.value).toBe(false)
  })

  it('プレビュー: 古い読み直しの応答が後から届いても、新しい方を上書きしない', async () => {
    const { createCurator } = await loadCuratorModule()
    const slow = deferred<PhotoPage>()
    let call = 0
    const { desktop } = fakeDesktop({
      getProjectPhotoPage: () => {
        call += 1
        return call === 1 ? slow.promise : Promise.resolve({ photos: photos('P', 3), total: 3 })
      }
    })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')
    const first = curator.loadPreview('P')
    await settle()
    await curator.loadPreview('P')
    slow.resolve({ photos: photos('P', 1), total: 1 })
    await first
    expect(curator.previewPhotos.value).toHaveLength(3)
    expect(curator.previewTotal.value).toBe(3)
  })

  it('プレビュー: 読み直した後に、前の「続きを読む」の応答は足さない', async () => {
    const { createCurator } = await loadCuratorModule()
    const more = deferred<PhotoPage>()
    let call = 0
    const { desktop } = fakeDesktop({
      getProjectPhotoPage: () => {
        call += 1
        if (call === 1) return Promise.resolve({ photos: photos('P', 2), total: 5 })
        if (call === 2) return more.promise
        return Promise.resolve({ photos: photos('P', 2), total: 5 })
      }
    })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')
    await curator.loadPreview('P')
    const loadingMore = curator.loadMorePreview()
    await settle()
    await curator.loadPreview('P')
    more.resolve({ photos: photos('P', 2).map(photo => ({ ...photo, id: `late-${photo.id}` })), total: 5 })
    await loadingMore
    expect(curator.previewPhotos.value.map(photo => photo.id)).toEqual(['P-0', 'P-1'])
  })
})
