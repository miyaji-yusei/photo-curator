// サイドカーの同期を確かめる。backend は偽物（メモリ）。判断は本物の core（wasm）。
// node:fs を使うため .mjs にしてある（`selectionFlow.test.mjs` と同じ理由）。
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import * as core from '~/lib/core'
import { asideFileName, createSidecarSync, summarize } from '~/composables/useSidecarSync'

const wasmPath = join(import.meta.dirname, '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm')

beforeAll(() => {
  core.initWithBytes(readFileSync(wasmPath))
})

const threshold = { window_ms: 4000, distance: 9, d_hash_version: 2 }
const refs = count =>
  Array.from({ length: count }, (_unused, index) => ({
    relative_path: `IMG_${index}.JPG`, captured_at: index * 60_000, d_hash: null, d_hash_version: 2
  }))

/** 写真 6 枚のプロジェクト 1 つを持つ偽の backend。 */
function fakeBackend(options = {}) {
  const files = new Map() // ファイル名 → JSON
  const writes = [] // 書いたファイル名の記録
  const backend = {
    access: options.access ?? 'readwrite',
    files,
    writes,
    failWrite: false,
    rows: refs(6).map((ref, index) => ({
      id: `id-${index}`, relativePath: ref.relative_path, rating: 0
    })),
    session: null,
    overrides: [],
    distance: null,
    state: { seenAt: 0, seenBy: '', localChanged: false },
    identity: { id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee', name: 'この PC' },
    sidecarSupported: async () => backend.access,
    readSidecar: async () => files.get('catalog.json') ?? null,
    writeSidecar: async (_projectId, json, fileName = 'catalog.json') => {
      if (backend.failWrite) throw new Error('書けません')
      files.set(fileName, json)
      writes.push(fileName)
    },
    loadSidecarState: async () => ({ ...backend.state }),
    saveSidecarState: async (_projectId, state) => { backend.state = { ...state } },
    deviceIdentity: async () => backend.identity,
    getCoreInputs: async () => backend.rows.map(row => ({ ...row })),
    saveSelectionResults: async (_projectId, entries) => {
      for (const entry of entries) {
        const row = backend.rows.find(item => item.id === entry.id)
        if (row) row.rating = entry.rating
      }
    },
    getPairOverrides: async () => backend.overrides,
    savePairOverrides: async (_projectId, overrides) => { backend.overrides = overrides },
    loadSession: async () => backend.session,
    saveSession: async (_projectId, value) => { backend.session = value },
    listProjects: async () => [{ id: 'p1', burstThreshold: backend.distance }],
    saveBurstThreshold: async (_projectId, value) => { backend.distance = value },
    clearBurstThreshold: async () => { backend.distance = null }
  }
  return backend
}

const project = { id: 'p1' }
let clock = 1_000_000
const nextClock = () => (clock += 1000)

/** 別の端末が書いた、★の付いた記録。 */
function remoteSidecar(overrides = {}) {
  const session = core.advance(core.startRound(refs(6), 4, 0, false, threshold, []), ['IMG_1.JPG', 'IMG_2.JPG'])
  return {
    version: 1,
    updatedAt: 500,
    updatedBy: 'zzzzzzzz-1111-2222-3333-444444444444',
    updatedByName: 'Pixel',
    photos: { 'IMG_1.JPG': { rating: 1 }, 'IMG_2.JPG': { rating: 3 }, 'IMG_9.JPG': { rating: 2 } },
    burstOverrides: [{ left: 'IMG_1.JPG', right: 'IMG_2.JPG', decision: 'split' }],
    sessions: { tournament: session },
    burstDistance: 7,
    ...overrides
  }
}

const seeFor = sidecar => ({ seenAt: sidecar.updatedAt, seenBy: sidecar.updatedBy })

describe('Sidecar の組み立て', () => {
  it('星は全部・手直し・学習した距離・Session を載せ、撮影時刻と指紋は載せない', async () => {
    const backend = fakeBackend()
    backend.rows[3].rating = 2
    backend.overrides = [{ left: 'IMG_0.JPG', right: 'IMG_1.JPG', decision: 'join' }]
    backend.distance = 11
    const start = core.startRound(refs(6), 4, 0, false, threshold, [])
    backend.session = { v: 2, core: start, updatedAt: 42 }
    const sync = createSidecarSync(backend, nextClock)

    const sidecar = await sync.buildSidecar(project, 777)
    expect(sidecar.updatedAt).toBe(777)
    expect(sidecar.updatedBy).toBe(backend.identity.id)
    expect(sidecar.updatedByName).toBe('この PC')
    expect(Object.keys(sidecar.photos)).toHaveLength(6)
    expect(sidecar.photos['IMG_3.JPG']).toEqual({ rating: 2 })
    expect(sidecar.photos['IMG_0.JPG']).toEqual({ rating: 0 })
    expect(sidecar.burstOverrides).toEqual(backend.overrides)
    expect(sidecar.burstDistance).toBe(11)
    expect(sidecar.sessions.tournament.round).toBe(start.round)

    const json = core.sidecarToJson(sidecar)
    expect(json).not.toContain('captured_at')
    expect(json).not.toContain('d_hash')
    // 学習していなければ距離のキーは無い。
    backend.distance = null
    const plain = await sync.buildSidecar(project, 1)
    expect(core.sidecarToJson(plain)).not.toContain('burstDistance')
  })

  it('要約は ★1 以上の枚数・ROUND・最終更新・端末名', () => {
    const summary = summarize(remoteSidecar())
    expect(summary.starred).toBe(3)
    expect(summary.round).toBe(1)
    expect(summary.updatedAt).toBe(500)
    expect(summary.deviceName).toBe('Pixel')
  })

  it('退避のファイル名は端末の id の先頭 12 文字', () => {
    expect(asideFileName('zzzzzzzz-1111-2222')).toBe('catalog.zzzzzzzz-111.json')
    expect(asideFileName('a/b:c')).toBe('catalog.a-b-c.json')
    expect(asideFileName('')).toBe('catalog.other.json')
  })
})

describe('開いたときの 4 通り', () => {
  it('Settled: 何もしない（書かない・状態も変えない）', async () => {
    const backend = fakeBackend()
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { ...seeFor(remote), localChanged: false }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('settled')
    expect(backend.writes).toEqual([])
    expect(backend.rows.every(row => row.rating === 0)).toBe(true)
    expect(backend.state.localChanged).toBe(false)
  })

  it('Push（サイドカーがまだ無い）: 端末の分を書き、seen を更新して変更なしにする', async () => {
    const backend = fakeBackend()
    backend.rows[1].rating = 1
    backend.state = { seenAt: 0, seenBy: '', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pushed')
    expect(backend.writes).toEqual(['catalog.json'])
    const written = core.sidecarFromJson(backend.files.get('catalog.json'))
    expect(written.photos['IMG_1.JPG']).toEqual({ rating: 1 })
    expect(backend.state).toEqual({
      seenAt: written.updatedAt, seenBy: backend.identity.id, localChanged: false
    })
  })

  it('Push（自分が最後に書いたものと同じ）: 変更があれば書く', async () => {
    const backend = fakeBackend()
    const mine = remoteSidecar({ updatedBy: backend.identity.id, updatedByName: 'この PC' })
    backend.files.set('catalog.json', core.sidecarToJson(mine))
    backend.state = { ...seeFor(mine), localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pushed')
    expect(backend.writes).toEqual(['catalog.json'])
  })

  it('Pull: 取り込む。Session・手直し・距離・星が置き換わり、無い写真は 0、警告は出さない', async () => {
    const backend = fakeBackend()
    backend.rows[0].rating = 4 // 記録に無い → 0 になる
    backend.rows[1].rating = 5
    backend.overrides = [{ left: 'IMG_4.JPG', right: 'IMG_5.JPG', decision: 'join' }]
    backend.distance = 3
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: false }
    const sync = createSidecarSync(backend, nextClock)

    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pulled')
    expect(backend.writes).toEqual([])
    expect(backend.rows.map(row => row.rating)).toEqual([0, 1, 3, 0, 0, 0])
    expect(backend.overrides).toEqual([{ left: 'IMG_1.JPG', right: 'IMG_2.JPG', decision: 'split' }])
    expect(backend.distance).toBe(7)
    expect(backend.session.v).toBe(2)
    expect(backend.session.core.round).toBe(remote.sessions.tournament.round)
    expect(backend.session.stage).toBe('tournament')
    expect(backend.session.settings.groupSize).toBe(4)
    expect(backend.state).toEqual({ seenAt: 500, seenBy: remote.updatedBy, localChanged: false })
    // 取り込んだ直後は、何も変えていなければ書かない。
    expect(await sync.pushIfChanged(project)).toBe(false)
  })

  it('Pull: 記録に Session も距離も無ければ、Session と距離を空にする', async () => {
    const backend = fakeBackend()
    backend.distance = 5
    backend.session = { v: 2, core: core.startRound(refs(6), 4, 0, false, threshold, []), updatedAt: 1 }
    const remote = remoteSidecar({ sessions: {}, burstDistance: undefined })
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pulled')
    expect(backend.session).toBeNull()
    expect(backend.distance).toBeNull()
  })

  it('Clash: どちらも進んでいる。何も書かず、両方の要約を返す', async () => {
    const backend = fakeBackend()
    backend.rows[0].rating = 1
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(outcome.theirsSummary).toMatchObject({ starred: 3, deviceName: 'Pixel', updatedAt: 500 })
    expect(outcome.mine).toMatchObject({ starred: 1, deviceName: 'この PC' })
    expect(backend.writes).toEqual([])
    expect(backend.rows[0].rating).toBe(1)
    expect(backend.state).toEqual({ seenAt: 100, seenBy: 'old', localChanged: true })
  })

  it('時刻の大小では決めない: 端末の時計が進んでいても、変更なしなら相手を取り込む', async () => {
    const backend = fakeBackend()
    const remote = remoteSidecar({ updatedAt: 5 }) // とても古い時刻
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 9_999_999_999_999, seenBy: 'old', localChanged: false }
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pulled')
  })

  it('Android 形式（l / r / d）の手直しを読んで取り込める', async () => {
    const backend = fakeBackend()
    const android = JSON.stringify({
      version: 1, updatedAt: 800, updatedBy: 'android-device-0001', updatedByName: 'Pixel',
      photos: { 'IMG_1.JPG': { rating: 2 } },
      burstOverrides: [{ l: 'IMG_1.JPG', r: 'IMG_2.JPG', d: 'join' }],
      sessions: {}
    })
    backend.files.set('catalog.json', android)
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pulled')
    expect(backend.overrides).toEqual([{ left: 'IMG_1.JPG', right: 'IMG_2.JPG', decision: 'join' }])
    expect(backend.rows[1].rating).toBe(2)
  })

  it('壊れたサイドカーは上書きせず、理由つきで失敗する', async () => {
    const backend = fakeBackend()
    backend.files.set('catalog.json', '{ これは JSON ではない')
    backend.state = { seenAt: 0, seenBy: '', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    await expect(sync.checkOnOpen(project)).rejects.toThrow('形式を読めません')
    expect(backend.writes).toEqual([])
  })
})

describe('食い違いの選択', () => {
  async function clashed() {
    const backend = fakeBackend()
    backend.rows[0].rating = 1
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    return { backend, remote, sync, outcome }
  }

  it('この端末の結果を使う: 相手を catalog.<id 先頭 12 文字>.json に残し、自分の分を書く', async () => {
    const { backend, remote, sync, outcome } = await clashed()
    await sync.keepMine(project, outcome.theirs)
    expect(backend.writes).toEqual(['catalog.zzzzzzzz-111.json', 'catalog.json'])
    expect(core.sidecarFromJson(backend.files.get('catalog.zzzzzzzz-111.json')).updatedByName).toBe('Pixel')
    const now = core.sidecarFromJson(backend.files.get('catalog.json'))
    expect(now.updatedBy).toBe(backend.identity.id)
    expect(now.photos['IMG_0.JPG']).toEqual({ rating: 1 })
    expect(backend.state).toEqual({ seenAt: now.updatedAt, seenBy: backend.identity.id, localChanged: false })
    expect(backend.rows[0].rating).toBe(1)
    expect(remote.updatedAt).toBe(500)
  })

  it('NAS の記録を使う: この端末の分を catalog.<自分の id 先頭 12 文字>.json に残し、相手を取り込む', async () => {
    const { backend, sync, outcome } = await clashed()
    await sync.keepTheirs(project, outcome.theirs)
    expect(backend.writes).toEqual(['catalog.aaaaaaaa-bbb.json'])
    const aside = core.sidecarFromJson(backend.files.get('catalog.aaaaaaaa-bbb.json'))
    expect(aside.photos['IMG_0.JPG']).toEqual({ rating: 1 })
    // catalog.json は相手のまま。
    expect(core.sidecarFromJson(backend.files.get('catalog.json')).updatedBy).toContain('zzzzzzzz')
    expect(backend.rows.map(row => row.rating)).toEqual([0, 1, 3, 0, 0, 0])
    expect(backend.state).toEqual({ seenAt: 500, seenBy: outcome.theirs.updatedBy, localChanged: false })
  })

  it('退避が書けなければ、どちらも実行しない（選ばなかった方を失わない）', async () => {
    const { backend, sync, outcome } = await clashed()
    backend.failWrite = true
    await expect(sync.keepTheirs(project, outcome.theirs)).rejects.toThrow('書けません')
    expect(backend.rows[0].rating).toBe(1)
    expect(backend.state).toEqual({ seenAt: 100, seenBy: 'old', localChanged: true })
  })
})

describe('食い違いの解決で書けなかったとき', () => {
  async function clashedUnwritable() {
    const backend = fakeBackend()
    backend.rows[0].rating = 1
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar()))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    backend.failWrite = true
    return { backend, sync, outcome }
  }

  for (const choice of ['mine', 'theirs']) {
    it(`${choice}: 書けなければ読むだけの出所として相手を取り込み、理由を返す`, async () => {
      const { backend, sync, outcome } = await clashedUnwritable()
      const result = await sync.resolveClash(project, outcome.theirs, choice)
      expect(result).toEqual({ kind: 'readonly', reason: '書けません' })
      expect(backend.rows.map(row => row.rating)).toEqual([0, 1, 3, 0, 0, 0])
      expect(backend.state).toEqual({ seenAt: 500, seenBy: outcome.theirs.updatedBy, localChanged: false })
    })
  }

  it('書ければ普通に実行する', async () => {
    const { backend, sync, outcome } = await clashedUnwritable()
    backend.failWrite = false
    expect(await sync.resolveClash(project, outcome.theirs, 'mine')).toEqual({ kind: 'done' })
    expect(backend.writes).toEqual(['catalog.zzzzzzzz-111.json', 'catalog.json'])
  })
})

describe('書き込み', () => {
  it('変更が無ければ書かない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
  })

  it('markChanged のあと書く。書けたら seen を更新して localChanged を戻す', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    expect(backend.state.localChanged).toBe(true)
    expect(await sync.pushIfChanged(project)).toBe(true)
    const written = core.sidecarFromJson(backend.files.get('catalog.json'))
    expect(backend.state).toEqual({ seenAt: written.updatedAt, seenBy: backend.identity.id, localChanged: false })
    // もう一度は書かない。
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toHaveLength(1)
  })

  it('書けなかったら seen も localChanged も変えない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    backend.failWrite = true
    await expect(sync.pushIfChanged(project)).rejects.toThrow('書けません')
    expect(backend.state).toEqual({ seenAt: 0, seenBy: '', localChanged: true })
    // 直ったら書ける。
    backend.failWrite = false
    expect(await sync.pushIfChanged(project)).toBe(true)
  })

  it('書いている間にまた変わったら、変更ありのまま残す', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    const original = backend.writeSidecar
    backend.writeSidecar = async (...args) => {
      await original(...args)
      await sync.markChanged('p1')
    }
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(backend.state.localChanged).toBe(true)
    expect(backend.state.seenBy).toBe(backend.identity.id)
  })

  it('載せる行が 0 件なら書かない。localChanged も落とさない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    const rows = backend.rows
    backend.rows = []
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
    expect(backend.state.localChanged).toBe(true)
    // 行が戻れば、残っていた変更が書かれる。
    backend.rows = rows
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(backend.state.localChanged).toBe(false)
  })

  it('書けない出所（readonly）へは書かない', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
  })
})

describe('書けない出所', () => {
  it('none（ピッカーなど）: 読まない・何もしない', async () => {
    const backend = fakeBackend({ access: 'none' })
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar()))
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('skipped')
    expect(backend.rows.every(row => row.rating === 0)).toBe(true)
  })

  it('readonly（開発用）: 読めれば取り込むだけ。端末に変更があってもダイアログは出さない', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    backend.rows[0].rating = 1
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar()))
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pulled')
    expect(outcome.access).toBe('readonly')
    expect(backend.rows.map(row => row.rating)).toEqual([0, 1, 3, 0, 0, 0])
    expect(backend.writes).toEqual([])
  })

  it('readonly: サイドカーが無ければ何もしない', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    backend.state = { seenAt: 0, seenBy: '', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('settled')
    expect(backend.writes).toEqual([])
  })
})
