// サイドカーの同期を確かめる。backend は偽物（メモリ）。判断は本物の core（wasm の sidecarPlan）。
// node:fs を使うため .mjs にしてある（`selectionFlow.test.mjs` と同じ理由）。
//
// U34（設計書「サイドカー同期の調査と直し方の設計」の PR-2・PR-4）で、判断を core の `sidecarPlan` に
// 切り替えた。**2 台（PC と Android）が 1 つの catalog.json を共有する**偽物で、報告された
// 「選別状況が同期の取り込みで消える」を両方の視点で再現している（「2 台で 1 つのファイル」の節）。
// Android 側は、U35 で同じ core の判断に切り替える前提で、同じ `createSidecarSync` を
// 「Android の端末」として動かしている（判断の表は core が 1 か所に持つ）。
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import * as core from '~/lib/core'
import { createSidecarSync, summarize } from '~/composables/useSidecarSync'
import { asideName, asideStamp, asideTag, asidesToDrop } from '~/utils/sidecarAside'

const wasmPath = join(import.meta.dirname, '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm')

beforeAll(() => {
  core.initWithBytes(readFileSync(wasmPath))
})

const threshold = { window_ms: 4000, distance: 9, d_hash_version: 2 }
const refs = (count, names) =>
  Array.from({ length: count }, (_unused, index) => ({
    relative_path: names?.[index] ?? `IMG_${index}.JPG`, captured_at: index * 60_000, d_hash: null, d_hash_version: 2
  }))

const PC = { id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee', name: 'この PC' }
const ANDROID = { id: 'zzzzzzzz-1111-2222-3333-444444444444', name: 'Pixel' }

/** 2 台が共有する NAS の `.photo-curator/`（メモリ上）。 */
function sharedNas() {
  return {
    files: new Map(), // ファイル名 → JSON
    writes: [], // 書いたファイル名の記録（どの端末からでも）
    failWrite: false,
    locked: false,
    /** 楽観ロックの書き込みで、「読んで確かめる」直前に 1 回だけ呼ぶ（別の端末の割り込み）。 */
    beforeCheck: null,
    /** 退避の名前の時刻（偽物の時計。退避のたびに 1 秒進める）。 */
    asideClock: Date.UTC(2026, 9, 3, 0, 0, 0)
  }
}

/**
 * NAS の退避（U52 D4。Android の U44 と同じ形）: `catalog.<端末>.<UTC yyyyMMddHHmmss>.json` を無いときだけ作り、
 * 同じ秒なら `-2` 以降。書けたら同じ端末の時刻つきの退避は新しい 5 つだけ残す。
 */
function fakeAside(nas, json, owner) {
  if (nas.failWrite) throw new Error('書けません')
  nas.asideClock += 1000
  const stamp = asideStamp(nas.asideClock)
  let name = null
  for (let n = 1; n <= 9 && !name; n++) {
    const candidate = asideName(owner, stamp, n)
    if (!nas.files.has(candidate)) name = candidate
  }
  if (!name) throw new Error('退避の名前がありません')
  nas.files.set(name, json)
  nas.writes.push(name)
  for (const old of asidesToDrop([...nas.files.keys()], owner, 5)) nas.files.delete(old)
  return name
}

/** その端末の印の、時刻つきの退避の名前か。 */
const asideRe = id => new RegExp(`^catalog\\.${asideTag(id)}\\.\\d{14}(-\\d+)?\\.json$`)
/** その端末の印の、いちばん新しい退避の名前（無ければ undefined）。 */
const latestAsideName = (backend, id) => [...backend.files.keys()].filter(name => asideRe(id).test(name)).sort().pop()
/** その端末の印の、いちばん新しい退避の中身（Sidecar）。 */
const latestAside = (backend, id) => {
  const name = latestAsideName(backend, id)
  return name ? core.sidecarFromJson(backend.files.get(name)) : null
}
const anAside = id => expect.stringMatching(asideRe(id))

/** 写真 6 枚のプロジェクト 1 つを持つ、1 台ぶんの偽の backend。`nas` を渡すと 2 台で共有する。 */
function fakeBackend(options = {}) {
  const nas = options.nas ?? sharedNas()
  const names = options.names
  const backend = {
    access: options.access ?? 'readwrite',
    nas,
    files: nas.files,
    writes: nas.writes,
    get failWrite() { return nas.failWrite },
    set failWrite(value) { nas.failWrite = value },
    rows: refs(6, names).map((ref, index) => ({
      id: `id-${index}`, relativePath: ref.relative_path, rating: 0
    })),
    session: null,
    overrides: [],
    distance: null,
    folderPath: options.folderPath ?? 'C:\\photos\\x',
    state: { seenAt: 0, seenBy: '', localChanged: false },
    identity: options.identity ?? PC,
    sidecarSupported: async () => backend.access,
    readSidecar: async () => nas.files.get('catalog.json') ?? null,
    writeSidecar: async (_projectId, json, fileName = 'catalog.json') => {
      if (nas.failWrite) throw new Error('書けません')
      nas.files.set(fileName, json)
      nas.writes.push(fileName)
    },
    asideSidecar: async (_projectId, json, owner) => fakeAside(nas, json, owner),
    /** 端末の中の退避（U52 D4。PC はアプリのデータフォルダの aside/、Web は IndexedDB）。 */
    localAsides: [],
    failLocalAside: false,
    asideLocal: async (_projectId, json) => {
      if (backend.failLocalAside) throw new Error('端末に控えられません')
      backend.localAsides.push(json)
    },
    writeSidecarChecked: async (_projectId, json, expected, asideOwner) => {
      if (nas.failWrite) throw new Error('書けません')
      if (nas.locked) return 'locked'
      const hook = nas.beforeCheck
      nas.beforeCheck = null
      if (hook) await hook()
      const current = nas.files.get('catalog.json') ?? null
      if (current !== expected) return 'changed'
      // ロックを取って確かめたあとで、置き換える版を退避する（退避が書けなければ書かない）。
      if (asideOwner && current !== null) fakeAside(nas, current, asideOwner)
      nas.files.set('catalog.json', json)
      nas.writes.push('catalog.json')
      return 'written'
    },
    loadSidecarState: async () => ({ ...backend.state }),
    saveSidecarState: async (_projectId, state) => { backend.state = { ...state } },
    deviceIdentity: async () => backend.identity,
    // 行は欠損（isMissing）を除いて読む（PC・Web の getCoreInputs と同じ）。
    getCoreInputs: async () => backend.rows.filter(row => !row.isMissing).map(row => ({ ...row })),
    // U52 D13: 欠損の行のうち星が 1 以上のもの。
    getMissingRatings: async () => backend.rows
      .filter(row => row.isMissing && row.rating >= 1)
      .map(row => ({ relativePath: row.relativePath, rating: row.rating })),
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
    listProjects: async () => [{
      id: 'p1', burstThreshold: backend.distance, folderPath: backend.folderPath,
      pairRawJpeg: backend.pair.value, pairRawJpegAt: backend.pair.at
    }],
    saveBurstThreshold: async (_projectId, value) => { backend.distance = value },
    clearBurstThreshold: async () => { backend.distance = null },
    // U48: プロジェクトの「同名の JPEG と RAW を 1 枚の写真として扱う」と、切り替えた時刻。
    pair: options.pair ?? { value: true, at: 0 },
    pairSaves: [],
    saveProjectPairRaw: async (_projectId, enabled, at) => {
      backend.pair = { value: enabled, at: at ?? clock }
      backend.pairSaves.push({ ...backend.pair })
      return enabled
    }
  }
  return backend
}

const project = { id: 'p1' }
let clock = 1_000_000
const nextClock = () => (clock += 1000)

const envelope = session => ({
  v: 2,
  core: session,
  stage: session.finished ? 'result' : 'tournament',
  settings: { groupSize: session.group_size, groupBursts: false },
  multiSelect: false,
  selectedInGroup: [],
  learning: null,
  burstDistance: null,
  updatedAt: 1
})

/** 端末で選別を進める（行の星も Session に合わせる。PC の applyCore と同じ）。 */
function play(backend, picks, groupSize = 2) {
  let session = core.startRound(refs(6, backend.rows.map(row => row.relativePath)), groupSize, 0, false, threshold, [])
  for (const chosen of picks) session = core.advance(session, chosen)
  backend.session = envelope(session)
  for (const row of backend.rows) row.rating = session.ratings[row.relativePath] ?? 0
  return session
}

/** 「選別を開始」を押しただけ（判断 0 件）。 */
const startOnly = backend => play(backend, [])

/** 別の端末が書いた、★の付いた記録（Session と星がそろっている）。 */
function remoteSidecar(overrides = {}) {
  const session = core.advance(core.startRound(refs(6), 4, 0, false, threshold, []), ['IMG_1.JPG', 'IMG_2.JPG'])
  const photos = {}
  for (const [path, rating] of Object.entries(session.ratings)) photos[path] = { rating }
  return {
    version: 1,
    updatedAt: 500,
    updatedBy: ANDROID.id,
    updatedByName: 'Pixel',
    photos,
    burstOverrides: [{ left: 'IMG_1.JPG', right: 'IMG_2.JPG', decision: 'split' }],
    sessions: { tournament: session },
    burstDistance: 7,
    ...overrides
  }
}

const seeFor = sidecar => ({ seenAt: sidecar.updatedAt, seenBy: sidecar.updatedBy })
const nasCatalog = backend => core.sidecarFromJson(backend.files.get('catalog.json'))
const ratingsOf = backend => backend.rows.map(row => row.rating)

describe('Sidecar の組み立て', () => {
  it('星は全部・手直し・学習した距離・Session を載せ、撮影時刻とハッシュ値は載せない', async () => {
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
    const summary = summarize(remoteSidecar({ photos: { 'IMG_1.JPG': { rating: 1 }, 'IMG_2.JPG': { rating: 3 }, 'IMG_9.JPG': { rating: 2 } } }))
    expect(summary.starred).toBe(3)
    expect(summary.round).toBe(1)
    expect(summary.updatedAt).toBe(500)
    expect(summary.deviceName).toBe('Pixel')
  })

  it('退避のファイル名は Android（U44）と同じ: catalog.<端末の印 12 文字>.<UTC yyyyMMddHHmmss>(-n).json', () => {
    expect(asideTag('zzzzzzzz-1111-2222')).toBe('zzzzzzzz-111')
    expect(asideTag('a/b:c_d')).toBe('abc_d')
    expect(asideTag('')).toBe('unknown')
    expect(asideStamp(Date.UTC(2026, 9, 3, 4, 5, 6))).toBe('20261003040506')
    expect(asideName('zzzzzzzz-111', '20261003040506')).toBe('catalog.zzzzzzzz-111.20261003040506.json')
    expect(asideName('zzzzzzzz-111', '20261003040506', 2)).toBe('catalog.zzzzzzzz-111.20261003040506-2.json')
    // 同じ端末の時刻つきだけを、新しい 5 つ残して消す（時刻の無い古い名前・ほかの端末の退避は消さない）。
    const names = [
      'catalog.json', 'catalog.lock', 'catalog.abc.json', 'catalog.other.20200101000000.json',
      ...['20260101000001', '20260101000002', '20260101000003', '20260101000004', '20260101000005']
        .map(stamp => `catalog.abc.${stamp}.json`),
      'catalog.abc.20260101000005-2.json', 'catalog.abc.20260101000006.json'
    ]
    expect(asidesToDrop(names, 'abc', 5).sort())
      .toEqual(['catalog.abc.20260101000001.json', 'catalog.abc.20260101000002.json'])
  })
})

describe('開いたとき（core の sidecarPlan）', () => {
  it('Settled: 見た版のままで端末も変わっていなければ、何もしない（書かない・取り込まない）', async () => {
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

  it('Push（サイドカーがまだ無い）: 端末の分を書き、控えを書いた版にする', async () => {
    const backend = fakeBackend()
    backend.rows[1].rating = 1
    backend.state = { seenAt: 0, seenBy: '', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pushed')
    expect(backend.writes).toEqual(['catalog.json'])
    const written = nasCatalog(backend)
    expect(written.photos['IMG_1.JPG']).toEqual({ rating: 1 })
    expect(written.version).toBe(2)
    expect(written.keyBase).toBe('folder')
    expect(written.writeId).toBeTruthy()
    // 状態の形が増えたので toMatchObject（古い 3 つの値は今までどおり）。
    expect(backend.state).toMatchObject({
      seenAt: written.updatedAt, seenBy: backend.identity.id, localChanged: false, seenToken: written.writeId
    })
  })

  it('Push（自分が最後に書いたものと同じ）: 変更があれば書く', async () => {
    const backend = fakeBackend()
    const mine = remoteSidecar({ updatedBy: backend.identity.id, updatedByName: 'この PC' })
    backend.files.set('catalog.json', core.sidecarToJson(mine))
    backend.state = { ...seeFor(mine), localChanged: true }
    // 端末には進んだ分がある（U34: 未着手の端末で着手済みの版を置き換えるときは退避が増えるため、端末に分を持たせる）。
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pushed')
    expect(backend.writes).toEqual(['catalog.json'])
  })

  it('Pull（端末が未着手）: 確認なしに取り込む。Session・手直し・距離・星が NAS の分になる', async () => {
    const backend = fakeBackend()
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: false }
    const sync = createSidecarSync(backend, nextClock)

    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pulled')
    expect(outcome.reason).toBe('LocalUntouched')
    expect(backend.writes).toEqual([])
    expect(ratingsOf(backend)).toEqual([0, 1, 1, 0, 0, 0])
    expect(backend.overrides).toEqual([{ left: 'IMG_1.JPG', right: 'IMG_2.JPG', decision: 'split' }])
    expect(backend.distance).toBe(7)
    expect(backend.session.v).toBe(2)
    expect(backend.session.core.round).toBe(remote.sessions.tournament.round)
    expect(backend.session.stage).toBe('tournament')
    expect(backend.session.settings.groupSize).toBe(4)
    expect(backend.state).toMatchObject({ seenAt: 500, seenBy: remote.updatedBy, localChanged: false })
    expect(backend.state.seenToken).toBe(core.sidecarToken(remote))
    // 取り込んだ直後は、何も変えていなければ書かない。
    expect(await sync.pushIfChanged(project)).toBe(false)
  })

  it('Pull（端末が未着手）: 記録に無い写真は★0、Session の無い記録は行の星（photos）から取り込む', async () => {
    const backend = fakeBackend()
    const remote = remoteSidecar({ sessions: {}, photos: { 'IMG_3.JPG': { rating: 2 } } })
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pulled')
    expect(ratingsOf(backend)).toEqual([0, 0, 0, 2, 0, 0])
  })

  // 仕様の変更（U34・ユーザーの決定。設計書 §1.6・PR-2 の 1）: 以前は「相手に Session が無ければ端末の Session を
  // 消す」を仕様として固定していた。それが「選別状況が同期の取り込みで消える」の一因だったので、
  // **端末が未着手でない限り、相手に無い Session・境目は消さない**に反転した。
  it('取り込み: 相手に Session も距離も無くても、端末の進んだ Session と距離は消さない', async () => {
    const backend = fakeBackend()
    backend.distance = 5
    const before = play(backend, [['IMG_0.JPG'], ['IMG_2.JPG']])
    const remote = remoteSidecar({ sessions: {}, burstDistance: undefined, photos: { 'IMG_4.JPG': { rating: 3 } } })
    const sync = createSidecarSync(backend, nextClock)
    await sync.adopt(project, remote)
    expect(backend.session).not.toBeNull()
    expect(backend.session.core.history).toHaveLength(before.history.length)
    expect(backend.distance).toBe(5)
    // 星は取り込む（行と Session の星をそろえる。開いたときの自己修復で戻されないように）。
    expect(ratingsOf(backend)).toEqual([0, 0, 0, 0, 3, 0])
    expect(backend.session.core.ratings['IMG_4.JPG']).toBe(3)
    expect(backend.session.core.ratings['IMG_0.JPG']).toBe(0)
  })

  it('取り込み: 端末が未着手なら、相手に Session が無いとき端末の Session を空にする', async () => {
    const backend = fakeBackend()
    startOnly(backend)
    const sync = createSidecarSync(backend, nextClock)
    await sync.adopt(project, remoteSidecar({ sessions: {} }))
    expect(backend.session).toBeNull()
  })

  it('Clash: どちらも進んでいて中身が違う。何も書かず、両方の要約と見込みを返す', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(outcome.clash.reason).toBe('Diverged')
    expect(outcome.clash.theirsName).toBe('Pixel')
    expect(outcome.clash.theirsAt).toBe(500)
    expect(outcome.clash.mineName).toBe('この PC')
    expect(outcome.clash.theirsProgress).toMatchObject({ started: true, starred: 2, decided: 1 })
    expect(outcome.clash.mineProgress).toMatchObject({ started: true, starred: 1, decided: 1 })
    expect(outcome.clash.preview.union_starred).toBe(3)
    expect(backend.writes).toEqual([])
    expect(backend.rows[0].rating).toBe(1)
    expect(backend.state).toEqual({ seenAt: 100, seenBy: 'old', localChanged: true })
  })

  it('時刻の大小では決めない: 端末の時計が進んでいても、端末が未着手なら相手を取り込む', async () => {
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
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    await expect(sync.checkOnOpen(project)).rejects.toThrow('形式を読めません')
    expect(backend.writes).toEqual([])
  })

  it('意味が同じなら、並び・空白・updatedAt・updatedBy が違っても何もしない（警告も書き込みも無し）', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG'], ['IMG_3.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    // 端末と同じ選別状況を、別の端末が別の書き方で書いた（PC は★0 の写真も載せるが、Android は載せない）。
    const mine = await sync.buildSidecar(project, 1)
    const rewritten = { ...mine, updatedAt: 99_999, updatedBy: ANDROID.id, updatedByName: 'Pixel', photos: {} }
    const text = JSON.stringify(Object.fromEntries(Object.entries(JSON.parse(core.sidecarToJson(rewritten))).reverse()), null, 2)
    backend.files.set('catalog.json', text)
    backend.state = { seenAt: 1, seenBy: 'old', localChanged: true }
    const outcome = await sync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'settled', reason: 'Same' })
    expect(backend.writes).toEqual([])
    expect(backend.state.seenToken).toBe(core.sidecarToken(core.sidecarFromJson(text)))
  })

  it('写真の場所が違う記録（鍵の一致が半分未満）は取り込まずに理由を返す', async () => {
    const backend = fakeBackend()
    const session = core.advance(core.startRound(refs(6, ['a/1.JPG', 'a/2.JPG', 'a/3.JPG', 'a/4.JPG', 'a/5.JPG', 'a/6.JPG']), 2, 0, false, threshold, []), ['a/1.JPG'])
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar({ photos: {}, burstOverrides: [], sessions: { tournament: session }, keyBase: 'folder' })))
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'mismatch', matched: 0 })
    expect(backend.session).toBeNull()
  })
})

describe('2 台で 1 つのファイル（報告されたシナリオ）', () => {
  function twoDevices() {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    return { nas, pc, android, pcSync: createSidecarSync(pc, nextClock), androidSync: createSidecarSync(android, nextClock) }
  }

  it('Android が選別を進めて書いたあと、PC が「選別を開始」だけで書いても、Android の版を上書きしない', async () => {
    const { pc, android, pcSync, androidSync } = twoDevices()
    await androidSync.checkOnOpen(project) // 最初の確認（まだ無い）
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    expect(await androidSync.pushIfChanged(project)).toBe(true) // ラウンドの途中で書いた（A1）
    const a1 = pc.files.get('catalog.json')

    startOnly(pc) // PC は判断 0 件
    expect(await pcSync.pushIfChanged(project)).toBe(false) // 窓を隠す・ホームへ戻る
    expect(pc.files.get('catalog.json')).toBe(a1)

    // Android がプロジェクト画面に戻る → 何もしない。Android の選別状況はそのまま。
    const androidBefore = JSON.stringify(android.session)
    expect((await androidSync.checkOnOpen(project)).kind).toBe('settled')
    expect(JSON.stringify(android.session)).toBe(androidBefore)
    expect(ratingsOf(android)).toEqual([1, 0, 1, 0, 0, 0])
  })

  it('PC 側の視点: PC が未着手なら、開いた（開始の前の）確認で Android の進んだ版を確認なしに取り込む', async () => {
    const { pc, android, pcSync, androidSync } = twoDevices()
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await androidSync.pushIfChanged(project)
    startOnly(pc)
    const outcome = await pcSync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'pulled', reason: 'LocalUntouched' })
    expect(ratingsOf(pc)).toEqual([1, 0, 1, 0, 0, 0])
    expect(pc.session.core.history).toHaveLength(2)
    // 書いていない（NAS は Android の版のまま）。
    expect(pc.writes).toEqual(['catalog.json'])
  })

  it('古い PC が書いた未着手の版（P1）が Android の上に来ても、Android は取り込まずに自分の分を書き、P1 は退避する', async () => {
    const { pc, android, androidSync } = twoDevices()
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await androidSync.pushIfChanged(project) // A1。Android の控え＝A1
    // 古い版の PC（U34 より前）が、判断 0 件の Session を確かめずに書いた（writeId 無し・PC 形式の鍵）。
    const untouched = core.startRound(refs(6), 4, 0, false, threshold, [])
    const p1 = {
      version: 1, updatedAt: 9_000_000, updatedBy: PC.id, updatedByName: 'DESKTOP-ABC',
      photos: Object.fromEntries(refs(6).map(ref => [ref.relative_path, { rating: 0 }])),
      burstOverrides: [], sessions: { tournament: untouched }
    }
    pc.files.set('catalog.json', core.sidecarToJson(p1))

    const androidBefore = JSON.stringify(android.session)
    const outcome = await androidSync.checkOnOpen(project) // 選別画面から戻った
    expect(outcome).toMatchObject({ kind: 'pushed', reason: 'TheirsUntouched' })
    expect(JSON.stringify(android.session)).toBe(androidBefore)
    expect(nasCatalog(android).updatedBy).toBe(ANDROID.id)
    expect(latestAside(android, PC.id).updatedByName).toBe('DESKTOP-ABC')
  })

  it('PC 側の視点: PC も進んでいるときは、Android の進んだ版を黙って取り込まずに確認する（端末の分を失わない）', async () => {
    const { pc, android, pcSync, androidSync } = twoDevices()
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await androidSync.pushIfChanged(project)
    play(pc, [['IMG_1.JPG']])
    const outcome = await pcSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(ratingsOf(pc)).toEqual([0, 1, 0, 0, 0, 0])
    expect(nasCatalog(pc).updatedBy).toBe(ANDROID.id)
  })

  it('早送り: PC が Android の版から続けて書いたら、Android は確認なしに取り込み、自分の分を NAS に退避する', async () => {
    const { pc, android, pcSync, androidSync } = twoDevices()
    play(android, [['IMG_0.JPG']])
    await androidSync.pushIfChanged(project) // A1
    expect((await pcSync.checkOnOpen(project)).kind).toBe('pulled') // PC は未着手 → 取り込む
    const continued = core.advance(pc.session.core, ['IMG_2.JPG'])
    pc.session = envelope(continued)
    for (const row of pc.rows) row.rating = continued.ratings[row.relativePath] ?? 0
    expect(await pcSync.pushIfChanged(project)).toBe(true) // P2（A1 から続けた）

    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'pulled', reason: 'FastForward' })
    expect(android.session.core.history).toHaveLength(2)
    expect(ratingsOf(android)).toEqual(ratingsOf(pc))
    expect(latestAsideName(android, ANDROID.id)).toBeDefined()
  })

  it('PC が書いた（\\ 区切りの）入れ子の写真の星を、Android 形式の古い記録とも行き来できる', async () => {
    const nas = sharedNas()
    const names = ['IMG_0.JPG', 'IMG_1.JPG', 'IMG_2.JPG', 'sub\\IMG_3.JPG', 'sub\\IMG_4.JPG', 'IMG_5.JPG']
    const pc = fakeBackend({ nas, names, folderPath: '\\\\NAS\\share\\photo\\x' })
    // 古い Android（U35 より前）の形: 共有の根からの相対（フォルダ名付き）・keyBase なし。
    const androidSession = core.advance(
      core.startRound(refs(6, names.map(name => `photo/x/${name.replace('\\', '/')}`)), 2, 0, false, threshold, []),
      ['photo/x/IMG_0.JPG']
    )
    const advanced = core.advance(androidSession, ['photo/x/sub/IMG_3.JPG'])
    nas.files.set('catalog.json', JSON.stringify({
      version: 1, updatedAt: 10, updatedBy: ANDROID.id, updatedByName: 'Pixel',
      photos: {}, burstOverrides: [], sessions: { tournament: advanced }
    }))
    const sync = createSidecarSync(pc, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pulled')
    expect(ratingsOf(pc)).toEqual([1, 0, 0, 1, 0, 0])
    expect(Object.keys(pc.session.core.ratings)).toContain('sub\\IMG_3.JPG')

    // PC が続けて書くと、鍵は「選んだフォルダからの相対・/ 区切り」になる。
    const next = core.advance(pc.session.core, ['IMG_5.JPG'])
    pc.session = envelope(next)
    for (const row of pc.rows) row.rating = next.ratings[row.relativePath] ?? 0
    expect(await sync.pushIfChanged(project)).toBe(true)
    const written = nasCatalog(pc)
    expect(written.keyBase).toBe('folder')
    expect(Object.keys(written.photos)).toContain('sub/IMG_3.JPG')
    expect(Object.keys(written.sessions.tournament.ratings)).toContain('sub/IMG_3.JPG')
    expect(JSON.stringify(written)).not.toContain('\\\\')
  })
})

describe('書き込み（楽観ロック）', () => {
  it('変更が無ければ書かない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
  })

  it('判断が変わったら書く。書けたら控えを書いた版にし、もう一度は書かない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    // U34: 書くかどうかは印ではなく中身（比較キー）で決める。印だけでは書かない。
    await sync.markChanged('p1')
    expect(backend.state.localChanged).toBe(true)
    expect(await sync.pushIfChanged(project)).toBe(false)
    play(backend, [['IMG_0.JPG']])
    await sync.markChanged('p1')
    expect(await sync.pushIfChanged(project)).toBe(true)
    const written = nasCatalog(backend)
    expect(backend.state).toMatchObject({ seenAt: written.updatedAt, seenBy: backend.identity.id, localChanged: false })
    // もう一度は書かない。
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toHaveLength(1)
  })

  it('書けなかったら seen も localChanged も変えない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    play(backend, [['IMG_0.JPG']])
    await sync.markChanged('p1')
    backend.failWrite = true
    await expect(sync.pushIfChanged(project)).rejects.toThrow('書けません')
    expect(backend.state).toEqual({ seenAt: 0, seenBy: '', localChanged: true })
    // 直ったら書ける。
    backend.failWrite = false
    expect(await sync.pushIfChanged(project)).toBe(true)
  })

  it('開いたあとに相手が書いた版の上には、自動の書き込みは書かない（読んで確かめる）', async () => {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    play(android, [['IMG_0.JPG']])
    await androidSync.pushIfChanged(project) // A1
    await pcSync.checkOnOpen(project) // PC は未着手 → 取り込む
    const pcNext = core.advance(pc.session.core, ['IMG_3.JPG'])
    pc.session = envelope(pcNext)
    for (const row of pc.rows) row.rating = pcNext.ratings[row.relativePath] ?? 0
    // その間に Android が別の続きを書いた（A2）。
    const androidNext = core.advance(android.session.core, ['IMG_2.JPG'])
    android.session = envelope(androidNext)
    for (const row of android.rows) row.rating = androidNext.ratings[row.relativePath] ?? 0
    await androidSync.pushIfChanged(project)
    const a2 = nas.files.get('catalog.json')

    expect(await pcSync.pushIfChanged(project)).toBe(false) // 窓を隠した
    expect(nas.files.get('catalog.json')).toBe(a2)
    expect((await pcSync.checkOnOpen(project)).kind).toBe('clash') // 次に開いたときに確認
  })

  it('判断してから書くまでの間に別の端末が書いたら、書かずに判定し直す', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    const other = core.sidecarToJson(remoteSidecar({ writeId: 'w-other' }))
    backend.nas.beforeCheck = async () => { backend.files.set('catalog.json', other) }
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.files.get('catalog.json')).toBe(other)
    expect(backend.state.seenToken ?? null).toBeNull()
  })

  it('ほかの端末が書いている最中（ロック）なら書かず、控えも変えない', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    backend.nas.locked = true
    const sync = createSidecarSync(backend, nextClock)
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
    expect(backend.state).toEqual({ seenAt: 0, seenBy: '', localChanged: false })
    backend.nas.locked = false
    expect(await sync.pushIfChanged(project)).toBe(true)
  })

  it('書いている間に判断が変わったら、次も変更ありとして書く（控えは書いた写しの比較キー）', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    backend.nas.beforeCheck = async () => { // 書いている最中の判断（次の組で IMG_2 を残した）
      const next = core.advance(backend.session.core, ['IMG_2.JPG'])
      backend.session = envelope(next)
      for (const row of backend.rows) row.rating = next.ratings[row.relativePath] ?? 0
    }
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(nasCatalog(backend).sessions.tournament.ratings['IMG_2.JPG']).toBe(0)
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(nasCatalog(backend).sessions.tournament.ratings['IMG_2.JPG']).toBe(1)
    expect(await sync.pushIfChanged(project)).toBe(false)
  })

  it('markChanged は書き込みの列に並び、書き終えた控え（seen）を古い値に戻さない', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    let pending = null
    backend.nas.beforeCheck = async () => { pending = sync.markChanged('p1') } // 書いている最中に付いた印
    expect(await sync.pushIfChanged(project)).toBe(true)
    await pending
    const written = nasCatalog(backend)
    expect(backend.state.seenToken).toBe(written.writeId)
    expect(backend.state.seenAt).toBe(written.updatedAt)
    // 中身は変わっていないので、次は書かない（以前は seen が戻り、自分の版を他人の変更と読み違えた）。
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect((await sync.checkOnOpen(project)).kind).toBe('settled')
  })

  it('載せる行が 0 件なら書かない。行が戻れば書く', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    play(backend, [['IMG_0.JPG']])
    await sync.markChanged('p1')
    const rows = backend.rows
    const saved = backend.session
    backend.rows = []
    backend.session = null
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
    expect(backend.state.localChanged).toBe(true)
    // 行が戻れば、残っていた変更が書かれる。
    backend.rows = rows
    backend.session = saved
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(backend.state.localChanged).toBe(false)
  })

  it('書けない出所（readonly）へは書かない', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    await sync.markChanged('p1')
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes).toEqual([])
  })

  it('古い控え（seenAt/seenBy だけ）から legacy: の控えを作り、自分の書いた古い版を他人の変更と読み違えない', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    // U34 より前の PC が書いた版（writeId なし）と、その控え。
    const old = { ...(await sync.buildSidecar(project, 4242)), version: 1 }
    backend.files.set('catalog.json', core.sidecarToJson(old))
    backend.state = { seenAt: 4242, seenBy: PC.id, localChanged: false }
    expect((await sync.checkOnOpen(project)).kind).toBe('settled')
    expect(backend.writes).toEqual([])
  })
})

describe('食い違いの 5 択', () => {
  async function clashed() {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    // Android: ROUND 1 の途中（IMG_0 と IMG_2 を残した）。PC: 別の 2 組（IMG_1 と IMG_2 を残した）。
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await androidSync.pushIfChanged(project)
    play(pc, [['IMG_1.JPG'], ['IMG_2.JPG']])
    const outcome = await pcSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    pc.nas.writes.length = 0
    return { nas, pc, android, pcSync, androidSync, clash: outcome.clash }
  }

  it('A 取り込む: 端末の分を NAS に退避し、NAS の分（星・Session・手直し・境目）にする', async () => {
    const { pc, pcSync, clash } = await clashed()
    const result = await pcSync.resolveClash(project, clash, 'theirs')
    expect(result.kind).toBe('done')
    expect(pc.writes).toEqual([anAside(PC.id)])
    const aside = latestAside(pc, PC.id)
    expect(aside.sessions.tournament.ratings['IMG_1.JPG']).toBe(1)
    expect(ratingsOf(pc)).toEqual([1, 0, 1, 0, 0, 0])
    expect(nasCatalog(pc).updatedBy).toBe(ANDROID.id)
    expect(pc.state.seenToken).toBe(clash.theirs.writeId)
    expect(pc.state.detached).toBe(false)
  })

  it('B 残す: NAS には触らず切り離す。自動では書かず、NAS がまた変われば聞き直す', async () => {
    const { pc, android, pcSync, androidSync, clash } = await clashed()
    expect((await pcSync.resolveClash(project, clash, 'keep')).kind).toBe('done')
    expect(pc.writes).toEqual([])
    expect(ratingsOf(pc)).toEqual([0, 1, 1, 0, 0, 0])
    expect(pc.state.detached).toBe(true)
    expect(await pcSync.pushIfChanged(project)).toBe(false)
    expect(await pcSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', reason: 'Detached' })
    // Android が続きを書いた → もう一度聞く。
    const next = core.advance(android.session.core, ['IMG_4.JPG'])
    android.session = envelope(next)
    for (const row of android.rows) row.rating = next.ratings[row.relativePath] ?? 0
    await androidSync.pushIfChanged(project)
    expect((await pcSync.checkOnOpen(project)).kind).toBe('clash')
  })

  it('B のあと「NAS に書き込む」: C と同じく、NAS の版を退避して端末の分を書く', async () => {
    const { pc, pcSync, clash } = await clashed()
    await pcSync.resolveClash(project, clash, 'keep')
    expect(await pcSync.writeToNas(project)).toBe(true)
    expect(pc.writes).toEqual([anAside(ANDROID.id), 'catalog.json'])
    expect(nasCatalog(pc).updatedBy).toBe(PC.id)
    expect(pc.state.detached).toBe(false)
  })

  it('C 書き込む: NAS の版を退避して端末の分を書く。Android は次に開いたとき確認なしに取り込む', async () => {
    const { pc, android, pcSync, androidSync, clash } = await clashed()
    expect((await pcSync.resolveClash(project, clash, 'mine')).kind).toBe('done')
    expect(pc.writes).toEqual([anAside(ANDROID.id), 'catalog.json'])
    const written = nasCatalog(pc)
    expect(written.updatedBy).toBe(PC.id)
    expect(written.basedOn).toBe(clash.theirs.writeId)
    expect(pc.state.seenToken).toBe(written.writeId)
    expect(ratingsOf(pc)).toEqual([0, 1, 1, 0, 0, 0])
    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'pulled', reason: 'FastForward' })
    expect(ratingsOf(android)).toEqual([0, 1, 1, 0, 0, 0])
  })

  for (const [choice, mode, expected] of [
    ['intersection', 'Intersection', null],
    ['union', 'Union', [1, 1, 1, 0, 0, 0]]
  ]) {
    it(`${choice === 'intersection' ? 'D 積集合' : 'E 和集合'}: まだどちらも見ていない写真は続きから選別できる形にし、両方を退避して NAS にも書く（U45）`, async () => {
      const { pc, android, pcSync, androidSync, clash } = await clashed()
      const mineJ = core.sidecarJudgement(core.sidecarKeysToFolder(await pcSync.buildSidecar(project, 1), ''))
      const theirsJ = core.sidecarJudgement(clash.theirs)
      const stars = core.mergeJudgements(mineJ, theirsJ, mode, 2, 'e').ratings
      const want = expected ?? pc.rows.map(row => stars[row.relativePath] ?? 0)

      expect((await pcSync.resolveClash(project, clash, choice)).kind).toBe('done')
      expect(pc.writes).toEqual([anAside(PC.id), anAside(ANDROID.id), 'catalog.json'])
      expect(ratingsOf(pc)).toEqual(want)
      // U45: 両方とも IMG_4・IMG_5 をまだ見ていない → 完了にせず、選別画面で続きから出す（以前は完了・結果画面）。
      expect(pc.session.core.finished).toBe(false)
      expect([...pc.session.core.current, ...pc.session.core.queue]).toEqual(['IMG_4.JPG', 'IMG_5.JPG'])
      expect(pc.session.core.history).toEqual([])
      expect(pc.session.stage).toBe('tournament')
      const written = nasCatalog(pc)
      expect(written.sessions.tournament.finished).toBe(false)
      expect(written.sessions.tournament.current).toEqual(['IMG_4.JPG', 'IMG_5.JPG'])
      // Android は確認なしに混ぜた結果を取り込む（早送り）。
      expect((await androidSync.checkOnOpen(project)).kind).toBe('pulled')
      expect(ratingsOf(android)).toEqual(want)
      // もう落ち着いている。
      expect((await pcSync.checkOnOpen(project)).kind).toBe('settled')
    })
  }

  it('U45: ROUND が違えば D・E は押せない扱いで、選ばれても何も変えずに理由を返す', async () => {
    const { pc, pcSync, clash } = await clashed()
    // PC だけ ROUND 2（★1 から）の途中にいることにする。
    pc.session = envelope({ ...pc.session.core, round: 2, target_star: 1 })
    const before = pc.files.get('catalog.json')
    const result = await pcSync.resolveClash(project, clash, 'union')
    expect(result.kind).toBe('failed')
    expect(result.reason).toContain('混ぜられません')
    expect(pc.writes).toEqual([])
    expect(pc.files.get('catalog.json')).toBe(before)
    expect(pc.session.core.round).toBe(2)
  })

  it('積集合: 片方で未判定の写真は判定済みの側の★を採る（Android で未判定の IMG_4 は PC の★）', async () => {
    const { pc, pcSync, clash } = await clashed()
    // PC はもう 1 組進めて IMG_4 を残した。Android では IMG_4 はまだ見ていない。
    const next = core.advance(pc.session.core, ['IMG_4.JPG'])
    pc.session = envelope(next)
    for (const row of pc.rows) row.rating = next.ratings[row.relativePath] ?? 0
    const fresh = await pcSync.checkOnOpen(project)
    expect(fresh.kind).toBe('clash')
    await pcSync.resolveClash(project, fresh.clash, 'intersection')
    expect(pc.rows[4].rating).toBe(1)
    expect(pc.rows[2].rating).toBe(1) // 両方で残した
    expect(pc.rows[1].rating).toBe(0) // Android で落とした
  })

  it('ダイアログを出したあとに NAS が変わったら、実行せずに出し直す', async () => {
    const { pc, android, pcSync, androidSync, clash } = await clashed()
    const next = core.advance(android.session.core, ['IMG_4.JPG'])
    android.session = envelope(next)
    for (const row of android.rows) row.rating = next.ratings[row.relativePath] ?? 0
    await androidSync.pushIfChanged(project)
    pc.nas.writes.length = 0
    const result = await pcSync.resolveClash(project, clash, 'mine')
    expect(result.kind).toBe('changed')
    expect(result.outcome.kind).toBe('clash')
    expect(pc.writes).toEqual([])
    expect(nasCatalog(pc).updatedBy).toBe(ANDROID.id)
  })

  it('一時的に書けないときは、読むだけの共有と決めつけて相手を取り込まない（何も変えずに理由を返す）', async () => {
    const { pc, pcSync, clash } = await clashed()
    const stateBefore = { ...pc.state }
    pc.failWrite = true
    for (const choice of ['mine', 'theirs', 'union']) {
      const result = await pcSync.resolveClash(project, clash, choice)
      expect(result).toMatchObject({ kind: 'failed', reason: '書けません', access: 'readwrite' })
    }
    expect(ratingsOf(pc)).toEqual([0, 1, 1, 0, 0, 0])
    expect(pc.state).toEqual(stateBefore)
    // 直れば選び直せる。
    pc.failWrite = false
    expect((await pcSync.resolveClash(project, clash, 'mine')).kind).toBe('done')
  })

  it('書けない共有だと分かったら、その理由と access を返す（取り込みはしない）', async () => {
    const { pc, pcSync, clash } = await clashed()
    pc.access = 'readonly'
    const result = await pcSync.resolveClash(project, clash, 'mine')
    expect(result).toMatchObject({ kind: 'failed', access: 'readonly' })
    expect(ratingsOf(pc)).toEqual([0, 1, 1, 0, 0, 0])
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

  it('readonly（開発用）: 端末が未着手なら読めた分を取り込む', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar()))
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome.kind).toBe('pulled')
    expect(outcome.access).toBe('readonly')
    expect(ratingsOf(backend)).toEqual([0, 1, 1, 0, 0, 0])
    expect(backend.writes).toEqual([])
  })

  // 仕様の変更（U34・設計書 §4.3 の #8）: 以前は「端末に変更があっても、読めれば取り込む」だった。
  // 端末の選別状況を確認なしに捨てないため、書けない共有では端末の分をそのまま残す（この端末だけの結果）。
  it('readonly: 端末が進んでいれば取り込まず、ダイアログも出さない（この端末だけの結果）', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    play(backend, [['IMG_0.JPG']])
    backend.state = { seenAt: 100, seenBy: 'old', localChanged: true }
    backend.files.set('catalog.json', core.sidecarToJson(remoteSidecar()))
    const sync = createSidecarSync(backend, nextClock)
    const outcome = await sync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'settled', reason: 'ReadOnly', access: 'readonly' })
    expect(ratingsOf(backend)).toEqual([1, 0, 0, 0, 0, 0])
    expect(backend.writes).toEqual([])
  })

  it('readonly: サイドカーが無ければ何もしない', async () => {
    const backend = fakeBackend({ access: 'readonly' })
    play(backend, [['IMG_0.JPG']])
    backend.state = { seenAt: 0, seenBy: '', localChanged: true }
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('settled')
    expect(backend.writes).toEqual([])
  })
})

describe('やり直し（epoch）', () => {
  it('PC がやり直したら、Android（まだ共有していない判断がある）は黙って空にせず確認する', async () => {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    play(android, [['IMG_0.JPG']])
    await androidSync.pushIfChanged(project)
    await pcSync.checkOnOpen(project) // PC は取り込む
    // Android はその後も進めた（まだ書いていない）。
    play(android, [['IMG_0.JPG'], ['IMG_3.JPG']])
    // PC で「最初からやり直す」（星・Session・手直し・距離を消し、世代を変える）。
    for (const row of pc.rows) row.rating = 0
    pc.session = null
    await pcSync.markRestarted('p1')
    expect(await pcSync.pushIfChanged(project)).toBe(true)
    // 着手済みの版（Android の A1）を未着手で置き換える前に退避している。
    expect(latestAside(pc, ANDROID.id).updatedBy).toBe(ANDROID.id)
    expect(nasCatalog(pc).epoch).toMatch(/^e-/)
    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(outcome.clash.reason).toBe('TheirsRestarted')
    expect(ratingsOf(android)).toEqual([1, 0, 0, 1, 0, 0])
  })

  it('U42: Android が書いたあと何も変えていなくても、PC がやり直した版は確認なしに取り込まない', async () => {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    play(android, [['IMG_0.JPG']])
    await androidSync.pushIfChanged(project)
    await pcSync.checkOnOpen(project) // PC は取り込む
    // Android はその後も進め、書いた（共有していない判断は無い）。
    play(android, [['IMG_0.JPG'], ['IMG_3.JPG']])
    expect(await androidSync.pushIfChanged(project)).toBe(true)
    // PC で「最初からやり直す」。PC は Android の版を見ないまま、その上に書く（早送りの関係）。
    await pcSync.checkOnOpen(project)
    for (const row of pc.rows) row.rating = 0
    pc.session = null
    await pcSync.markRestarted('p1')
    expect(await pcSync.pushIfChanged(project)).toBe(true)
    expect(nasCatalog(pc).epoch).toMatch(/^e-/)
    // Android は見た版のまま。以前は早送りになり、確認なしに空になっていた。
    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(outcome.clash.reason).toBe('TheirsRestarted')
    expect(ratingsOf(android)).toEqual([1, 0, 0, 1, 0, 0])
  })
})

describe('プロジェクトの設定の同期（U48: settings.pairRawJpeg）', () => {
  function twoDevices() {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    return { nas, pc, android, pcSync: createSidecarSync(pc, nextClock), androidSync: createSidecarSync(android, nextClock) }
  }

  /** Android が選別を進めて書き、PC がそれを取り込んだ（両方が同じ版を見ている）。 */
  async function synced() {
    const devices = twoDevices()
    play(devices.android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await devices.androidSync.pushIfChanged(project)
    expect((await devices.pcSync.checkOnOpen(project)).kind).toBe('pulled')
    return devices
  }

  const pairOf = backend => nasCatalog(backend).settings?.pairRawJpeg

  it('書くとき、端末の値と切り替えた時刻を settings.pairRawJpeg に入れる（一度も切り替えていなければ at は 0）', async () => {
    const backend = fakeBackend()
    play(backend, [['IMG_0.JPG']])
    const sync = createSidecarSync(backend, nextClock)
    expect((await sync.checkOnOpen(project)).kind).toBe('pushed')
    expect(pairOf(backend)).toEqual({ value: true, at: 0 })
  })

  it('PC が切り替える → 区切りで設定だけを書く → もう一台は確認なしに取り込み、再読み込みを促す', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    const writesBefore = pc.writes.length
    await pc.saveProjectPairRaw('p1', false, 5_000_000)
    const pushed = await pcSync.pushAuto(project)
    expect(pushed).toMatchObject({ kind: 'settled', settingsPushed: true })
    expect(pc.writes.length).toBe(writesBefore + 1)
    expect(pairOf(pc)).toEqual({ value: false, at: 5_000_000 })

    const androidBefore = JSON.stringify(android.session)
    const outcome = await androidSync.checkOnOpen(project)
    // 選別状況は同じ。確認も取り込みも書き込みも無い（設定だけが変わった版）。
    expect(outcome).toMatchObject({ kind: 'settled', reason: 'Same', settingsAdopted: false })
    expect(android.pair).toEqual({ value: false, at: 5_000_000 })
    expect(JSON.stringify(android.session)).toBe(androidBefore)
    expect(pc.writes.length).toBe(writesBefore + 1)
    // 「端末が変わった」にもならない（次の自動の書き込みで書かない）。
    expect((await androidSync.pushAuto(project)).kind).toBe('settled')
    expect((await pcSync.pushAuto(project)).kind).toBe('settled')
    expect(pc.writes.length).toBe(writesBefore + 1)
  })

  it('設定だけを書き直した版の上に、もう一台の進んだ選別を確認なしに書く（設定は新しい方のまま）', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG'], ['IMG_4.JPG']]) // Android はまだ書いていない判断がある
    await pc.saveProjectPairRaw('p1', false, 5_000_000)
    await pcSync.pushAuto(project)

    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'pushed', reason: 'LocalChanged', settingsAdopted: false })
    expect(nasCatalog(android).updatedBy).toBe(ANDROID.id)
    expect(pairOf(android)).toEqual({ value: false, at: 5_000_000 })
    // PC は Android の進んだ版を早送りで取り込む（確認は出ない）。
    expect((await pcSync.checkOnOpen(project)).kind).toBe('pulled')
    expect(ratingsOf(pc)).toEqual(ratingsOf(android))
  })

  it('新しく切り替えた方が勝つ（NAS の方が新しければ取り込み、端末の方が新しければ書く）', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    await pc.saveProjectPairRaw('p1', false, 5_000_000)
    await pcSync.pushAuto(project)
    // Android はそのあとで（オフの記録を見る前に）オンに切り替えていた。
    await android.saveProjectPairRaw('p1', true, 6_000_000)
    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome).toMatchObject({ kind: 'settled', settingsPushed: true })
    expect(outcome.settingsAdopted).toBeUndefined()
    expect(android.pair).toEqual({ value: true, at: 6_000_000 })
    expect(pairOf(android)).toEqual({ value: true, at: 6_000_000 })
    // PC は古い方なので取り込む。
    expect(await pcSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', settingsAdopted: true })
    expect(pc.pair).toEqual({ value: true, at: 6_000_000 })

    // 逆: 端末で先に（古い時刻で）切り替えていても、NAS の新しい方に合わせる。
    await pc.saveProjectPairRaw('p1', false, 7_000_000)
    await pcSync.pushAuto(project)
    await android.saveProjectPairRaw('p1', true, 6_500_000)
    expect(await androidSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', settingsAdopted: false })
    expect(android.pair).toEqual({ value: false, at: 7_000_000 })
  })

  it('値が同じなら時刻が違っても何もしない（取り込まない・書かない）', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    await pc.saveProjectPairRaw('p1', true, 5_000_000) // オンのまま（切り替えて戻した）
    await pcSync.pushAuto(project)
    const writes = pc.writes.length
    const outcome = await androidSync.checkOnOpen(project)
    expect(outcome.kind).toBe('settled')
    expect(outcome.settingsAdopted).toBeUndefined()
    expect(outcome.settingsPushed).toBeUndefined()
    expect(android.pairSaves).toEqual([])
    expect(android.pair).toEqual({ value: true, at: 0 })
    expect(pc.writes.length).toBe(writes)
  })

  it('settings の無い古い版: 端末が一度も切り替えていなければ、設定のためだけには書かない', async () => {
    const backend = fakeBackend()
    const remote = remoteSidecar()
    backend.files.set('catalog.json', core.sidecarToJson(remote))
    backend.state = seeFor(remote)
    const outcome = await createSidecarSync(backend, nextClock).checkOnOpen(project)
    expect(outcome.kind).toBe('settled') // 見た版のままで端末も変わっていない（今までどおり）
    expect(outcome.settingsAdopted).toBeUndefined()
    expect(backend.writes).toEqual([])
    expect(backend.pair).toEqual({ value: true, at: 0 })
  })

  it('settings の無い古い版: 端末で切り替えていれば設定を足して書く。選別状況は変えない', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    // 古いアプリが書いた版（settings なし）に置き換わった（中身は同じ選別状況）。
    const old = nasCatalog(pc)
    delete old.settings
    pc.files.set('catalog.json', core.sidecarToJson({ ...old, writeId: 'old-app' }))
    const keyBefore = core.judgementKey(core.sidecarJudgement(nasCatalog(pc)))
    await pc.saveProjectPairRaw('p1', false, 5_000_000)
    expect(await pcSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', settingsPushed: true })
    expect(pairOf(pc)).toEqual({ value: false, at: 5_000_000 })
    expect(core.judgementKey(core.sidecarJudgement(nasCatalog(pc)))).toBe(keyBefore)
    expect(await androidSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', settingsAdopted: false })
    expect(android.pair.value).toBe(false)
  })

  it('NAS が書けない共有・切り離し中は、設定のためだけに書かない', async () => {
    const { pc, android, pcSync, androidSync } = await synced()
    await pc.saveProjectPairRaw('p1', false, 5_000_000)
    await pcSync.pushAuto(project)
    android.access = 'readonly'
    await android.saveProjectPairRaw('p1', true, 6_000_000)
    const writes = pc.writes.length
    expect((await androidSync.checkOnOpen(project)).kind).toBe('settled')
    expect(pc.writes.length).toBe(writes)
    // 切り離し中（「この端末の状況を残す」のあと。端末の選別状況は NAS と違う）。
    android.access = 'readwrite'
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG'], ['IMG_4.JPG']])
    android.state = { ...android.state, detached: true }
    expect(await androidSync.checkOnOpen(project)).toMatchObject({ kind: 'settled', reason: 'Detached' })
    expect(pc.writes.length).toBe(writes)
  })

  it('食い違いの確認には設定の違いを含めない（確認は選別状況だけ。設定は確認なしに新しい方）', async () => {
    const { pc, android, pcSync, androidSync } = twoDevices()
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await android.saveProjectPairRaw('p1', false, 5_000_000)
    await androidSync.pushIfChanged(project)
    play(pc, [['IMG_1.JPG']])
    const outcome = await pcSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    expect(outcome.settingsAdopted).toBe(false)
    expect(pc.pair).toEqual({ value: false, at: 5_000_000 })
    // C（端末の分を書く）でも、取り込んだ設定を書く（古い値で上書きしない）。
    expect((await pcSync.resolveClash(project, outcome.clash, 'mine')).kind).toBe('done')
    expect(pairOf(pc)).toEqual({ value: false, at: 5_000_000 })
  })
})

describe('U52 D4: 退避に成功してから変える（PC・Web。Android の U44 と同じ水準）', () => {
  async function clashedPair() {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    play(android, [['IMG_0.JPG'], ['IMG_2.JPG']])
    await androidSync.pushIfChanged(project)
    play(pc, [['IMG_1.JPG'], ['IMG_2.JPG']])
    const outcome = await pcSync.checkOnOpen(project)
    expect(outcome.kind).toBe('clash')
    nas.writes.length = 0
    return { nas, pc, android, pcSync, androidSync, clash: outcome.clash }
  }

  it('書けない共有で A（取り込む）を選んでも、端末の中に控えてから取り込む', async () => {
    const { pc, pcSync, clash } = await clashedPair()
    pc.access = 'readonly'
    expect((await pcSync.resolveClash(project, clash, 'theirs')).kind).toBe('done')
    expect(pc.writes).toEqual([]) // NAS には書かない
    expect(pc.localAsides).toHaveLength(1)
    const kept = core.sidecarFromJson(pc.localAsides[0])
    expect(kept.sessions.tournament.ratings['IMG_1.JPG']).toBe(1) // 取り込む前の PC の分
    expect(ratingsOf(pc)).toEqual([1, 0, 1, 0, 0, 0])
  })

  it('A: 端末の中に控えられなければ取り込まない（何も変えずに理由を返す）', async () => {
    const { pc, pcSync, clash } = await clashedPair()
    pc.failLocalAside = true
    const stateBefore = { ...pc.state }
    const result = await pcSync.resolveClash(project, clash, 'theirs')
    expect(result).toMatchObject({ kind: 'failed', reason: '端末に控えられません' })
    expect(ratingsOf(pc)).toEqual([0, 1, 1, 0, 0, 0])
    expect(pc.state).toEqual(stateBefore)
    expect(pc.writes).toEqual([])
  })

  it('A: 端末の中と NAS の両方に控える（NAS は時刻つきの名前）', async () => {
    const { pc, pcSync, clash } = await clashedPair()
    expect((await pcSync.resolveClash(project, clash, 'theirs')).kind).toBe('done')
    expect(pc.localAsides).toHaveLength(1)
    expect(pc.writes).toEqual([anAside(PC.id)])
  })

  it('自動の早送りの取り込みでも、取り込む前の端末の分を端末の中に控える', async () => {
    const nas = sharedNas()
    const pc = fakeBackend({ nas, identity: PC })
    const android = fakeBackend({ nas, identity: ANDROID })
    const pcSync = createSidecarSync(pc, nextClock)
    const androidSync = createSidecarSync(android, nextClock)
    play(android, [['IMG_0.JPG']])
    await androidSync.pushIfChanged(project)
    await pcSync.checkOnOpen(project)
    const continued = core.advance(pc.session.core, ['IMG_2.JPG'])
    pc.session = envelope(continued)
    for (const row of pc.rows) row.rating = continued.ratings[row.relativePath] ?? 0
    expect(await pcSync.pushIfChanged(project)).toBe(true)
    expect(await androidSync.checkOnOpen(project)).toMatchObject({ kind: 'pulled', reason: 'FastForward' })
    expect(android.localAsides).toHaveLength(1)
    expect(core.sidecarFromJson(android.localAsides[0]).sessions.tournament.history).toHaveLength(1)
  })

  it('C: 確かめる前に NAS が変わって書かなかったときは、置き換えなかった版の退避も書かない', async () => {
    const { nas, pc, pcSync, clash } = await clashedPair()
    // 置き換える直前に、別の端末が（意味の同じ）版を書き直した。
    nas.beforeCheck = async () => { nas.files.set('catalog.json', `${nas.files.get('catalog.json')}\n`) }
    const result = await pcSync.resolveClash(project, clash, 'mine')
    expect(result.kind).toBe('changed')
    expect(pc.writes.filter(name => name !== 'catalog.json')).toEqual([])
  })

  it('NAS の退避は上書きせず、同じ端末の時刻つきの退避は新しい 5 つだけ残す', async () => {
    const { nas, pc, pcSync, clash } = await clashedPair()
    // 時刻の無い古い退避（U44 より前の名前）は消さない。
    nas.files.set('catalog.zzzzzzzz-111.json', 'old')
    for (let index = 0; index < 7; index++) {
      // 「NAS に書き込む」で Android の版を置き換え（退避する）、また Android の版に戻す。
      expect(await pcSync.writeToNas(project)).toBe(true)
      nas.files.set('catalog.json', clash.theirsText)
    }
    const asides = [...nas.files.keys()].filter(name => asideRe(ANDROID.id).test(name))
    expect(asides).toHaveLength(5)
    expect(nas.files.get('catalog.zzzzzzzz-111.json')).toBe('old')
  })
})

describe('U52 D13: 一時的に欠けた写真の星を、セッションの無い端末が NAS から消さない', () => {
  it('セッションの無い PC で写真が欠損になっても、星は NAS に残り、書き直しもしない', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    backend.rows[1].rating = 2
    backend.rows[3].rating = 1
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(nasCatalog(backend).photos['IMG_3.JPG']).toEqual({ rating: 1 })
    const writes = backend.writes.length
    // サブフォルダが一時的に見えず、IMG_3 が欠損の印になった（行の星は残っている）。
    backend.rows[3].isMissing = true
    expect(await sync.pushIfChanged(project)).toBe(false)
    expect(backend.writes.length).toBe(writes)
    expect(nasCatalog(backend).photos['IMG_3.JPG']).toEqual({ rating: 1 })
    // 別の判断で書くときも、欠損の写真の星を載せる。
    backend.rows[0].rating = 3
    expect(await sync.pushIfChanged(project)).toBe(true)
    expect(nasCatalog(backend).photos['IMG_3.JPG']).toEqual({ rating: 1 })
    expect(nasCatalog(backend).photos['IMG_0.JPG']).toEqual({ rating: 3 })
  })

  it('全部の行が欠損なら、星があっても書かない（空に近い記録で共有を上書きしない）', async () => {
    const backend = fakeBackend()
    const sync = createSidecarSync(backend, nextClock)
    backend.rows[1].rating = 2
    for (const row of backend.rows) row.isMissing = true
    expect((await sync.pushAuto(project)).kind).not.toBe('pushed')
    expect(backend.writes).toEqual([])
  })
})

describe('U52 D15: 写真の鍵の Unicode・大文字小文字・ドライブの割り当て', () => {
  const NFD = 'か\u3099めら.JPG' // 「がめら」の濁点を分けた形（Mac で作った名前など）
  const NFC = NFD.normalize('NFC')

  /** 古い Android（U35 より前）の形: 共有の根からの相対・keyBase なし。 */
  function legacyAndroid(head) {
    const session = core.advance(
      core.startRound(refs(6, refs(6).map(ref => `${head}/${ref.relative_path}`)), 2, 0, false, threshold, []),
      [`${head}/IMG_0.JPG`]
    )
    return JSON.stringify({
      version: 1, updatedAt: 10, updatedBy: ANDROID.id, updatedByName: 'Pixel',
      photos: {}, burstOverrides: [], sessions: { tournament: session }
    })
  }

  it('端末の写真の名前が NFD でも、取り込んだ星とセッションの鍵は端末の行の名前に合う', async () => {
    const nas = sharedNas()
    const names = [NFD, 'IMG_1.JPG', 'IMG_2.JPG', 'IMG_3.JPG', 'IMG_4.JPG', 'IMG_5.JPG']
    const android = fakeBackend({ nas, identity: ANDROID, names: names.map(name => name.normalize('NFC')) })
    const pc = fakeBackend({ nas, names })
    play(android, [[NFC], ['IMG_2.JPG']])
    await createSidecarSync(android, nextClock).pushIfChanged(project)
    // NAS の鍵は NFC。
    expect(Object.keys(nasCatalog(pc).sessions.tournament.ratings)).toContain(NFC)
    const pcSync = createSidecarSync(pc, nextClock)
    expect((await pcSync.checkOnOpen(project)).kind).toBe('pulled')
    expect(ratingsOf(pc)).toEqual([1, 0, 1, 0, 0, 0])
    expect(Object.keys(pc.session.core.ratings)).toContain(NFD)
    expect(Object.keys(pc.session.core.ratings)).not.toContain(NFC)
    // 取り込んだあとは落ち着いている（鍵の食い違いで「変更あり」にならない）。
    expect((await pcSync.checkOnOpen(project)).kind).toBe('settled')
  })

  it('古い Android の記録の頭は、選んだフォルダのパスと大文字小文字が違っても外せる（Windows のパス）', async () => {
    const pc = fakeBackend({ folderPath: 'l:\\photos\\2021' })
    pc.files.set('catalog.json', legacyAndroid('Photos/2021'))
    const outcome = await createSidecarSync(pc, nextClock).checkOnOpen(project)
    expect(outcome.kind).toBe('pulled')
    expect(ratingsOf(pc)[0]).toBe(1)
  })

  for (const folderPath of ['L:\\2021', '\\\\NAS\\Share\\2021']) {
    it(`ドライブの割り当て（${folderPath}）に関係なく、古い Android の記録の頭を外せる`, async () => {
      const pc = fakeBackend({ folderPath })
      pc.files.set('catalog.json', legacyAndroid('2021'))
      expect((await createSidecarSync(pc, nextClock).checkOnOpen(project)).kind).toBe('pulled')
      expect(ratingsOf(pc)[0]).toBe(1)
    })
  }

  it('PC が書く鍵は選んだフォルダからの相対なので、ドライブの割り当てで変わらない', async () => {
    const written = []
    for (const folderPath of ['L:\\2021', '\\\\NAS\\Share\\2021']) {
      const pc = fakeBackend({ folderPath })
      play(pc, [['IMG_0.JPG']])
      await createSidecarSync(pc, nextClock).pushIfChanged(project)
      written.push(Object.keys(nasCatalog(pc).sessions.tournament.ratings).sort())
    }
    expect(written[0]).toEqual(written[1])
  })
})
