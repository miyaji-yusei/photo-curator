import { beforeAll, describe, expect, it } from 'vitest'
import { focusMask, initWithBytes, type FocusMask } from '~/lib/core'
import {
  createFocusMaskLoader, createMaskCache, fitWithin, maskKey, maskToRgba, type GrayPixels
} from '~/utils/focusPeaking'
import { wasmBytes } from './helpers/wasmBytes.mjs'

function mask(value: number, width = 2, height = 2): FocusMask {
  return { width, height, data: new Uint8Array(width * height).fill(value) }
}

describe('fitWithin', () => {
  it('長辺が上限に収まるよう縮め、拡大はしない', () => {
    expect(fitWithin(4000, 3000, 1024)).toEqual({ width: 1024, height: 768 })
    expect(fitWithin(800, 600, 1024)).toEqual({ width: 800, height: 600 })
    expect(fitWithin(0, 10, 1024)).toEqual({ width: 0, height: 0 })
    expect(fitWithin(10000, 1, 1024)).toEqual({ width: 1024, height: 1 })
  })
})

describe('maskToRgba', () => {
  it('強さをアルファにして色を塗る', () => {
    const rgba = maskToRgba({ width: 2, height: 1, data: Uint8Array.from([0, 200]) }, [1, 2, 3])
    expect(Array.from(rgba)).toEqual([1, 2, 3, 0, 1, 2, 3, 200])
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

describe('createFocusMaskLoader', () => {
  const pixels: GrayPixels = { gray: new Uint8Array(4), width: 2, height: 2 }
  function make(compute = () => mask(255)) {
    const calls: Array<{ src: string, side: number }> = []
    const loader = createFocusMaskLoader({
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
    const loader = createFocusMaskLoader({
      read: async () => { running += 1; peak = Math.max(peak, running); await Promise.resolve(); running -= 1; return pixels },
      compute: () => mask(255),
      yieldToUi: async () => undefined
    })
    await Promise.all(['a', 'b', 'c'].map(id => loader.load({ photoId: id, kind: 'display', src: id })))
    expect(peak).toBe(1)
  })

  it('読めない・空のマスクは null で、キャッシュに入れない', async () => {
    const failing = createFocusMaskLoader({ read: async () => { throw new Error('tainted') }, compute: () => mask(1), yieldToUi: async () => undefined })
    expect(await failing.load({ photoId: 'x', kind: 'display', src: 'u' })).toBeNull()
    const empty = createFocusMaskLoader({ read: async () => pixels, compute: () => ({ width: 0, height: 0, data: new Uint8Array() }), yieldToUi: async () => undefined })
    expect(await empty.load({ photoId: 'x', kind: 'display', src: 'u' })).toBeNull()
    expect(empty.cacheSize).toBe(0)
  })

  it('キャッシュは上限つき', async () => {
    const { loader } = make()
    for (const id of ['a', 'b', 'c']) await loader.load({ photoId: id, kind: 'display', src: id })
    expect(loader.cacheSize).toBe(2)
    expect(loader.cached('a', 'display')).toBeUndefined()
  })
})

describe('core の focusMask（wasm 越し）', () => {
  beforeAll(() => initWithBytes(wasmBytes()))

  it('左がくっきり・右がのっぺりなら、印は左だけに出る', () => {
    const width = 128
    const height = 64
    const gray = new Uint8Array(width * height)
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        gray[y * width + x] = x < width / 2 ? (((x >> 2) + (y >> 2)) % 2 === 0 ? 20 : 235) : 128
      }
    }
    const result = focusMask(gray, width, height)
    expect([result.width, result.height]).toEqual([128, 64])
    let left = 0
    let right = 0
    for (let y = 0; y < result.height; y += 1) {
      for (let x = 0; x < result.width; x += 1) {
        const value = result.data[y * result.width + x]!
        if (x < result.width / 2) left += value
        else right += value
      }
    }
    expect(left).toBeGreaterThan(0)
    expect(right).toBe(0)
  })

  it('読めない入力では空のマスク', () => {
    const result = focusMask(new Uint8Array(5), 3, 3)
    expect([result.width, result.height, result.data.length]).toEqual([0, 0, 0])
  })

  it('長辺 2048 の入力を 1 枚あたり 50ms 前後で処理する（目安。遅い環境では緩める）', () => {
    const width = 1536
    const height = 1024
    const gray = new Uint8Array(width * height)
    for (let i = 0; i < gray.length; i += 1) gray[i] = (i * 31) & 255
    focusMask(gray, width, height) // 暖機
    const started = performance.now()
    focusMask(gray, width, height)
    const elapsed = performance.now() - started
    console.log(`focusMask 1536x1024: ${elapsed.toFixed(1)}ms`)
    expect(elapsed).toBeLessThan(250)
  })
})
