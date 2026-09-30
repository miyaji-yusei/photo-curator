import { describe, expect, it } from 'vitest'
import { gridFor } from '~/utils/gridFor'

const GAP = 12

describe('gridFor', () => {
  it('4 枚・1440×900 の枠は 2×2', () => {
    expect(gridFor(4, 1440, 900, GAP)).toEqual({ rows: 2, cols: 2 })
  })

  it('幅が広く低い枠（2560×800）は 1×4', () => {
    expect(gridFor(4, 2560, 800, GAP)).toEqual({ rows: 1, cols: 4 })
  })

  it('縦長の枠（800×1200）は面積の大きいほう（2×2）', () => {
    // 2×2: マス 394×594 → 写真 394×263 ≒ 103.6k。4×1: マス 800×291 → 写真 436×291 ≒ 126.9k。
    expect(gridFor(4, 800, 1200, GAP)).toEqual({ rows: 4, cols: 1 })
  })

  it('5 枚は空きが 1 行未満の並び（2×3）が選ばれうる', () => {
    // 幅広の枠で 1×5 より 2×3 のほうが大きい。
    expect(gridFor(5, 1440, 900, GAP)).toEqual({ rows: 2, cols: 3 })
  })

  it('空きが 1 行以上になる並びは選ばれない', () => {
    // 5 枚で 4×2（空き 3 ≧ 列 2）・3×2（空き 1 <列 2）。4 行×2 列は候補外。
    for (const [w, h] of [[400, 1600], [1440, 900], [900, 1440], [3000, 500]]) {
      const { rows, cols } = gridFor(5, w, h, GAP)
      expect(rows * cols - 5).toBeLessThan(cols)
      expect(rows * cols).toBeGreaterThanOrEqual(5)
    }
    for (let n = 1; n <= 10; n++) {
      const { rows, cols } = gridFor(n, 700, 1300, GAP)
      expect(rows * cols - n).toBeLessThan(cols)
    }
  })

  it('最大との差が 5% 未満なら current を保つ', () => {
    const keep = gridFor(4, 1440, 900, GAP, { rows: 2, cols: 2 })
    expect(keep).toEqual({ rows: 2, cols: 2 })
    // 大きく劣る current は離れる
    expect(gridFor(4, 2560, 800, GAP, { rows: 4, cols: 1 })).toEqual({ rows: 1, cols: 4 })
  })

  it('5% 未満の差なら保ち、10% 以上離れたら切り替える', () => {
    // 4 枚で、2×2 と 1×4 の面積が近い枠を探し、current が 5% 未満差なら保持・以上なら切り替え
    const area = (w: number, h: number, r: number, c: number) => {
      const cw = (w - GAP * (c - 1)) / c
      const ch = (h - GAP * (r - 1)) / r
      return Math.min(cw, ch * 1.5) * Math.min(ch, cw / 1.5)
    }
    let checkedKeep = false
    let checkedSwitch = false
    for (let w = 900; w <= 3000; w += 10) {
      const best = gridFor(4, w, 700, GAP)
      const cur = best.rows === 2 ? { rows: 1, cols: 4 } : { rows: 2, cols: 2 }
      const ratio = area(w, 700, cur.rows, cur.cols) / area(w, 700, best.rows, best.cols)
      const got = gridFor(4, w, 700, GAP, cur)
      if (ratio > 0.95 && ratio < 1) { expect(got).toEqual(cur); checkedKeep = true }
      if (ratio < 0.9) { expect(got).toEqual(best); checkedSwitch = true }
    }
    expect(checkedKeep).toBe(true)
    expect(checkedSwitch).toBe(true)
  })

  it('枚数 1・2・10', () => {
    expect(gridFor(1, 1440, 900, GAP)).toEqual({ rows: 1, cols: 1 })
    expect(gridFor(2, 1440, 900, GAP)).toEqual({ rows: 1, cols: 2 })
    expect(gridFor(2, 600, 1000, GAP)).toEqual({ rows: 2, cols: 1 })
    const ten = gridFor(10, 1440, 900, GAP)
    expect(ten.rows * ten.cols).toBeGreaterThanOrEqual(10)
    expect(ten.rows * ten.cols - 10).toBeLessThan(ten.cols)
    expect(ten).toEqual({ rows: 3, cols: 4 })
  })

  it('枠が 0 以下・枚数が 0 以下でも壊れない', () => {
    expect(gridFor(4, 0, 0, GAP)).toEqual({ rows: 1, cols: 4 })
    expect(gridFor(4, 1440, 0, GAP)).toEqual({ rows: 1, cols: 4 })
    expect(gridFor(0, 1440, 900, GAP)).toEqual({ rows: 1, cols: 1 })
    expect(gridFor(-3, 1440, 900, GAP)).toEqual({ rows: 1, cols: 1 })
  })

  it('枚数が変わったら、成り立たない current は使わない', () => {
    // 2×2 は 7 枚には足りない
    const r = gridFor(7, 1440, 900, GAP, { rows: 2, cols: 2 })
    expect(r.rows * r.cols).toBeGreaterThanOrEqual(7)
  })
})
