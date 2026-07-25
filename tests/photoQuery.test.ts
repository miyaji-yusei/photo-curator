import { describe, expect, it } from 'vitest'
import {
  compareByCaptureOrder, comparePhotos, filterPhotos, pagePhotos, summarizeRatings
} from '~/utils/photoQuery'
import type { QueryablePhoto } from '~/utils/photoQuery'

const photo = (
  relativePath: string, rating = 0, capturedAt: number | null = null, isMissing = false
): QueryablePhoto => ({ relativePath, rating, capturedAt, isMissing })

const paths = (photos: QueryablePhoto[]) => photos.map(item => item.relativePath)

describe('comparePhotos', () => {
  it('星の高い順に並べ、同点はファイル順で安定させる', () => {
    const photos = [photo('c.jpg', 3), photo('a.jpg', 5), photo('b.jpg', 3)]
    expect(paths([...photos].sort(comparePhotos('rating')))).toEqual(['a.jpg', 'b.jpg', 'c.jpg'])
  })

  it('name はファイル順だけ', () => {
    const photos = [photo('c.jpg', 5), photo('a.jpg', 0), photo('b.jpg', 3)]
    expect(paths([...photos].sort(comparePhotos('name')))).toEqual(['a.jpg', 'b.jpg', 'c.jpg'])
  })
})

describe('compareByCaptureOrder', () => {
  it('撮影順に並べる', () => {
    const photos = [photo('c.jpg', 0, 300), photo('a.jpg', 0, 100), photo('b.jpg', 0, 200)]
    expect(paths([...photos].sort(compareByCaptureOrder))).toEqual(['a.jpg', 'b.jpg', 'c.jpg'])
  })

  it('撮影日時が無い写真は末尾に回し、その中はファイル順にする', () => {
    const photos = [
      photo('z-none.jpg', 0, null), photo('m.jpg', 0, 200),
      photo('b-none.jpg', 0, null), photo('a.jpg', 0, 100)
    ]
    expect(paths([...photos].sort(compareByCaptureOrder)))
      .toEqual(['a.jpg', 'm.jpg', 'b-none.jpg', 'z-none.jpg'])
  })

  it('同じ撮影時刻はファイル順で安定させる', () => {
    const photos = [photo('b.jpg', 0, 100), photo('a.jpg', 0, 100)]
    expect(paths([...photos].sort(compareByCaptureOrder))).toEqual(['a.jpg', 'b.jpg'])
  })

  it('撮影日時 0 は「無し」と混同しない', () => {
    // 0 を falsy として扱うと、1970 年の写真が「日時なし」の側に回ってしまう。
    // ファイル名の順は**わざと逆**にしてある。名前順の後段に助けられて
    // テストが素通りしないようにするため。
    const photos = [photo('a-none.jpg', 0, null), photo('z-epoch.jpg', 0, 0)]
    expect(paths([...photos].sort(compareByCaptureOrder))).toEqual(['z-epoch.jpg', 'a-none.jpg'])
  })
})

describe('filterPhotos', () => {
  const photos = [photo('a.jpg', 0), photo('b.jpg', 3), photo('c.jpg', 3), photo('d.jpg', 5)]

  it('星ちょうど一致で絞る', () => {
    expect(paths(filterPhotos(photos, 3))).toEqual(['b.jpg', 'c.jpg'])
    expect(paths(filterPhotos(photos, 5))).toEqual(['d.jpg'])
    expect(filterPhotos(photos, 4)).toEqual([])
  })

  it('★0 も絞り込みの対象（未評価を選別できる）', () => {
    expect(paths(filterPhotos(photos, 0))).toEqual(['a.jpg'])
  })

  it('null / undefined は全件', () => {
    expect(filterPhotos(photos, null)).toHaveLength(4)
    expect(filterPhotos(photos)).toHaveLength(4)
  })

  it('見失った写真は常に除く', () => {
    const withMissing = [...photos, photo('gone.jpg', 3, null, true)]
    expect(paths(filterPhotos(withMissing, 3))).toEqual(['b.jpg', 'c.jpg'])
    expect(filterPhotos(withMissing, null)).toHaveLength(4)
  })
})

describe('pagePhotos', () => {
  const items = ['a', 'b', 'c', 'd', 'e']

  it('offset と limit で切り出す', () => {
    expect(pagePhotos(items, 0, 2)).toEqual(['a', 'b'])
    expect(pagePhotos(items, 2, 2)).toEqual(['c', 'd'])
    expect(pagePhotos(items, 4, 2)).toEqual(['e'])
    expect(pagePhotos(items, 10, 2)).toEqual([])
  })

  it('負の offset と 0 以下の limit を丸める', () => {
    expect(pagePhotos(items, -5, 2)).toEqual(['a', 'b'])
    expect(pagePhotos(items, 0, 0)).toEqual(['a'])
  })
})

describe('summarizeRatings', () => {
  it('星ごとの枚数と総数を数える', () => {
    const summary = summarizeRatings([
      photo('a.jpg', 0), photo('b.jpg', 3), photo('c.jpg', 3), photo('d.jpg', 5)
    ])
    expect(summary.counts).toEqual([1, 0, 0, 2, 0, 1])
    expect(summary.total).toBe(4)
  })

  it('見失った写真は数えない', () => {
    const summary = summarizeRatings([photo('a.jpg', 3), photo('gone.jpg', 3, null, true)])
    expect(summary.counts[3]).toBe(1)
    expect(summary.total).toBe(1)
  })

  it('範囲外の星は端に丸める', () => {
    const summary = summarizeRatings([photo('a.jpg', 9), photo('b.jpg', -2)])
    expect(summary.counts[5]).toBe(1)
    expect(summary.counts[0]).toBe(1)
  })
})
