import { describe, expect, it } from 'vitest'
import { lumaFromRgba } from '~/utils/dhash'

// ハッシュ値そのもの（dHashFromGray）は core の試験（core-wasm-fixtures）が見る。

describe('lumaFromRgba', () => {
  it('BT.709 の重みで輝度を出す', () => {
    // 純色 1 画素ずつ。image クレートと同じ整数除算。
    const red = lumaFromRgba([255, 0, 0, 255], 1)
    const green = lumaFromRgba([0, 255, 0, 255], 1)
    const blue = lumaFromRgba([0, 0, 255, 255], 1)
    expect(red[0]).toBe(Math.floor((2126 * 255) / 10000))
    expect(green[0]).toBe(Math.floor((7152 * 255) / 10000))
    expect(blue[0]).toBe(Math.floor((722 * 255) / 10000))
    // 緑がいちばん明るく見える、が BT.709 の要点。
    expect(green[0]!).toBeGreaterThan(red[0]!)
    expect(red[0]!).toBeGreaterThan(blue[0]!)
  })

  it('白は 255、黒は 0', () => {
    expect(lumaFromRgba([255, 255, 255, 255], 1)[0]).toBe(255)
    expect(lumaFromRgba([0, 0, 0, 255], 1)[0]).toBe(0)
  })
})
