// サイドカー同期の判断（U33）を core-wasm から呼べることを確かめる。
//
// 判断そのものは core（cargo test の sidecar_sync::tests）で確かめている。ここは
// 「wasm 越しに同じ答えが返る」「JS の値（数・null・文字列の enum）がそのまま渡る」ことを見る。
// core/tests/fixtures/sidecar_sync.json は cargo test 側（core/tests/fixtures_match.rs）も読む。
//
// 新しい関数の TS の型（lib/core.ts）は U34（PC・Web の呼び出しの切り替え）で足す。
// それまでは core-wasm/pkg を直接呼ぶ（初期化は lib/core の initWithBytes が同じモジュールに行う）。
// `pnpm core:wasm` を先に実行し、core-wasm/pkg が作られていること。

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import { advance, initWithBytes, roundFor, sidecarFromJson, sidecarToJson, startRound } from '~/lib/core'
import * as wasm from '../core-wasm/pkg/photo_curator_core_wasm.js'

const root = import.meta.dirname
const wasmPath = join(root, '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm')
const fixture = JSON.parse(
  readFileSync(join(root, '..', 'core', 'tests', 'fixtures', 'sidecar_sync.json'), 'utf-8')
)

const NAMES = ['a.jpg', 'b.jpg', 'sub/c.jpg', 'd.jpg', 'e.jpg', 'f.jpg']
const threshold = { window_ms: 4000, distance: 6, d_hash_version: 2 }
const refs = NAMES.map((name, index) => ({
  relative_path: name,
  captured_at: index * 100000,
  d_hash: '0000000000000000',
  d_hash_version: 2
}))

const fresh = () => startRound(refs, 2, 0, false, threshold, [])
const progressed = () => advance(advance(fresh(), ['a.jpg']), ['sub/c.jpg', 'd.jpg'])
const otherProgress = () => advance(fresh(), ['b.jpg'])

function catalog(session, { writeId = null, by = 'pc', at = 1 } = {}) {
  const value = {
    version: 1,
    updatedAt: at,
    updatedBy: by,
    updatedByName: by,
    photos: {},
    burstOverrides: [],
    sessions: session ? { tournament: session } : {},
    keyBase: 'folder'
  }
  if (writeId) value.writeId = writeId
  return value
}

const judge = session => wasm.canonicalJudgement(session, null, null, undefined, undefined)

beforeAll(() => {
  initWithBytes(readFileSync(wasmPath))
})

describe('core-wasm のサイドカー同期（U33）', () => {
  it('フィクスチャ: Android の古い形と PC の形が同じ比較キーになり、未着手の版には書く判断になる', () => {
    const read = (value, folder) => wasm.sidecarNormalizeKeys(sidecarFromJson(JSON.stringify(value)), folder)
    const android = read(fixture.android, fixture.android_folder)
    const pc = read(fixture.pc, fixture.pc_folder)
    const untouched = read(fixture.untouched, fixture.pc_folder)

    const androidKey = wasm.judgementKey(wasm.sidecarJudgement(android))
    expect(androidKey).toBe(fixture.expected_key)
    expect(wasm.judgementKey(wasm.sidecarJudgement(pc))).toBe(fixture.expected_key)

    const seen = { token: fixture.seen_token, key: androidKey, epoch: null }
    const plan = wasm.sidecarPlan(seen, wasm.sidecarJudgement(android), untouched, true, false)
    expect(plan).toEqual(fixture.expected_plan)

    // U42: 端末は見た版のままでも、ほかの端末がやり直した版（早送りの関係）は確認する。
    const restarted = read(fixture.restarted, fixture.pc_folder)
    const again = wasm.sidecarPlan(seen, wasm.sidecarJudgement(android), restarted, true, false)
    expect(again[fixture.expected_restarted.plan]?.reason).toBe(fixture.expected_restarted.reason)
  })

  it('意味が同じなら、並び・空白・updatedAt が違っても何もしない（警告なし）', () => {
    const original = sidecarFromJson(JSON.stringify(catalog(progressed(), { writeId: 'w-1', by: 'android', at: 10 })))
    const seen = wasm.sidecarSeen(original)
    const reordered = JSON.parse(sidecarToJson(original))
    reordered.updatedAt = 77777
    reordered.updatedBy = 'pc'
    reordered.writeId = 'w-2'
    const text = JSON.stringify(Object.fromEntries(Object.entries(reordered).reverse()), null, 4)
    const rewritten = sidecarFromJson(text)

    expect(wasm.judgementEquivalent(wasm.sidecarJudgement(original), wasm.sidecarJudgement(rewritten))).toBe(true)
    const plan = wasm.sidecarPlan(seen, judge(progressed()), rewritten, true, false)
    expect(plan.Settled.reason).toBe('Same')
    expect(plan.Settled.seen.token).toBe('w-2')
  })

  it('端末が進んでいて NAS が未着手の版なら、取り込まずに書く（今回の現象）', () => {
    const a1 = catalog(progressed(), { writeId: 'w-a1', by: 'android', at: 10 })
    const p1 = catalog(fresh(), { writeId: 'w-p1', by: 'pc', at: 20 })
    // 旧い判断（sidecarDecide）は取り込みになる。
    expect(Object.keys(wasm.sidecarDecide(10, 'android', false, p1))).toEqual(['Pull'])
    const plan = wasm.sidecarPlan(wasm.sidecarSeen(a1), judge(progressed()), p1, true, false)
    expect(plan).toEqual({ Push: { expected: 'w-p1', aside_theirs: true, reason: 'TheirsUntouched' } })
  })

  it('端末が未着手なら確認なしに取り込み、見た版がなければ空の控えでよい', () => {
    const theirs = catalog(progressed(), { writeId: 'w-a1', by: 'android' })
    const plan = wasm.sidecarPlan(null, judge(fresh()), theirs, true, false)
    expect(plan.Pull.reason).toBe('LocalUntouched')
    expect(plan.Pull.aside_mine).toBe(false)
    expect(plan.Pull.seen.token).toBe('w-a1')
    expect(plan.Pull.theirs.writeId).toBe('w-a1')
  })

  it('両方着手で違えば、両方の要約と見込みを付けて確認する', () => {
    const a1 = catalog(progressed(), { writeId: 'w-a1', by: 'android' })
    const theirs = catalog(otherProgress(), { by: 'pc', at: 30 })
    const plan = wasm.sidecarPlan(wasm.sidecarSeen(a1), judge(progressed()), theirs, true, false)
    expect(plan.Clash.reason).toBe('Diverged')
    expect(plan.Clash.order).toBe('Ahead')
    expect(plan.Clash.mine_progress).toMatchObject({ started: true, round: 1, decided: 2, starred: 3 })
    expect(plan.Clash.theirs_progress).toMatchObject({ started: true, decided: 1, starred: 1, total: 6 })
    expect(plan.Clash.preview).toMatchObject({ mine_starred: 3, theirs_starred: 1, union_starred: 4 })
  })

  it('早送り（系統に見た版がある）なら確認なしに取り込む', () => {
    const a1 = catalog(progressed(), { writeId: 'w-a1', by: 'android' })
    const b2 = catalog(advance(progressed(), ['e.jpg']), { writeId: 'w-b2', by: 'pc', at: 30 })
    b2.basedOn = 'w-b1'
    b2.lineage = ['w-b1', 'w-a1']
    const plan = wasm.sidecarPlan(wasm.sidecarSeen(a1), judge(progressed()), b2, true, false)
    expect(plan.Pull.reason).toBe('FastForward')
    expect(plan.Pull.aside_mine).toBe(true)
  })

  it('未着手と比較キー', () => {
    expect(wasm.isUntouched(judge(fresh()))).toBe(true)
    expect(wasm.isUntouched(judge(progressed()))).toBe(false)
    const key = wasm.judgementKey(judge(progressed()))
    expect(key).toMatch(/^j1:[0-9a-f]{64}$/)
    expect(wasm.judgementKey(wasm.sidecarJudgement(catalog(progressed())))).toBe(key)
    expect(wasm.normalizeKey('sub\\c.jpg')).toBe('sub/c.jpg')
    expect(wasm.progressCmp(wasm.judgementProgress(judge(progressed())), wasm.judgementProgress(judge(fresh())))).toBe('Ahead')
  })

  it('混ぜ方: 積集合・和集合と、混ぜた星の完了状態から続きを始められる', () => {
    const done = { ...fresh(), queue: [], current: [], finished: true, ratings: { 'a.jpg': 2, 'b.jpg': 1, 'd.jpg': 1 } }
    const midway = {
      ...fresh(),
      queue: [],
      current: ['d.jpg'],
      ratings: { 'a.jpg': 1, 'sub/c.jpg': 1 },
      history: [{ group: ['a.jpg', 'b.jpg'], chosen: ['a.jpg'], topped: null, before: {} }]
    }
    const mine = judge(done)
    const theirs = judge(midway)
    expect(wasm.mergeStars(mine, theirs, 'Union')).toEqual({ 'a.jpg': 2, 'b.jpg': 1, 'sub/c.jpg': 1, 'd.jpg': 1 })
    expect(wasm.mergeStars(mine, theirs, 'Intersection')).toEqual({ 'a.jpg': 1, 'd.jpg': 1 })
    expect(wasm.mergePreview(mine, theirs)).toEqual({
      mine_starred: 3,
      theirs_starred: 2,
      intersection_starred: 2,
      union_starred: 4,
      undecided: 1,
      mid_round: true
    })

    const result = wasm.mergeJudgements(mine, theirs, 'Intersection', 2, 'e-new')
    expect(result.session.finished).toBe(true)
    expect(result.session.history).toEqual([])
    expect(result.ratings).toEqual({ 'a.jpg': 1, 'd.jpg': 1 })
    const again = roundFor(result.session, refs, 1, false, threshold, [])
    expect(again.current).toEqual(['a.jpg', 'd.jpg'])

    const session = wasm.sessionFromRatings({ 'a.jpg': 5, 'b.jpg': 1 }, 1, 0, 2, null)
    expect(session.survivors).toEqual(['b.jpg'])
    expect(
      wasm.mergeOverrides(
        [{ left: 'a.jpg', right: 'b.jpg', decision: 'split' }],
        [{ left: 'a.jpg', right: 'b.jpg', decision: 'join' }]
      )
    ).toEqual([{ left: 'a.jpg', right: 'b.jpg', decision: 'split' }])
  })

  it('写真の鍵: Android 形式とフォルダ形式を行き来し、一致率を数える', () => {
    const android = catalog(null)
    delete android.keyBase
    android.photos = { 'photo/x/a.jpg': { rating: 1 }, 'photo/x/sub/c.jpg': { rating: 2 } }
    const folder = wasm.sidecarKeysToFolder(android, 'photo/x')
    expect(Object.keys(folder.photos).sort()).toEqual(['a.jpg', 'sub/c.jpg'])
    expect(folder.keyBase).toBe('folder')
    const back = wasm.sidecarKeysFromFolder(folder, 'photo/x', '/')
    expect(Object.keys(back.photos).sort()).toEqual(['photo/x/a.jpg', 'photo/x/sub/c.jpg'])
    const pc = wasm.sidecarKeysFromFolder(folder, '', '\\')
    expect(Object.keys(pc.photos).sort()).toEqual(['a.jpg', 'sub\\c.jpg'])
    const guessed = wasm.sidecarNormalizeKeys(android, '\\\\NAS\\share\\photo\\x')
    expect(Object.keys(guessed.photos).sort()).toEqual(['a.jpg', 'sub/c.jpg'])
    expect(wasm.sidecarKeyCoverage(folder, ['a.jpg', 'z.jpg'])).toEqual({ matched: 1, total: 2 })
  })

  it('書く前の刻印（v2 の項目）が JS と catalog.json を行き来しても保たれる', () => {
    const base = catalog(fresh(), { writeId: 'w-b' })
    base.lineage = ['w-a']
    const stamped = wasm.sidecarStamp(catalog(progressed(), { by: 'android', at: 50 }), 'w-new', base)
    expect(stamped).toMatchObject({ version: 2, writeId: 'w-new', basedOn: 'w-b', lineage: ['w-b', 'w-a'], keyBase: 'folder' })
    expect(stamped.progress).toMatchObject({ started: true, round: 1, decided: 2, remaining: 2, starred: 3, total: 6 })
    // JS の数（progress）をそのまま wasm に戻しても読める。
    expect(wasm.sidecarToken(stamped)).toBe('w-new')
    const text = sidecarToJson(stamped)
    const back = sidecarFromJson(text)
    expect(back.progress).toEqual(stamped.progress)
    expect(back.lineage).toEqual(['w-b', 'w-a'])
    // 古い形（新しい項目なし）を書いても新しい項目は出ない。
    const legacy = catalog(fresh())
    delete legacy.keyBase
    expect(sidecarToJson(legacy)).not.toMatch(/writeId|basedOn|lineage|epoch|keyBase|progress/)
  })
})
