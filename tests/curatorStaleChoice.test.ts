// @vitest-environment node
import { describe, expect, it } from 'vitest'
import type { Photo } from '~/types/photo'
import { fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

/** 写真 `count` 枚のプロジェクトを開き、`groupSize` 枚ずつの選別を始めた curator。 */
async function startedCurator(count: number, groupSize: number) {
  const { createCurator } = await loadCuratorModule()
  const all = photos('P', count)
  const byId = new Map(all.map(photo => [photo.id, photo]))
  const { desktop } = fakeDesktop({
    getCoreInputs: async () => all,
    getPhotosByIds: async (_id: string, ids: string[]) => ids.map(id => byId.get(id)).filter((p): p is Photo => !!p),
    getProjectPhotoPage: async () => ({ photos: all.slice(0, 3), total: count })
  })
  const curator = createCurator(desktop)
  curator.settings.groupSize = groupSize
  await curator.openProject(project('P', { photoCount: count }))
  curator.pendingTournamentSettings.value = { groupSize, groupBursts: false }
  await curator.finishTournamentStart()
  await settle()
  return curator
}

describe('W5: 古い写真への操作が、新しい組を黙って「選ばない」にしない', () => {
  it('トーナメント: 前の組の写真をクリックしても、新しい組は進まない（単数選択）', async () => {
    const curator = await startedCurator(12, 4)
    const first = [...curator.currentGroup.value]
    expect(first).toHaveLength(4)
    await curator.toggleChoice(first[0]!) // 正しい操作。次の組へ
    const second = [...curator.currentGroup.value]
    expect(second).not.toEqual(first)
    const historyBefore = curator.session.value!.core.history.length

    await curator.toggleChoice(first[1]!) // 前の組が見えている間の遅れたクリック
    await settle()

    expect(curator.currentGroup.value).toEqual(second)
    expect(curator.session.value!.core.history.length).toBe(historyBefore)
  })

  it('トーナメント: 複数選択でも、前の組の写真は選択に入らない', async () => {
    const curator = await startedCurator(12, 4)
    const first = [...curator.currentGroup.value]
    await curator.toggleChoice(first[0]!)
    curator.session.value!.multiSelect = true
    await curator.toggleChoice(first[2]!)
    expect(curator.session.value!.selectedInGroup).toEqual([])
  })

  it('スライドショー: 戻したあとに、前の写真の id で「落とす」が届いても、戻した写真は落ちない', async () => {
    const curator = await startedCurator(5, 1)
    const a = curator.currentGroup.value[0]!
    await curator.decideSlide('keep', a)
    const b = curator.currentGroup.value[0]!
    expect(b).not.toBe(a)
    await curator.undoChoice() // 飛ばしている最中の Backspace
    expect(curator.currentGroup.value[0]).toBe(a)

    await curator.decideSlide('drop', b) // 遅れて走る、前の写真（b）への判断
    await settle()

    expect(curator.currentGroup.value[0]).toBe(a)
    expect(curator.session.value!.core.history.length).toBe(0)
  })

  it('正しい写真への操作は、今までどおり進む', async () => {
    const curator = await startedCurator(5, 1)
    const a = curator.currentGroup.value[0]!
    await curator.decideSlide('drop', a)
    expect(curator.session.value!.core.history.length).toBe(1)
    expect(curator.currentGroup.value[0]).not.toBe(a)
  })
})
