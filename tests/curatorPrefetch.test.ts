// @vitest-environment node
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Photo } from '~/types/photo'
import { fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

afterEach(() => vi.unstubAllGlobals())

class FakeImage {
  decoding = ''
  src = ''
  decode() { return Promise.resolve() }
}

describe('W13: 先読みは連打で同じ組を重複して読まない・2 組を並べて読む', () => {
  it('loadCurrentPhotos を続けて呼んでも、次の 2 組はそれぞれ 1 回ずつだけ読む', async () => {
    vi.stubGlobal('Image', FakeImage)
    const { createCurator } = await loadCuratorModule()
    const all = photos('P', 12)
    const byId = new Map(all.map(photo => [photo.id, photo]))
    const reads: string[] = []
    let inFlight = 0
    let peak = 0
    let slow = false
    const { desktop } = fakeDesktop({
      getCoreInputs: async () => all,
      photoDisplayUrl: (photo: Photo) => photo.displayPath,
      getPhotosByIds: async (_id: string, ids: string[]) => {
        reads.push(ids.join(','))
        if (slow) {
          inFlight += 1
          peak = Math.max(peak, inFlight)
          await new Promise(resolve => setTimeout(resolve, 20))
          inFlight -= 1
        }
        return ids.map(id => byId.get(id)).filter((p): p is Photo => !!p)
      },
      getProjectPhotoPage: async () => ({ photos: all.slice(0, 3), total: 12 })
    })
    const curator = createCurator(desktop)
    curator.settings.groupSize = 4
    await curator.openProject(project('P', { photoCount: 12 }))
    curator.pendingTournamentSettings.value = { groupSize: 4, groupBursts: false }
    slow = true
    await curator.finishTournamentStart()
    // 最初の先読み（次の 2 組。まだ返っていない）の最中に、さらに 2 回呼ぶ（連打）。
    await Promise.all([curator.loadCurrentPhotos(), curator.loadCurrentPhotos()])
    await new Promise(resolve => setTimeout(resolve, 200))
    await settle()

    const current = curator.currentGroup.value.join(',')
    const counts = new Map<string, number>()
    for (const key of reads) if (key !== current) counts.set(key, (counts.get(key) ?? 0) + 1)
    console.log(`[bench] 連打の先読み: 次の組の読み ${[...counts.values()].join('+')} 回、同時に走った最大 ${peak} 本`)
    expect(counts.size).toBe(2)
    expect([...counts.values()]).toEqual([1, 1])
    expect(peak).toBeGreaterThanOrEqual(2)
  })
})
