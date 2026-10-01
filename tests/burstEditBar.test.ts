import { describe, expect, it } from 'vitest'
import { moveCut, toggleAt } from '~/utils/burstEdit'

describe('toggleAt', () => {
  it('切る／つなぐを入れ替える', () => {
    expect(toggleAt([false, false, false], 1)).toEqual([false, true, false])
    expect(toggleAt([false, true, false], 1)).toEqual([false, false, false])
  })
  it('範囲外は何もしない', () => {
    expect(toggleAt([false], 5)).toEqual([false])
    expect(toggleAt([false], -1)).toEqual([false])
  })
})

describe('moveCut', () => {
  it('切れている境目を隣へずらす', () => {
    expect(moveCut([false, true, false, false], 1, 2)).toEqual([false, false, true, false])
    expect(moveCut([false, true, false, false], 1, 0)).toEqual([true, false, false, false])
  })
  it('つながっている境目は動かさない', () => {
    expect(moveCut([false, true, false], 0, 2)).toEqual([false, true, false])
  })
  it('範囲外は端に収める', () => {
    expect(moveCut([false, true, false], 1, 9)).toEqual([false, false, true])
    expect(moveCut([false, true, false], 1, -4)).toEqual([true, false, false])
  })
  it('切れている境目に重ねると 1 つになる', () => {
    expect(moveCut([true, true, false], 1, 0)).toEqual([true, false, false])
  })
})
