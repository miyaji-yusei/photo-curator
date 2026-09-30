// 選別の封筒の読み書きと、星の行への写し（syncRatings）を確かめる。
// node:fs を使うため .mjs にしてある（`core-wasm-fixtures.test.mjs` と同じ理由。@types/node を足さない）。
// 読み戻しは core（wasm）が行うので、`core-wasm-fixtures.test.mjs` と同じ方法で読み込む。
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import * as core from '~/lib/core'
import {
  healRatings, parseSavedSelection, serializeSavedSelection, syncRatings
} from '~/utils/selectionFlow'

const wasmPath = join(import.meta.dirname, '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm')

beforeAll(() => {
  core.initWithBytes(readFileSync(wasmPath))
})

const threshold = { window_ms: 4000, distance: 9, d_hash_version: 2 }
const refs = count =>
  Array.from({ length: count }, (_unused, index) => ({
    relative_path: `IMG_${index}.JPG`, captured_at: index * 60_000, d_hash: null, d_hash_version: 2
  }))

function envelope() {
  return {
    v: 2,
    core: core.startRound(refs(6), 4, 0, false, threshold, []),
    stage: 'tournament',
    settings: { groupSize: 4, groupBursts: false },
    multiSelect: true,
    selectedInGroup: ['IMG_1.JPG'],
    learning: null,
    burstDistance: 9,
    updatedAt: 123
  }
}

describe('healRatings（開いたときの自己修復）', () => {
  const start = () => core.startRound(refs(6), 4, 0, false, threshold, [])
  const rowsOf = ratings => refs(6).map((ref, index) => ({
    id: `id-${index}`, relativePath: ref.relative_path, rating: ratings[ref.relative_path] ?? 0
  }))

  it('行の星が Session に追いついていなければ、Session に合わせる（行だけ +1 された逆は戻す）', () => {
    const session = core.advance(start(), ['IMG_1.JPG'])
    // 行だけ先に IMG_1 が ★1、さらに IMG_2 が食い違って ★3。Session は IMG_1 = 1・IMG_2 = 0。
    const entries = healRatings(rowsOf({ 'IMG_1.JPG': 2, 'IMG_2.JPG': 3 }), session)
    expect(entries).toEqual([{ id: 'id-1', rating: 1 }, { id: 'id-2', rating: 0 }])
  })

  it('Session が進んでいて行が前のままなら、行を進める', () => {
    const session = core.advance(start(), ['IMG_1.JPG'])
    expect(healRatings(rowsOf({}), session)).toEqual([{ id: 'id-1', rating: 1 }])
  })

  it('揃っていれば何も書かない。Session に無い写真は触らない', () => {
    const session = core.advance(start(), ['IMG_1.JPG'])
    expect(healRatings(rowsOf({ 'IMG_1.JPG': 1 }), session)).toEqual([])
    const rows = [...rowsOf({ 'IMG_1.JPG': 1 }), { id: 'other', relativePath: 'OTHER.JPG', rating: 4 }]
    expect(healRatings(rows, session)).toEqual([])
  })
})

describe('syncRatings', () => {
  it('確定すると、選んだ写真の星だけが差として出る', () => {
    const before = core.startRound(refs(6), 4, 0, false, threshold, [])
    const after = core.advance(before, ['IMG_1.JPG', 'IMG_2.JPG'])
    expect(syncRatings(before, after)).toEqual([
      { relativePath: 'IMG_1.JPG', rating: 1 },
      { relativePath: 'IMG_2.JPG', rating: 1 }
    ])
  })

  it('1 つ戻すと、戻った星が差として出る', () => {
    const start = core.startRound(refs(6), 4, 0, false, threshold, [])
    const advanced = core.advance(start, ['IMG_0.JPG'])
    const undone = core.undo(advanced)
    expect(syncRatings(advanced, undone)).toEqual([{ relativePath: 'IMG_0.JPG', rating: 0 }])
    expect(syncRatings(start, undone)).toEqual([])
  })

  it('★5 で確定して戻すと、押す前の星に返る', () => {
    const start = core.startRound(refs(6), 4, 0, false, threshold, [])
    const topped = core.keepAndTop(start, [], 'IMG_0.JPG')
    expect(syncRatings(start, topped)).toEqual([{ relativePath: 'IMG_0.JPG', rating: 5 }])
    expect(syncRatings(topped, core.undo(topped))).toEqual([{ relativePath: 'IMG_0.JPG', rating: 0 }])
  })

  it('ratings に無い写真（このラウンドの外）は出さない', () => {
    const start = core.startRound(refs(3), 4, 0, false, threshold, [])
    const outside = { ...start, ratings: { ...start.ratings } }
    delete outside.ratings['IMG_2.JPG']
    const next = { ...start, ratings: { ...start.ratings, 'IMG_2.JPG': 3, 'NEW.JPG': 2 } }
    // prev に無い鍵は比べる相手が無いので書かない。
    expect(syncRatings(outside, next)).toEqual([])
  })

  it('前が無ければ何も書かない', () => {
    expect(syncRatings(null, core.startRound(refs(3), 4, 0, false, threshold, []))).toEqual([])
  })
})

describe('封筒（v: 2）の読み書き', () => {
  it('書いて読むと同じになる', () => {
    const written = envelope()
    const read = parseSavedSelection(serializeSavedSelection(written))
    expect(read).toEqual(written)
  })

  it('旧版の形（v が無い）は null', () => {
    const old = JSON.stringify({
      projectId: 'p', settings: { groupSize: 4, groupBursts: false }, candidates: [], groups: [],
      groupIndex: 0, selectedInGroup: [], multiSelect: false, ratings: {}, survivors: [], history: [],
      targetRating: 0, stage: 'tournament', updatedAt: 1
    })
    expect(parseSavedSelection(old)).toBeNull()
  })

  it('壊れた JSON・null・core が読めない形は null', () => {
    expect(parseSavedSelection('{')).toBeNull()
    expect(parseSavedSelection('null')).toBeNull()
    expect(parseSavedSelection(JSON.stringify({ ...envelope(), v: 1 }))).toBeNull()
    expect(parseSavedSelection(JSON.stringify({ ...envelope(), core: { group_size: 'x' } }))).toBeNull()
  })

  it('いまの組に無い写真の選択は捨てる', () => {
    const written = { ...envelope(), selectedInGroup: ['IMG_1.JPG', 'GONE.JPG'] }
    expect(parseSavedSelection(serializeSavedSelection(written))?.selectedInGroup).toEqual(['IMG_1.JPG'])
  })
})
