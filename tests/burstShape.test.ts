import { describe, expect, it } from 'vitest'
import type { PairOverride, PhotoRef } from '~/lib/core'
import { joinSpanOverrides, overridesFromShape } from '~/utils/burstShape'

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

describe('joinSpanOverrides（この写真をまとめる）', () => {
  const order = ['a', 'b', 'c', 'd', 'e', 'f']
  const join = (left: string, right: string): PairOverride => ({ left, right, decision: 'join' })

  it('選んだ 2 枚の、最初から最後までの隣どうしを全部 join にする（あいだの写真も入る）', () => {
    const result = joinSpanOverrides(order, ['b', 'e'], {}, [])
    expect(result).toEqual([join('b', 'c'), join('c', 'd'), join('d', 'e')])
  })

  it('代表の仲間も含めた撮影順の端から端まで', () => {
    // b の仲間は a と b、e の仲間は e と f。→ a から f まで
    const result = joinSpanOverrides(order, ['b', 'e'], { b: ['a', 'b'], e: ['e', 'f'] }, [])
    expect(result).toHaveLength(5)
    expect(result![0]).toEqual(join('a', 'b'))
    expect(result![4]).toEqual(join('e', 'f'))
  })

  it('選んだ順は関係ない', () => {
    expect(joinSpanOverrides(order, ['e', 'b'], {}, [])).toEqual(joinSpanOverrides(order, ['b', 'e'], {}, []))
  })

  it('同じ 2 枚の既存の手直し（split）は置き換え、ほかは残す', () => {
    const existing: PairOverride[] = [
      { left: 'c', right: 'd', decision: 'split' },
      { left: 'e', right: 'f', decision: 'split' }
    ]
    const result = joinSpanOverrides(order, ['b', 'd'], {}, existing)!
    expect(result).toContainEqual({ left: 'e', right: 'f', decision: 'split' })
    expect(result).not.toContainEqual({ left: 'c', right: 'd', decision: 'split' })
    expect(result).toContainEqual(join('c', 'd'))
    expect(result).toHaveLength(3)
  })

  it('もう一度押しても結果が変わらない（冪等）', () => {
    const once = joinSpanOverrides(order, ['b', 'e'], {}, [])!
    expect(joinSpanOverrides(order, ['b', 'e'], {}, once)).toEqual(once)
  })

  it('対象が 2 枚に満たなければ null', () => {
    expect(joinSpanOverrides(order, ['b'], {}, [])).toBeNull()
    expect(joinSpanOverrides(order, [], {}, [])).toBeNull()
    expect(joinSpanOverrides(order, ['zzz'], {}, [])).toBeNull()
  })
})
