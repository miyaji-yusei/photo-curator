import { describe, expect, it } from 'vitest'
import { DESKTOP_GROUP_SIZE, TOUCH_GROUP_SIZE, clampGroupSize, groupSizeLimits } from '~/utils/groupSize'

describe('groupSizeLimits', () => {
  it('iPad などブラウザでは既定 4 枚・上限 9 枚', () => {
    const limits = groupSizeLimits(false)
    expect(limits.default).toBe(4)
    expect(limits.max).toBe(9)
  })

  it('デスクトップアプリは今までどおり 10 枚', () => {
    const limits = groupSizeLimits(true)
    expect(limits.default).toBe(10)
    expect(limits.max).toBe(10)
  })

  it('どちらも下限は 2 枚（1 枚では比較にならない）', () => {
    expect(groupSizeLimits(true).min).toBe(2)
    expect(groupSizeLimits(false).min).toBe(2)
  })

  it('9 枚は 3×3、4 枚は 2×2 に収まる枚数', () => {
    // columnsFor が 4→2 列、9→3 列にする前提。
    expect(Math.sqrt(TOUCH_GROUP_SIZE.max)).toBe(3)
    expect(Math.sqrt(TOUCH_GROUP_SIZE.default)).toBe(2)
  })
})

describe('clampGroupSize', () => {
  it('上限を超える枚数は上限に収める', () => {
    // デスクトップで 10 枚だったセッションを iPad で再開する場合。
    expect(clampGroupSize(10, TOUCH_GROUP_SIZE)).toBe(9)
    expect(clampGroupSize(100, TOUCH_GROUP_SIZE)).toBe(9)
  })

  it('下限を下回る枚数は下限に収める', () => {
    expect(clampGroupSize(1, TOUCH_GROUP_SIZE)).toBe(2)
    expect(clampGroupSize(0, DESKTOP_GROUP_SIZE)).toBe(2)
  })

  it('範囲内はそのまま', () => {
    expect(clampGroupSize(4, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(9, TOUCH_GROUP_SIZE)).toBe(9)
    expect(clampGroupSize(10, DESKTOP_GROUP_SIZE)).toBe(10)
  })

  it('数値でない値は既定に落とす', () => {
    expect(clampGroupSize(Number.NaN, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(Number.POSITIVE_INFINITY, TOUCH_GROUP_SIZE)).toBe(4)
  })

  it('小数は丸める', () => {
    expect(clampGroupSize(4.4, TOUCH_GROUP_SIZE)).toBe(4)
    expect(clampGroupSize(4.6, TOUCH_GROUP_SIZE)).toBe(5)
  })
})
