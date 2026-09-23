import { describe, expect, it } from 'vitest'
import { DESKTOP_GROUP_SIZE, TOUCH_GROUP_SIZE, clampGroupSize, groupSizeLimits } from '~/utils/groupSize'

describe('groupSizeLimits', () => {
  it('largeGroups が無い環境は既定 4 枚・上限 4 枚（05 章の表）', () => {
    const limits = groupSizeLimits(false)  // largeGroups なし
    expect(limits.default).toBe(4)
    expect(limits.max).toBe(4)
  })

  it('largeGroups がある環境は 10 枚', () => {
    const limits = groupSizeLimits(true)   // largeGroups あり
    expect(limits.default).toBe(10)
    expect(limits.max).toBe(10)
  })

  it('どちらも下限は 2 枚（1 枚では比較にならない）', () => {
    expect(groupSizeLimits(true).min).toBe(2)
    expect(groupSizeLimits(false).min).toBe(2)
  })

  it('4 枚は 2×2 に収まる枚数', () => {
    // columnsFor が 4→2 列にする前提。
    expect(Math.sqrt(TOUCH_GROUP_SIZE.max)).toBe(2)
    expect(Math.sqrt(TOUCH_GROUP_SIZE.default)).toBe(2)
  })
})

describe('clampGroupSize', () => {
  it('上限を超える枚数は上限に収める', () => {
    // デスクトップで 10 枚だったセッションを Web ピッカーで再開する場合。
    expect(clampGroupSize(10, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(100, TOUCH_GROUP_SIZE)).toBe(4)
  })

  it('下限を下回る枚数は下限に収める', () => {
    expect(clampGroupSize(1, TOUCH_GROUP_SIZE)).toBe(2)
    expect(clampGroupSize(0, DESKTOP_GROUP_SIZE)).toBe(2)
  })

  it('範囲内はそのまま', () => {
    expect(clampGroupSize(4, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(2, TOUCH_GROUP_SIZE)).toBe(2)
    expect(clampGroupSize(10, DESKTOP_GROUP_SIZE)).toBe(10)
  })

  it('数値でない値は既定に落とす', () => {
    expect(clampGroupSize(Number.NaN, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(Number.POSITIVE_INFINITY, TOUCH_GROUP_SIZE)).toBe(4)
  })

  it('小数は丸める', () => {
    expect(clampGroupSize(3.4, TOUCH_GROUP_SIZE)).toBe(3)
    expect(clampGroupSize(3.6, TOUCH_GROUP_SIZE)).toBe(4)
  })
})
