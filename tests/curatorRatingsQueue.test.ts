// @vitest-environment node
import { describe, expect, it } from 'vitest'
import type { Photo, SelectionResult } from '~/types/photo'
import { deferred, fakeDesktop, loadCuratorModule, photos, project, settle } from './helpers/curatorHarness'

/** 行の星の書き込みを、外から終わらせられるようにした curator。 */
async function gatedCurator(count: number, groupSize: number) {
  const { createCurator } = await loadCuratorModule()
  const all = photos('P', count)
  const byId = new Map(all.map(photo => [photo.id, photo]))
  const log: string[] = []
  const gates: Array<ReturnType<typeof deferred<void>>> = []
  let saves = 0
  const { desktop, calls } = fakeDesktop({
    getCoreInputs: async () => all,
    getPhotosByIds: async (_id: string, ids: string[]) => ids.map(id => byId.get(id)).filter((p): p is Photo => !!p),
    getProjectPhotoPage: async () => ({ photos: all.slice(0, 3), total: count }),
    saveSelectionResults: (_id: string, entries: SelectionResult[]) => {
      const n = ++saves
      log.push(`write#${n} start (${entries.map(entry => `${entry.id}=${entry.rating}`).join(',')})`)
      const gate = deferred<void>()
      gates.push(gate)
      return gate.promise.then(() => { log.push(`write#${n} end`) })
    },
    getSelectionSummary: async () => {
      log.push('summary')
      return { counts: [0, 0, 0, 0, 0, 1], total: count }
    }
  })
  const curator = createCurator(desktop)
  curator.settings.groupSize = groupSize
  await curator.openProject(project('P', { photoCount: count }))
  curator.pendingTournamentSettings.value = { groupSize, groupBursts: false }
  await curator.finishTournamentStart()
  await settle()
  // 開始までの書き込みは済ませる。
  while (gates.length) gates.shift()!.resolve()
  await settle()
  log.length = 0
  return { curator, log, gates, calls, release: () => gates.shift()?.resolve() }
}

describe('W1: 次の組の表示は、行の星の書き込みの完了を待たない', () => {
  it('タップのあと、書き込みが終わっていなくても次の組が出る', async () => {
    const { curator, log, release } = await gatedCurator(12, 4)
    const first = [...curator.currentGroup.value]
    const tap = curator.toggleChoice(first[0]!)
    await settle()
    // 書き込みは始まっているが終わっていない。それでも画面は次の組。
    expect(log.some(line => line.startsWith('write#1 start'))).toBe(true)
    expect(log).not.toContain('write#1 end')
    expect(curator.tournamentPhotos.value.map(photo => photo.id)).toEqual(curator.currentGroup.value)
    expect(curator.currentGroup.value).not.toEqual(first)
    release()
    await tap
  })

  it('書き込みは入れた順に 1 本ずつ（「1 つ戻す」が前の書き込みを追い越さない）', async () => {
    const { curator, log, release } = await gatedCurator(12, 4)
    const first = [...curator.currentGroup.value]
    void curator.toggleChoice(first[0]!)
    await settle()
    const undoing = curator.undoChoice() // 1 つ目の書き込みの途中で戻す
    await settle()
    expect(log.filter(line => line.includes('start'))).toHaveLength(1) // 2 つ目はまだ始まらない
    release()
    await settle()
    release()
    await undoing
    await settle()
    const order = log.filter(line => line.startsWith('write'))
    expect(order[0]).toMatch(/^write#1 start/)
    expect(order[1]).toBe('write#1 end')
    expect(order[2]).toMatch(/^write#2 start/)
    // 戻した星（元の 0）が最後に書かれる。
    expect(order[2]).toContain('=0')
    expect(curator.currentGroup.value).toEqual(first)
  })

  it('星を読む操作（集計）は、入っている書き込みが終わってから走る', async () => {
    const { curator, log, release } = await gatedCurator(12, 4)
    void curator.toggleChoice(curator.currentGroup.value[0]!)
    await settle()
    const loading = curator.loadSummary()
    await settle()
    expect(log).not.toContain('summary') // 書き込み中は読まない
    release()
    await loading
    expect(log.indexOf('write#1 end')).toBeLessThan(log.indexOf('summary'))
  })

  it('連続して書いても、集計の読み直しは書き込みが空になったあとに 1 回', async () => {
    const { curator, log, gates } = await gatedCurator(12, 4)
    void curator.toggleChoice(curator.currentGroup.value[0]!)
    await settle()
    void curator.toggleChoice(curator.currentGroup.value[0]!)
    await settle()
    gates.shift()!.resolve(); await settle()
    gates.shift()!.resolve(); await settle()
    expect(log.filter(line => line === 'summary')).toHaveLength(1)
    expect(log.indexOf('summary')).toBeGreaterThan(log.indexOf('write#2 end'))
  })

  it('ラウンドが終わると、書き込みと集計が済んでから結果の画面になる', async () => {
    const { curator, log, release } = await gatedCurator(4, 4)
    expect(curator.view.value).toBe('tournament')
    const tap = curator.toggleChoice(curator.currentGroup.value[0]!)
    await settle()
    expect(curator.session.value!.core.finished).toBe(true)
    expect(curator.view.value).toBe('tournament') // 書き込み中はまだ結果へ移らない（結果の数字が古くならないように）
    release()
    await tap
    expect(curator.view.value).toBe('result')
    expect(log.indexOf('write#1 end')).toBeLessThan(log.indexOf('summary'))
    expect(curator.selectionSummary.value?.total).toBe(4)
  })
})

describe('W1 計測（模擬: 書き込み 30ms・集計 20ms を偽 desktop に足す）', () => {
  it('タップから次の組が出るまでの時間を出す', async () => {
    const { createCurator } = await loadCuratorModule()
    const all = photos('P', 200)
    const byId = new Map(all.map(photo => [photo.id, photo]))
    const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
    const { desktop } = fakeDesktop({
      getCoreInputs: async () => all,
      getPhotosByIds: async (_id: string, ids: string[]) => ids.map(id => byId.get(id)).filter((p): p is Photo => !!p),
      saveSelectionResults: async () => { await sleep(30) },
      getSelectionSummary: async () => { await sleep(20); return { counts: [0, 0, 0, 0, 0, 0], total: 200 } }
    })
    const curator = createCurator(desktop)
    curator.settings.groupSize = 4
    await curator.openProject(project('P', { photoCount: 200 }))
    curator.pendingTournamentSettings.value = { groupSize: 4, groupBursts: false }
    await curator.finishTournamentStart()
    await settle()
    const times: number[] = []
    for (let i = 0; i < 10; i += 1) {
      const start = performance.now()
      await curator.toggleChoice(curator.currentGroup.value[0]!)
      times.push(performance.now() - start)
    }
    await sleep(100)
    times.sort((a, b) => a - b)
    console.log(`[bench] タップ→次の組（模擬 IPC 50ms）: p50=${times[5]!.toFixed(1)}ms 最大=${times[9]!.toFixed(1)}ms`)
    expect(curator.session.value!.core.history.length).toBe(10)
  })
})
