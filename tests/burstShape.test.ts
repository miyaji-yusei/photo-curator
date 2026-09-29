import { describe, expect, it } from 'vitest'
import type { PairOverride, PhotoRef } from '~/lib/core'
import { overridesFromShape } from '~/utils/burstShape'

const ref = (name: string): PhotoRef =>
  ({ relative_path: name, captured_at: 0, d_hash: '0000000000000000', d_hash_version: 2 })

const run = ['a', 'b', 'c', 'd'].map(ref)
/** 基準の判定を、繋ぐ組の一覧で与える。 */
const byThreshold = (joined: string[]) => (left: PhotoRef, right: PhotoRef) =>
  joined.includes(`${left.relative_path}${right.relative_path}`)

const sorted = (list: PairOverride[]) =>
  [...list].sort((x, y) => `${x.left}${x.right}`.localeCompare(`${y.left}${y.right}`))

describe('overridesFromShape', () => {
  it('形が基準どおりなら手直しは作らない', () => {
    const result = overridesFromShape(run, [['a', 'b'], ['c', 'd']], [], byThreshold(['ab', 'cd']))
    expect(result).toEqual([])
  })

  it('同じ塊なのに基準では別なら join', () => {
    const result = overridesFromShape(run, [['a', 'b', 'c', 'd']], [], byThreshold(['ab']))
    expect(sorted(result)).toEqual([
      { left: 'b', right: 'c', decision: 'join' },
      { left: 'c', right: 'd', decision: 'join' }
    ])
  })

  it('別の塊なのに基準では同じなら split', () => {
    const result = overridesFromShape(run, [['a'], ['b', 'c'], ['d']], [], byThreshold(['ab', 'bc', 'cd']))
    expect(sorted(result)).toEqual([
      { left: 'a', right: 'b', decision: 'split' },
      { left: 'c', right: 'd', decision: 'split' }
    ])
  })

  it('基準と一致した組の既存の手直しは消す（元に戻せば溜まらない）', () => {
    const existing: PairOverride[] = [{ left: 'a', right: 'b', decision: 'split' }]
    const result = overridesFromShape(run, [['a', 'b'], ['c'], ['d']], existing, byThreshold(['ab']))
    expect(result).toEqual([])
  })

  it('run の外の組の手直しは触らない', () => {
    const existing: PairOverride[] = [{ left: 'x', right: 'y', decision: 'join' }]
    const result = overridesFromShape(run, [['a', 'b'], ['c'], ['d']], existing, byThreshold(['ab']))
    expect(result).toEqual(existing)
  })

  it('同じ形を何度保存しても同じ（冪等）', () => {
    const blocks = [['a', 'b', 'c'], ['d']]
    const once = overridesFromShape(run, blocks, [], byThreshold(['ab']))
    const twice = overridesFromShape(run, blocks, once, byThreshold(['ab']))
    expect(sorted(twice)).toEqual(sorted(once))
  })

  it('どの塊にも入っていない写真は、隣と別のものとして扱う', () => {
    const result = overridesFromShape(run, [['a', 'b']], [], byThreshold(['ab', 'bc']))
    expect(sorted(result)).toEqual([{ left: 'b', right: 'c', decision: 'split' }])
  })
})
