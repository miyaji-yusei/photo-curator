import { beforeAll, describe, expect, it } from 'vitest'
import { focusRegions, initWithBytes, type FocusRegions } from '~/lib/core'
import {
  bracketPath, bracketStyle, createFocusLoader, createMaskCache, fitWithin, maskKey, type GrayPixels
} from '~/utils/focusPeaking'
import { wasmBytes } from './helpers/wasmBytes.mjs'

function found(count = 1): FocusRegions {
  return { regions: Array.from({ length: count }, () => ({ x: 0.1, y: 0.1, w: 0.3, h: 0.3, strength: 0.8 })) }
}

describe('fitWithin', () => {
  it('長辺が上限に収まるよう縮め、拡大はしない', () => {
    expect(fitWithin(4000, 3000, 1024)).toEqual({ width: 1024, height: 768 })
    expect(fitWithin(800, 600, 1024)).toEqual({ width: 800, height: 600 })
    expect(fitWithin(0, 10, 1024)).toEqual({ width: 0, height: 0 })
    expect(fitWithin(10000, 1, 1024)).toEqual({ width: 1024, height: 1 })
  })
})

describe('bracketPath', () => {
  it('四隅だけのかぎ括弧を、解析した画像の画素の座標で描く', () => {
    // 200x100 の画像の (0.1, 0.2)-(0.6, 0.8) = 画素 (20,20)-(120,80)。短辺 60 の 30 % = 腕 18。
    const d = bracketPath({ x: 0.1, y: 0.2, w: 0.5, h: 0.6, strength: 1 }, 200, 100)
    expect(d).toBe('M20 38V20H38M102 20H120V38M120 62V80H102M38 80H20V62')
    expect(d.match(/M/g)).toHaveLength(4)
  })
})

describe('bracketStyle', () => {
  it('いちばん強い 1 個は太く、2・3 個目は細く薄い', () => {
    const first = bracketStyle(0)
    const second = bracketStyle(1)
    expect(first.width).toBeGreaterThan(second.width)
    expect(first.opacity).toBeGreaterThan(second.opacity)
    expect(bracketStyle(2)).toEqual(second)
  })
})

describe('createMaskCache', () => {
  it('上限を超えたら使われていない順に捨てる', () => {
    const cache = createMaskCache<number>(2)
    cache.set('a', 1)
    cache.set('b', 2)
    expect(cache.get('a')).toBe(1) // a を使った → b が古くなる
    cache.set('c', 3)
    expect(cache.size).toBe(2)
    expect(cache.get('b')).toBeUndefined()
    expect(cache.get('a')).toBe(1)
    expect(cache.get('c')).toBe(3)
  })
})

describe('createFocusLoader', () => {
  const pixels: GrayPixels = { gray: new Uint8Array(4), width: 2, height: 2 }
  function make(compute = () => found()) {
    const calls: Array<{ src: string, side: number }> = []
    const loader = createFocusLoader({
      read: async (src, side) => { calls.push({ src, side }); return pixels },
      compute,
      yieldToUi: async () => undefined,
      cacheLimit: 2
    })
    return { loader, calls }
  }

  it('同じ写真・同じ種類は 2 回目から読まずにキャッシュを返す', async () => {
    const { loader, calls } = make()
    const first = await loader.load({ photoId: 'p', kind: 'display', src: 'u1' })
    const second = await loader.load({ photoId: 'p', kind: 'display', src: 'u1' })
    expect(second).toBe(first)
    expect(first).toMatchObject({ width: 2, height: 2 })
    expect(first!.regions).toHaveLength(1)
    expect(calls).toHaveLength(1)
    expect(calls[0]!.side).toBe(1024)
  })

  it('表示用と原本は別に持ち、原本は大きめに読む', async () => {
    const { loader, calls } = make()
    await loader.load({ photoId: 'p', kind: 'display', src: 'u1' })
    await loader.load({ photoId: 'p', kind: 'original', src: 'u2' })
    expect(calls.map(c => c.side)).toEqual([1024, 1536])
    expect(loader.cached('p', 'display')).toBeDefined()
    expect(loader.cached('p', 'original')).toBeDefined()
    expect(maskKey('p', 'display')).not.toBe(maskKey('p', 'original'))
  })

  it('順番が来たときにもう要らなければ読まない', async () => {
    const { loader, calls } = make()
    let wanted = true
    const first = loader.load({ photoId: 'a', kind: 'display', src: 'u1' })
    const second = loader.load({ photoId: 'b', kind: 'display', src: 'u2', wanted: () => wanted })
    wanted = false
    expect(await first).not.toBeNull()
    expect(await second).toBeNull()
    expect(calls).toHaveLength(1)
  })

  it('同時に 2 枚は計算しない（1 枚ずつ）', async () => {
    let running = 0
    let peak = 0
    const loader = createFocusLoader({
      read: async () => { running += 1; peak = Math.max(peak, running); await Promise.resolve(); running -= 1; return pixels },
      compute: () => found(),
      yieldToUi: async () => undefined
    })
    await Promise.all(['a', 'b', 'c'].map(id => loader.load({ photoId: id, kind: 'display', src: id })))
    expect(peak).toBe(1)
  })

  it('読めないものは null でキャッシュに入れない。四角が 0 個なのは失敗ではなく、空の結果をキャッシュする', async () => {
    const failing = createFocusLoader({ read: async () => { throw new Error('tainted') }, compute: () => found(), yieldToUi: async () => undefined })
    expect(await failing.load({ photoId: 'x', kind: 'display', src: 'u' })).toBeNull()
    const unreadable = createFocusLoader({ read: async () => null, compute: () => found(), yieldToUi: async () => undefined })
    expect(await unreadable.load({ photoId: 'x', kind: 'display', src: 'u' })).toBeNull()
    expect(unreadable.cacheSize).toBe(0)
    const none = createFocusLoader({ read: async () => pixels, compute: () => ({ regions: [] }), yieldToUi: async () => undefined })
    expect(await none.load({ photoId: 'x', kind: 'display', src: 'u' })).toEqual({ width: 2, height: 2, regions: [] })
    expect(none.cacheSize).toBe(1)
  })

  it('キャッシュは上限つき', async () => {
    const { loader } = make()
    for (const id of ['a', 'b', 'c']) await loader.load({ photoId: id, kind: 'display', src: id })
    expect(loader.cacheSize).toBe(2)
    expect(loader.cached('a', 'display')).toBeUndefined()
  })
})

describe('core の focusRegions（wasm 越し）', () => {
  beforeAll(() => initWithBytes(wasmBytes()))

  function checker(width: number, height: number, period: number, inside: (x: number) => boolean): Uint8Array {
    const gray = new Uint8Array(width * height).fill(128)
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        if (inside(x)) gray[y * width + x] = (Math.floor(x / period) + Math.floor(y / period)) % 2 === 0 ? 20 : 235
      }
    }
    return gray
  }

  it('左がくっきり・右がのっぺりなら、四角は左だけ（0〜1 の割合で返る）', () => {
    const width = 256
    const height = 128
    const result = focusRegions(checker(width, height, 8, x => x < width / 2), width, height)
    expect(result.regions.length).toBeGreaterThan(0)
    expect(result.regions.length).toBeLessThanOrEqual(3)
    for (const region of result.regions) {
      expect(region.x).toBeGreaterThanOrEqual(0)
      expect(region.x + region.w).toBeLessThanOrEqual(0.55)
      expect(region.y + region.h).toBeLessThanOrEqual(1.0001)
      expect(region.strength).toBeGreaterThan(0)
    }
  })

  it('のっぺりした画像・読めない入力では四角が 0 個', () => {
    expect(focusRegions(new Uint8Array(128 * 128).fill(100), 128, 128)).toEqual({ regions: [] })
    expect(focusRegions(new Uint8Array(5), 3, 3)).toEqual({ regions: [] })
  })

  it('長辺 2048 の入力を 1 枚あたり 100ms 前後で処理する（目安。遅い環境では緩める）', () => {
    const width = 1536
    const height = 1024
    const gray = new Uint8Array(width * height)
    for (let i = 0; i < gray.length; i += 1) gray[i] = (i * 31) & 255
    focusRegions(gray, width, height) // 暖機
    const started = performance.now()
    focusRegions(gray, width, height)
    const elapsed = performance.now() - started
    console.log(`focusRegions 1536x1024: ${elapsed.toFixed(1)}ms`)
    expect(elapsed).toBeLessThan(400)
  })
})
