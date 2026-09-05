import { describe, expect, it } from 'vitest'
import { zipEntriesByRating } from '~/utils/shareExport'

const blob = (text: string) => new Blob([new TextEncoder().encode(text)])

describe('zipEntriesByRating', () => {
  it('星ごとのフォルダに振り分ける', () => {
    const entries = zipEntriesByRating([
      { name: 'a.jpg', rating: 5, blob: blob('a') },
      { name: 'b.jpg', rating: 3, blob: blob('b') },
      { name: 'c.jpg', rating: 0, blob: blob('c') }
    ])
    expect(entries.map(entry => entry.path))
      .toEqual(['star-5/a.jpg', 'star-3/b.jpg', 'star-0/c.jpg'])
  })

  it('同じ星に同名があれば連番を付けて上書きを避ける', () => {
    const entries = zipEntriesByRating([
      { name: 'IMG_0001.jpg', rating: 5, blob: blob('a') },
      { name: 'IMG_0001.jpg', rating: 5, blob: blob('b') }
    ])
    expect(entries.map(entry => entry.path))
      .toEqual(['star-5/IMG_0001.jpg', 'star-5/IMG_0001 (2).jpg'])
  })

  it('星が違えば同名でもそのまま入る', () => {
    const entries = zipEntriesByRating([
      { name: 'IMG_0001.jpg', rating: 5, blob: blob('a') },
      { name: 'IMG_0001.jpg', rating: 3, blob: blob('b') }
    ])
    expect(entries.map(entry => entry.path))
      .toEqual(['star-5/IMG_0001.jpg', 'star-3/IMG_0001.jpg'])
  })

  it('中身と更新時刻をそのまま引き継ぐ', () => {
    const source = blob('payload')
    const entries = zipEntriesByRating([
      { name: 'a.jpg', rating: 4, blob: source, modifiedAt: 12_345 }
    ])
    expect(entries[0]!.blob).toBe(source)
    expect(entries[0]!.modifiedAt).toBe(12_345)
  })
})
