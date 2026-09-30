import { describe, expect, it } from 'vitest'
import { clampScale, panBy, wheelFactor, ZOOM_RESET, zoomAt, zoomLabel } from '~/utils/zoomPan'

describe('zoomPan', () => {
  it('倍率は 1〜8 に収まる', () => {
    expect(clampScale(0.3)).toBe(1)
    expect(clampScale(20)).toBe(8)
    expect(clampScale(NaN)).toBe(1)
  })

  it('ホイールの向きで拡大・縮小し、1 目盛りで約 1.1〜1.25 倍', () => {
    expect(wheelFactor(-100)).toBeGreaterThan(1.1)
    expect(wheelFactor(-100)).toBeLessThan(1.25)
    expect(wheelFactor(100)).toBeLessThan(1)
    expect(wheelFactor(-5000)).toBe(wheelFactor(-100))
  })

  it('マウスの下の点が拡大の前後で動かない', () => {
    const w = 800
    const h = 600
    const before = { scale: 2, x: 30, y: -20 }
    const mx = 120
    const my = 80
    const after = zoomAt(before, 1.5, mx, my, w, h)
    // 写真の中の点 p が画面に出る位置 = 中心 + 移動 + 倍率 × p
    const p = { x: (mx - before.x) / before.scale, y: (my - before.y) / before.scale }
    expect(after.x + after.scale * p.x).toBeCloseTo(mx)
    expect(after.y + after.scale * p.y).toBeCloseTo(my)
  })

  it('8 倍で止まり、1 倍まで縮めると位置も戻る', () => {
    expect(zoomAt({ scale: 7.9, x: 0, y: 0 }, 2, 0, 0, 800, 600).scale).toBe(8)
    expect(zoomAt({ scale: 1.05, x: 10, y: 10 }, 0.5, 5, 5, 800, 600)).toEqual(ZOOM_RESET)
  })

  it('移動は写真が枠から消えない範囲に収まり、1 倍では動かない', () => {
    const moved = panBy({ scale: 2, x: 0, y: 0 }, 9999, -9999, 800, 600)
    expect(moved).toEqual({ scale: 2, x: 400, y: -300 })
    expect(panBy(ZOOM_RESET, 50, 50, 800, 600)).toEqual(ZOOM_RESET)
  })

  it('倍率の表示は 1 倍では空', () => {
    expect(zoomLabel(1)).toBe('')
    expect(zoomLabel(2.54)).toBe('2.5倍')
  })
})
