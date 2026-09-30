import { describe, expect, it } from 'vitest'
import { burstNeighborhood } from '~/utils/burstNeighborhood'

const photo = (name: string, capturedAt: number | null, dHash: string | null = '0000000000000000') =>
  ({ relativePath: name, capturedAt, dHash })

describe('burstNeighborhood', () => {
  const run = [
    photo('a', 0),
    photo('b', 3000),
    photo('c', 10_000),
    photo('d', 12_000),
    photo('e', 15_500),
    photo('f', 40_000)
  ]

  it('選んだ写真の前後 4000ms に入る写真を、渡された並びで返す', () => {
    expect(burstNeighborhood(run, ['c', 'd']).map(item => item.relativePath)).toEqual(['c', 'd', 'e'])
  })

  it('選んだ写真が占める幅（最小〜最大）から広げる', () => {
    expect(burstNeighborhood(run, ['a', 'e']).map(item => item.relativePath))
      .toEqual(['a', 'b', 'c', 'd', 'e'])
  })

  it('ちょうど窓の縁は入り、1ms 外は入らない', () => {
    expect(burstNeighborhood(run, ['a']).map(item => item.relativePath)).toEqual(['a', 'b'])
    expect(burstNeighborhood([photo('a', 0), photo('b', 4001)], ['a']).map(item => item.relativePath)).toEqual(['a'])
  })

  it('窓は指定できる', () => {
    expect(burstNeighborhood(run, ['c'], 2500).map(item => item.relativePath)).toEqual(['c', 'd'])
  })

  it('撮影時刻か指紋の無い写真は対象にしない', () => {
    const mixed = [photo('a', 0), photo('x', 1000, null), photo('y', null), photo('b', 2000)]
    expect(burstNeighborhood(mixed, ['a']).map(item => item.relativePath)).toEqual(['a', 'b'])
  })

  it('選んだ写真が無い・見つからないときは空', () => {
    expect(burstNeighborhood(run, [])).toEqual([])
    expect(burstNeighborhood(run, ['nope'])).toEqual([])
  })
})
