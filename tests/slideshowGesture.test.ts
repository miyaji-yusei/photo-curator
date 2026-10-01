import { describe, expect, it } from 'vitest'
import {
  TAP_SLOP, decideThreshold, dragFeedback, fitContain, flyTarget, isTap, judgeDrag, slideKeyDecision, tapDecision,
  isDoubleTap, CENTER_RATIO, DOUBLE_TAP_MS, DOUBLE_TAP_DISTANCE
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
    expect(tap(379, 400)).toBe('drop')
    expect(tap(621, 400)).toBe('keep')
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

describe('tapDecision の中心（枠 1000×800 → 中心 (500,400)、±120 × ±96、上の帯は y<200）', () => {
  const tap = (x: number, y: number) => tapDecision(x, y, 0, 0, 1000, 800)
  it('定数は 0.12', () => {
    expect(CENTER_RATIO).toBe(0.12)
  })
  it('ほぼ中心は何もしない（center）', () => {
    expect(tap(500, 400)).toBe('center')
    expect(tap(450, 350)).toBe('center')
    expect(tap(550, 450)).toBe('center')
  })
  it('境目（横 ±120・縦 ±96）は中心に含み、1px 外は左右で分ける', () => {
    expect(tap(380, 400)).toBe('center')
    expect(tap(620, 400)).toBe('center')
    expect(tap(379, 400)).toBe('drop')
    expect(tap(621, 400)).toBe('keep')
    expect(tap(500, 304)).toBe('center')
    expect(tap(500, 496)).toBe('center')
    expect(tap(500, 497)).toBe('keep')
    expect(tap(499, 497)).toBe('drop')
    expect(tap(500, 303)).toBe('keep')
    expect(tap(499, 303)).toBe('drop')
  })
  it('帯が優先（帯の中は常に top。中心の長方形は帯の外）', () => {
    expect(tap(500, 199)).toBe('top')
    expect(tap(500, 200)).toBe('keep')
    // 低い枠では中心の長方形が帯にかかる。そのときも帯が先
    expect(tapDecision(500, 9, 0, 0, 1000, 40)).toBe('top')
    expect(tapDecision(500, 16, 0, 0, 1000, 40)).toBe('center')
  })
  it('枠がずれていても枠の中心を基準にする', () => {
    expect(tapDecision(400, 300, 200, 100, 400, 400)).toBe('center')
    expect(tapDecision(448, 300, 200, 100, 400, 400)).toBe('center')
    expect(tapDecision(449, 300, 200, 100, 400, 400)).toBe('keep')
  })
})

describe('isDoubleTap（300ms 以内・24px 以内）', () => {
  const at = (time: number, x = 100, y = 100) => ({ time, x, y })
  it('定数', () => {
    expect(DOUBLE_TAP_MS).toBe(300)
    expect(DOUBLE_TAP_DISTANCE).toBe(24)
  })
  it('記録が無ければ二度押しでない（中心以外のタップで記録が消えた後）', () => {
    expect(isDoubleTap(null, at(0))).toBe(false)
  })
  it('時間の境目', () => {
    expect(isDoubleTap(at(1000), at(1100))).toBe(true)
    expect(isDoubleTap(at(1000), at(1300))).toBe(true)
    expect(isDoubleTap(at(1000), at(1301))).toBe(false)
  })
  it('距離の境目', () => {
    expect(isDoubleTap(at(0, 100, 100), at(100, 124, 100))).toBe(true)
    expect(isDoubleTap(at(0, 100, 100), at(100, 125, 100))).toBe(false)
    expect(isDoubleTap(at(0, 100, 100), at(100, 118, 118))).toBe(false)
    expect(isDoubleTap(at(0, 100, 100), at(100, 116, 116))).toBe(true)
  })
  it('時刻が戻っていたら二度押しでない', () => {
    expect(isDoubleTap(at(1000), at(999))).toBe(false)
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
    expect(slideKeyDecision({ key: '1', code: 'Digit1' })).toBe('drop')
    expect(slideKeyDecision({ key: '3', code: 'Digit3' })).toBe('keep')
    expect(slideKeyDecision({ key: '5', code: 'Digit5' })).toBe('top')
  })
  it('テンキーは NumLock が切れていても（key が End・PageDown・Clear でも）code で判定する', () => {
    expect(slideKeyDecision({ key: 'End', code: 'Numpad1' })).toBe('drop')
    expect(slideKeyDecision({ key: 'PageDown', code: 'Numpad3' })).toBe('keep')
    expect(slideKeyDecision({ key: 'Clear', code: 'Numpad5' })).toBe('top')
    expect(slideKeyDecision({ key: '1', code: 'Numpad1' })).toBe('drop')
  })
  it('テンキーの 0・2 などは何もしない。修飾キー付きのテンキーも拾わない', () => {
    expect(slideKeyDecision({ key: '2', code: 'Numpad2' })).toBeNull()
    expect(slideKeyDecision({ key: 'ArrowDown', code: 'Numpad2' })).toBeNull()
    expect(slideKeyDecision({ key: '0', code: 'Numpad0' })).toBeNull()
    expect(slideKeyDecision({ key: 'Enter', code: 'NumpadEnter' })).toBeNull()
    expect(slideKeyDecision({ key: 'End', code: 'Numpad1', ctrlKey: true })).toBeNull()
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
