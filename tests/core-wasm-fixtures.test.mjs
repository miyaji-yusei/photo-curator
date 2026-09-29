// core（Rust）と core-wasm（wasm-bindgen）が同じ答えを返すことを確かめる。
//
// core/tests/fixtures/ の同じ入力・同じ期待値を、cargo test 側
// （core/tests/fixtures_match.rs）とここの両方から読む。**片方だけ直る
// 不具合を防ぐため**（設計 07 章 段1 1-4。かつて連写のまとめ方が
// Rust と TypeScript に二重実装されていて、実際にこれが起きた）。
//
// node:fs を使うため、あえて .ts ではなく .mjs にしている
// （このプロジェクトは @types/node を足さない方針。vitest.config.ts 参照）。
// `pnpm core:wasm` を先に実行し、core-wasm/pkg が作られていること。

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import {
  advance,
  dHashFromGray,
  groupBursts,
  initWithBytes,
  learnDistance,
  startRound,
  undo
} from '~/lib/core'

const root = import.meta.dirname
const fixturesDir = join(root, '..', 'core', 'tests', 'fixtures')
const wasmPath = join(root, '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm')

function readFixture(name) {
  return JSON.parse(readFileSync(join(fixturesDir, `${name}.json`), 'utf-8'))
}

beforeAll(() => {
  initWithBytes(readFileSync(wasmPath))
})

describe('core-wasm は core（cargo test）と同じ答えを返す', () => {
  it('group_bursts', () => {
    const fixture = readFixture('group_bursts')
    const actual = groupBursts(fixture.photos, fixture.threshold, fixture.overrides)
    expect(actual).toEqual(fixture.expected)
  })

  it('start_round → advance → undo', () => {
    const fixture = readFixture('round_advance_undo')

    const afterStart = startRound(
      fixture.photos,
      fixture.group_size,
      fixture.target_star,
      fixture.group_bursts_on,
      fixture.threshold,
      fixture.overrides
    )
    expect(afterStart).toEqual(fixture.expected_after_start)

    const afterAdvance = advance(afterStart, fixture.selected)
    expect(afterAdvance).toEqual(fixture.expected_after_advance)

    const afterUndo = undo(afterAdvance)
    expect(afterUndo).toEqual(fixture.expected_after_undo)
  })

  it('learn_distance', () => {
    const fixture = readFixture('learn_distance')
    expect(learnDistance(fixture.answers, fixture.fallback)).toBe(fixture.expected)
  })

  it('d_hash_from_gray', () => {
    const fixture = readFixture('d_hash_from_gray')
    expect(dHashFromGray(fixture.gray, fixture.width, fixture.height)).toBe(fixture.expected)
  })
})
