import { describe, expect, it } from 'vitest'
import {
  TAP_SLOP, decideThreshold, dragFeedback, fitContain, flyTarget, isTap, judgeDrag, slideKeyDecision, tapDecision
} from '~/utils/slideshowGesture'

describe('decideThreshold', () => {
  it('幅の 18%、60〜160px に収める', () => {
    expect(decideThreshold(200)).toBe(60)
    expect(decideThreshold(500)).toBeCloseTo(90)
    expect(decideThreshold(2000)).toBe(160)
    expect(decideThreshold(Number.NaN)).toBe(60)
  })
})

describe('isTap', () => {
  it('動きが小さければクリック', () => {
    expect(isTap(0, 0)).toBe(true)
    expect(isTap(TAP_SLOP - 1, 0)).toBe(true)
    expect(isTap(TAP_SLOP, 0)).toBe(false)
    expect(isTap(6, 6)).toBe(false)
  })
})

describe('judgeDrag（幅 500 → 閾値 90）', () => {
  it('左へ閾値以上で落とす、右で残す', () => {
    expect(judgeDrag(-90, 0, 500)).toBe('drop')
    expect(judgeDrag(120, 10, 500)).toBe('keep')
  })
  it('閾値に足りなければ決めない', () => {
    expect(judgeDrag(-89, 0, 500)).toBeNull()
    expect(judgeDrag(40, 0, 500)).toBeNull()
  })
  it('上へ閾値以上で ★5 確定', () => {
    expect(judgeDrag(0, -90, 500)).toBe('top')
    expect(judgeDrag(30, -150, 500)).toBe('top')
  })
  it('上向きでも量が足りなければ決めない', () => {
    expect(judgeDrag(0, -89, 500)).toBeNull()
  })
  it('下向きは使わない', () => {
    expect(judgeDrag(0, 200, 500)).toBeNull()
    expect(judgeDrag(10, 400, 500)).toBeNull()
  })
  it('斜めは優位な軸で決める（同じなら横）', () => {
    expect(judgeDrag(-100, -200, 500)).toBe('top')
    expect(judgeDrag(-200, -100, 500)).toBe('drop')
    expect(judgeDrag(150, -150, 500)).toBe('keep')
    expect(judgeDrag(150, 200, 500)).toBeNull()
  })
})

describe('dragFeedback', () => {
  it('右は残す・左は落とす、閾値で濃さ 1', () => {
    expect(dragFeedback(45, 0, 500)).toMatchObject({ direction: 'keep', strength: 0.5 })
    expect(dragFeedback(-90, 0, 500)).toMatchObject({ direction: 'drop', strength: 1 })
    expect(dragFeedback(-900, 0, 500).strength).toBe(1)
  })
  it('上が優位なら top、下は何も出さない', () => {
    expect(dragFeedback(5, -45, 500)).toMatchObject({ direction: 'top', strength: 0.5 })
    expect(dragFeedback(0, 100, 500)).toMatchObject({ direction: null, strength: 0 })
  })
  it('回転は横の動きに比例し ±15 度まで', () => {
    expect(dragFeedback(50, 0, 500).rotation).toBeCloseTo(3)
    expect(dragFeedback(-50, 0, 500).rotation).toBeCloseTo(-3)
    expect(dragFeedback(5000, 0, 500).rotation).toBe(15)
    expect(dragFeedback(-5000, 0, 500).rotation).toBe(-15)
  })
  it('決定になる動きでは判定と同じ向きになる', () => {
    for (const [dx, dy] of [[-100, 0], [100, 0], [0, -100], [-100, -200]] as const) {
      expect(dragFeedback(dx, dy, 500).direction).toBe(judgeDrag(dx, dy, 500))
    }
  })
})

describe('tapDecision（枠 left=0, top=0, 幅 1000, 高さ 800 → 上の帯は y<200）', () => {
  const tap = (x: number, y: number) => tapDecision(x, y, 0, 0, 1000, 800)
  it('左半分は落とす、右半分は残す', () => {
    expect(tap(100, 400)).toBe('drop')
    expect(tap(499, 400)).toBe('drop')
    expect(tap(500, 400)).toBe('keep')
    expect(tap(900, 799)).toBe('keep')
  })
  it('上の帯（上から 25% 未満）は左右を問わず ★5', () => {
    expect(tap(100, 0)).toBe('top')
    expect(tap(900, 199)).toBe('top')
    expect(tap(500, 100)).toBe('top')
  })
  it('帯の境目（25% ちょうど）から下は左右で分ける', () => {
    expect(tap(100, 200)).toBe('drop')
    expect(tap(900, 200)).toBe('keep')
  })
  it('枠がずれていても枠を基準にする', () => {
    expect(tapDecision(400, 150, 200, 100, 400, 400)).toBe('top')
    expect(tapDecision(400, 200, 200, 100, 400, 400)).toBe('keep')
    expect(tapDecision(399, 200, 200, 100, 400, 400)).toBe('drop')
  })
})

describe('slideKeyDecision', () => {
  it('1・← が落とす、3・→ が残す、5・↑ が ★5', () => {
    expect(slideKeyDecision({ key: '1' })).toBe('drop')
    expect(slideKeyDecision({ key: 'ArrowLeft' })).toBe('drop')
    expect(slideKeyDecision({ key: '3' })).toBe('keep')
    expect(slideKeyDecision({ key: 'ArrowRight' })).toBe('keep')
    expect(slideKeyDecision({ key: '5' })).toBe('top')
    expect(slideKeyDecision({ key: 'ArrowUp' })).toBe('top')
    expect(slideKeyDecision({ key: '1', code: 'Numpad1' })).toBe('drop')
    expect(slideKeyDecision({ key: '3', code: 'Numpad3' })).toBe('keep')
    expect(slideKeyDecision({ key: '5', code: 'Numpad5' })).toBe('top')
  })
  it('2・4・↓・Enter などは何もしない', () => {
    expect(slideKeyDecision({ key: '2' })).toBeNull()
    expect(slideKeyDecision({ key: '4' })).toBeNull()
    expect(slideKeyDecision({ key: 'ArrowDown' })).toBeNull()
    expect(slideKeyDecision({ key: 'Enter' })).toBeNull()
  })
  it('修飾キー付きは拾わない（Ctrl+1 は拡大のまま）', () => {
    expect(slideKeyDecision({ key: '1', ctrlKey: true })).toBeNull()
    expect(slideKeyDecision({ key: '!', code: 'Digit1', shiftKey: true })).toBeNull()
    expect(slideKeyDecision({ key: '3', altKey: true })).toBeNull()
    expect(slideKeyDecision({ key: 'ArrowLeft', ctrlKey: true })).toBeNull()
    expect(slideKeyDecision({ key: 'ArrowUp', metaKey: true })).toBeNull()
  })
})

describe('flyTarget', () => {
  it('左右は枠の外へ、上は上の外へ', () => {
    expect(flyTarget('drop', 1000, 600).x).toBeLessThan(-1000)
    expect(flyTarget('keep', 1000, 600).x).toBeGreaterThan(1000)
    expect(flyTarget('top', 1000, 600).x).toBe(0)
    expect(flyTarget('top', 1000, 600).y).toBeLessThan(-600)
  })
})

describe('fitContain', () => {
  it('縦横比を保って枠いっぱいにする', () => {
    expect(fitContain(300, 200, 900, 900)).toEqual({ width: 900, height: 600 })
    expect(fitContain(200, 300, 900, 600)).toEqual({ width: 400, height: 600 })
  })
  it('小さい写真は拡大する', () => {
    expect(fitContain(10, 10, 100, 50)).toEqual({ width: 50, height: 50 })
  })
  it('大きさが分からないときは枠いっぱい', () => {
    expect(fitContain(0, 0, 100, 50)).toEqual({ width: 100, height: 50 })
  })
})
