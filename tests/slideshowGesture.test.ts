import { describe, expect, it } from 'vitest'
import {
  TAP_SLOP, decideThreshold, dragFeedback, fitContain, flyTarget, isTap, judgeDrag, slideKeyDecision, tapDecision,
  isDoubleTap, TOP_ZONE_RATIO, SIDE_ZONE_RATIO, DOUBLE_TAP_MS, DOUBLE_TAP_DISTANCE
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

describe('tapDecision（枠 left=0, top=0, 幅 1000, 高さ 800 → 上は y<240、左は x<300、右は x>700）', () => {
  const tap = (x: number, y: number) => tapDecision(x, y, 0, 0, 1000, 800)
  it('定数は 0.30', () => {
    expect(TOP_ZONE_RATIO).toBe(0.3)
    expect(SIDE_ZONE_RATIO).toBe(0.3)
  })
  it('上の 30% 未満は左右・中央を問わず ★5', () => {
    expect(tap(100, 0)).toBe('top')
    expect(tap(900, 239)).toBe('top')
    expect(tap(500, 100)).toBe('top')
    expect(tap(300, 239)).toBe('top')
    expect(tap(700, 239)).toBe('top')
  })
  it('上の線（30% ちょうど）から下は左右・中央で分ける', () => {
    expect(tap(100, 240)).toBe('drop')
    expect(tap(900, 240)).toBe('keep')
    expect(tap(500, 240)).toBe('center')
  })
  it('左端 30% 未満は落とす、右端（70% より大きい）は残す', () => {
    expect(tap(0, 400)).toBe('drop')
    expect(tap(299, 400)).toBe('drop')
    expect(tap(701, 400)).toBe('keep')
    expect(tap(999, 799)).toBe('keep')
    expect(tap(100, 799)).toBe('drop')
  })
  it('ちょうど 30%・70% の線は中央（何もしない）', () => {
    expect(tap(300, 400)).toBe('center')
    expect(tap(700, 400)).toBe('center')
    expect(tap(300, 799)).toBe('center')
    expect(tap(700, 240)).toBe('center')
  })
  it('中央の縦帯は中心だけでなく上の線の下から最下部まで何もしない', () => {
    for (const y of [240, 300, 400, 500, 700, 790, 799]) {
      expect(tap(500, y)).toBe('center')
      expect(tap(301, y)).toBe('center')
      expect(tap(699, y)).toBe('center')
    }
  })
  it('枠がずれていても枠を基準にする（枠 x=200..600, y=100..500 → 上は y<220、左は x<320、右は x>480）', () => {
    expect(tapDecision(400, 219, 200, 100, 400, 400)).toBe('top')
    expect(tapDecision(319, 300, 200, 100, 400, 400)).toBe('drop')
    expect(tapDecision(320, 300, 200, 100, 400, 400)).toBe('center')
    expect(tapDecision(480, 499, 200, 100, 400, 400)).toBe('center')
    expect(tapDecision(481, 300, 200, 100, 400, 400)).toBe('keep')
    expect(tapDecision(400, 220, 200, 100, 400, 400)).toBe('center')
  })
})

describe('二度押しの拡大は中央の縦帯だけ（tapDecision と isDoubleTap の組み合わせ）', () => {
  // 画面側（SlideshowView）と同じ手順: 'center' の結果だけを二度押しの記録に使う
  function run(points: Array<[number, number, number]>) {
    let last: TapRecord | null = null
    let zoomed = false
    for (const [t, x, y] of points) {
      const result = tapDecision(x, y, 0, 0, 1000, 800)
      if (result !== 'center') { last = null; continue }
      const now = { time: t, x, y }
      if (isDoubleTap(last, now)) { last = null; zoomed = true } else last = now
    }
    return zoomed
  }
  it('中央の縦帯のどこでも 2 回で拡大（上の線の直下・中心・最下部）', () => {
    expect(run([[0, 500, 245], [200, 500, 250]])).toBe(true)
    expect(run([[0, 500, 400], [300, 510, 405]])).toBe(true)
    expect(run([[0, 400, 790], [100, 420, 795]])).toBe(true)
    expect(run([[0, 301, 500], [100, 305, 500]])).toBe(true)
  })
  it('左右の領域・上の領域では二度押ししても拡大しない', () => {
    expect(run([[0, 100, 500], [100, 100, 500]])).toBe(false)
    expect(run([[0, 900, 500], [100, 900, 500]])).toBe(false)
    expect(run([[0, 500, 100], [100, 500, 100]])).toBe(false)
  })
  it('中央の間に左右・上のタップが挟まれたら数え直し、遅い・遠い 2 回目も拡大しない', () => {
    expect(run([[0, 500, 400], [50, 100, 400], [100, 500, 400]])).toBe(false)
    expect(run([[0, 500, 400], [301, 500, 400]])).toBe(false)
    expect(run([[0, 400, 400], [100, 430, 400]])).toBe(false)
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
