import { describe, expect, it } from 'vitest'
import { D_HASH_HEIGHT, D_HASH_WIDTH, dHashFromLuma, hammingDistance, lumaFromRgba } from '~/utils/dhash'

/** 9×8 の輝度を、行ごとの値の配列から作る。 */
function luma(rows: number[][]) {
  return rows.flat()
}

const flat = (value: number) =>
  Array.from({ length: D_HASH_HEIGHT }, () => new Array<number>(D_HASH_WIDTH).fill(value))

describe('dHashFromLuma', () => {
  it('平坦な画像は 1 ビットも立たない', () => {
    // どの隣とも差が無い（左が明るくない）ので全ビット 0。
    expect(dHashFromLuma(luma(flat(128)))).toBe('0000000000000000')
  })

  it('左から右へ明るくなる画像も 1 ビットも立たない', () => {
    const rows = Array.from({ length: D_HASH_HEIGHT }, () =>
      Array.from({ length: D_HASH_WIDTH }, (_, x) => x * 10)
    )
    expect(dHashFromLuma(luma(rows))).toBe('0000000000000000')
  })

  it('右から左へ明るくなる画像は全ビット立つ', () => {
    const rows = Array.from({ length: D_HASH_HEIGHT }, () =>
      Array.from({ length: D_HASH_WIDTH }, (_, x) => (D_HASH_WIDTH - x) * 10)
    )
    // 64 ビットすべて 1。32bit のビット演算だと桁が溢れてここで壊れる。
    expect(dHashFromLuma(luma(rows))).toBe('ffffffffffffffff')
  })

  it('ビットの位置は y * 8 + x で並ぶ', () => {
    // 平坦な背景から 1 画素だけ明るくすると、その画素と右隣の間にだけ差が出る。
    // 背景を 0 にすると明るい画素の「右隣とその右」にも差ができて 2 ビット立つ。
    const withSpike = (y: number, x: number) => {
      const rows = flat(100)
      rows[y]![x] = 200
      return luma(rows)
    }
    // 最終行（y=7）の x=7 → ビット 63。
    expect(dHashFromLuma(withSpike(7, 7))).toBe('8000000000000000')
    // 先頭行の x=0 → ビット 0。
    expect(dHashFromLuma(withSpike(0, 0))).toBe('0000000000000001')
    // y=1 の x=0 → ビット 8。
    expect(dHashFromLuma(withSpike(1, 0))).toBe('0000000000000100')
  })

  it('縦方向の差は見ない', () => {
    // 行ごとに明るさが違っても、横に差が無ければ 0。
    const rows = Array.from({ length: D_HASH_HEIGHT }, (_, y) =>
      new Array<number>(D_HASH_WIDTH).fill(y * 30)
    )
    expect(dHashFromLuma(luma(rows))).toBe('0000000000000000')
  })

  it('同じ明るさでは立てず、1 だけ明るければ立つ', () => {
    // 比較は「真に大きい」。等しいときに立ててしまうと、平坦な写真同士の
    // 距離が 0 にならず連写判定が崩れる。
    const equal = flat(100)
    expect(dHashFromLuma(luma(equal))).toBe('0000000000000000')

    const barely = flat(100)
    barely[0]![0] = 101
    expect(dHashFromLuma(luma(barely))).toBe('0000000000000001')
  })
})

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

describe('hammingDistance', () => {
  it('同じハッシュは 0', () => {
    expect(hammingDistance('0f0f0f0f0f0f0f0f', '0f0f0f0f0f0f0f0f')).toBe(0)
  })

  it('全ビット違えば 64', () => {
    expect(hammingDistance('0000000000000000', 'ffffffffffffffff')).toBe(64)
  })

  it('最上位ビットの違いも数える', () => {
    // 32bit 演算だと上位側の差を取りこぼす。
    expect(hammingDistance('8000000000000000', '0000000000000000')).toBe(1)
    expect(hammingDistance('ffffffffffffffff', '7fffffffffffffff')).toBe(1)
  })

  it('立っているビットの数だけ増える', () => {
    expect(hammingDistance('0000000000000000', '0000000000000007')).toBe(3)
  })
})
