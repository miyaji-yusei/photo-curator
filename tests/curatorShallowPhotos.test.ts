// @vitest-environment node
import { isReactive, ref, shallowRef } from 'vue'
import { describe, expect, it } from 'vitest'
import { collapseBursts } from '~/utils/collapseBursts'
import { fakeDesktop, loadCuratorModule, photos, project } from './helpers/curatorHarness'

describe('W15: 一覧の Photo を深い reactive にしない', () => {
  it('結果の一覧の要素は reactive のプロキシにならない', async () => {
    const { createCurator } = await loadCuratorModule()
    const all = photos('P', 5)
    const { desktop } = fakeDesktop({ getProjectPhotoPage: async () => ({ photos: all, total: 5 }) })
    const curator = createCurator(desktop)
    curator.activeProject.value = project('P')
    await curator.loadResultsPage(true)
    expect(curator.resultsPhotos.value).toHaveLength(5)
    expect(isReactive(curator.resultsPhotos.value[0])).toBe(false)
  })

  it('計測: 4,000 枚を 120 枚ずつ足しながら全件を走査する時間（ref と shallowRef）', () => {
    const all = photos('P', 4_000)
    const run = (make: () => { value: typeof all }) => {
      const list = make()
      const start = performance.now()
      for (let at = 0; at < all.length; at += 120) {
        list.value = [...list.value, ...all.slice(at, at + 120)]
        collapseBursts(list.value, undefined)
      }
      return performance.now() - start
    }
    run(() => ref([] as typeof all))
    const deep = run(() => ref([] as typeof all))
    const shallow = run(() => shallowRef([] as typeof all))
    console.log(`[bench] 4,000 枚のページ送り 34 回: ref ${deep.toFixed(1)} ms → shallowRef ${shallow.toFixed(1)} ms`)
    expect(shallow).toBeGreaterThan(0)
  })
})
