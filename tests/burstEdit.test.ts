import { describe, expect, it } from 'vitest'
import {
  blockSizeOf, blocksFromCuts, boundaryCount, cutAll, cutAroundSelection,
  cutsFromGroups, joinAt, noCuts
} from '~/utils/burstEdit'

/** 撮影順に並んだ 5 枚。1 と 5 は端なので、境目は 4 つ。 */
const run = ['1', '2', '3', '4', '5']

/** 「どう分かれて見えるか」で確かめる。境目の真偽値そのものは実装の都合。 */
const shapeOf = (cuts: boolean[]) => blocksFromCuts(run, cuts).map(block => block.join(''))

describe('境目とまとまりの往復', () => {
  it('境目が無ければ 1 つのまとまり', () => {
    expect(shapeOf(noCuts(run))).toEqual(['12345'])
  })

  it('境目の数は写真の枚数より 1 つ少ない', () => {
    expect(boundaryCount(run)).toBe(4)
    expect(boundaryCount(['a'])).toBe(0)
    expect(boundaryCount([])).toBe(0)
  })

  it('いまのまとまり方から境目を起こせる', () => {
    // 1,2 と 4,5 がまとまっていて、3 はどちらにも属さない。
    const cuts = cutsFromGroups(run, [['1', '2'], ['4', '5']])
    expect(shapeOf(cuts)).toEqual(['12', '3', '45'])
  })

  it('グループが1つも無ければ全員バラバラ', () => {
    expect(shapeOf(cutsFromGroups(run, []))).toEqual(['1', '2', '3', '4', '5'])
  })

  it('写真が無いときは空', () => {
    expect(blocksFromCuts([], [])).toEqual([])
  })
})

describe('cutAroundSelection', () => {
  const whole = noCuts(run)

  it('連続した後ろ3枚を選ぶと 1,2 と 3,4,5 に分かれる', () => {
    expect(shapeOf(cutAroundSelection(run, whole, ['3', '4', '5']))).toEqual(['12', '345'])
  })

  it('連続した先頭2枚を選ぶと 1,2 と 3,4,5 に分かれる', () => {
    expect(shapeOf(cutAroundSelection(run, whole, ['1', '2']))).toEqual(['12', '345'])
  })

  it('真ん中の1枚を選ぶと、その1枚だけが切り離される', () => {
    expect(shapeOf(cutAroundSelection(run, whole, ['3']))).toEqual(['12', '3', '45'])
  })

  it('選んだ塊の内側は切らない', () => {
    // 3,4,5 を選んで 3 と 4 の間が切れたら、分割ではなく全解除になってしまう。
    const shape = shapeOf(cutAroundSelection(run, whole, ['3', '4', '5']))
    expect(shape).toContain('345')
    expect(shape).not.toContain('3')
  })

  it('飛び飛びに選ぶと、それぞれが独立する', () => {
    expect(shapeOf(cutAroundSelection(run, whole, ['1', '3', '5'])))
      .toEqual(['1', '2', '3', '4', '5'])
  })

  it('全部選んでも何も起きない。全解除は別の操作', () => {
    expect(shapeOf(cutAroundSelection(run, whole, [...run]))).toEqual(['12345'])
  })

  it('1枚も選ばなければ何も起きない', () => {
    expect(shapeOf(cutAroundSelection(run, whole, []))).toEqual(['12345'])
  })

  it('既にある境目は消さない', () => {
    const already = cutAroundSelection(run, whole, ['1'])
    expect(shapeOf(cutAroundSelection(run, already, ['4'])))
      .toEqual(['1', '23', '4', '5'])
  })

  it('run に無い id は無視する', () => {
    expect(shapeOf(cutAroundSelection(run, whole, ['zzz']))).toEqual(['12345'])
  })
})

describe('joinAt', () => {
  it('境目を1つ繋ぐ', () => {
    const cuts = cutAroundSelection(run, noCuts(run), ['3'])
    expect(shapeOf(cuts)).toEqual(['12', '3', '45'])
    expect(shapeOf(joinAt(cuts, 1))).toEqual(['123', '45'])
  })

  it('繋ぐのは指定した境目だけ', () => {
    const cuts = cutAll(noCuts(run))
    expect(shapeOf(joinAt(cuts, 0))).toEqual(['12', '3', '4', '5'])
  })

  it('範囲外は何も起きない', () => {
    const cuts = cutAll(noCuts(run))
    expect(shapeOf(joinAt(cuts, -1))).toEqual(['1', '2', '3', '4', '5'])
    expect(shapeOf(joinAt(cuts, 99))).toEqual(['1', '2', '3', '4', '5'])
  })

  it('元の配列を書き換えない', () => {
    const cuts = cutAll(noCuts(run))
    joinAt(cuts, 0)
    expect(cuts[0]).toBe(true)
  })
})

describe('cutAll', () => {
  it('全部バラバラにする', () => {
    expect(shapeOf(cutAll(noCuts(run)))).toEqual(['1', '2', '3', '4', '5'])
  })
})

describe('blockSizeOf', () => {
  it('その写真が属するまとまりの枚数を返す', () => {
    const blocks = blocksFromCuts(run, cutAroundSelection(run, noCuts(run), ['3']))
    expect(blockSizeOf(blocks, '1')).toBe(2)
    expect(blockSizeOf(blocks, '3')).toBe(1)
  })

  it('居ない写真は 0', () => {
    expect(blockSizeOf(blocksFromCuts(run, noCuts(run)), 'zzz')).toBe(0)
  })
})
