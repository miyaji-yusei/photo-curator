import { describe, expect, it } from 'vitest'
import { collapseBursts } from '~/utils/collapseBursts'

const row = (relativePath: string, rating: number) => ({ relativePath, rating })

describe('collapseBursts', () => {
  it('連写に入っていない写真はそのまま 1 枚ずつ', () => {
    const tiles = collapseBursts([row('a', 1), row('b', 2)], {})
    expect(tiles.map(tile => tile.photo.relativePath)).toEqual(['a', 'b'])
    expect(tiles.every(tile => tile.burstSize === 1)).toBe(true)
  })

  it('連写は、読み込んだ行の中で星が一番高い 1 枚だけにする', () => {
    const rows = [row('a', 1), row('b', 3), row('c', 2), row('z', 1)]
    const tiles = collapseBursts(rows, { a: ['a', 'b', 'c'] })
    expect(tiles.map(tile => tile.photo.relativePath)).toEqual(['b', 'z'])
    expect(tiles[0]).toMatchObject({ burstSize: 3, mates: ['a', 'b', 'c'] })
  })

  it('タイルは、その連写の最初の行の位置に出る', () => {
    const rows = [row('x', 1), row('a', 1), row('y', 1), row('b', 2)]
    const tiles = collapseBursts(rows, { a: ['a', 'b'] })
    expect(tiles.map(tile => tile.photo.relativePath)).toEqual(['x', 'b', 'y'])
  })

  it('星が同じなら、先に読み込んだ行を残す', () => {
    const tiles = collapseBursts([row('b', 2), row('a', 2)], { a: ['a', 'b'] })
    expect(tiles.map(tile => tile.photo.relativePath)).toEqual(['b'])
  })

  it('⧉N の N は、読み込んだ数ではなく仲間の数', () => {
    // 仲間 4 枚のうち、絞り込みに合って読み込んだのは 2 枚だけ。
    const tiles = collapseBursts([row('a', 2), row('d', 2)], { a: ['a', 'b', 'c', 'd'] })
    expect(tiles).toHaveLength(1)
    expect(tiles[0]!.burstSize).toBe(4)
  })

  it('ページをまたぐ仲間は畳まない（読み込んだ行の中だけ）', () => {
    const page1 = collapseBursts([row('a', 2)], { a: ['a', 'b'] })
    const page2 = collapseBursts([row('b', 2)], { a: ['a', 'b'] })
    expect(page1).toHaveLength(1)
    expect(page2).toHaveLength(1)
  })

  it('1 人だけの組・members が無いときは畳まない', () => {
    expect(collapseBursts([row('a', 1)], { a: ['a'] })[0]!.burstSize).toBe(1)
    expect(collapseBursts([row('a', 1)], null)).toHaveLength(1)
  })

  it('入力の行は変えない', () => {
    const rows = [row('a', 1), row('b', 2)]
    collapseBursts(rows, { a: ['a', 'b'] })
    expect(rows).toEqual([row('a', 1), row('b', 2)])
  })
})
