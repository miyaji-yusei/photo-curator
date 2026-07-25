import { describe, expect, it } from 'vitest'
import { buildShortcutPayload, formatCapturedAt, zipEntriesByRating } from '~/utils/shareExport'

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

describe('formatCapturedAt', () => {
  it('保存した値を暦の表記に戻す', () => {
    // civilTimestampMs が UTC 基準で作った値を、そのまま読み戻す。
    expect(formatCapturedAt(Date.UTC(2026, 5, 30, 18, 19, 32))).toBe('2026-06-30 18:19:32')
  })

  it('1 桁の月日時分秒を 0 で埋める', () => {
    expect(formatCapturedAt(Date.UTC(2026, 0, 2, 3, 4, 5))).toBe('2026-01-02 03:04:05')
  })

  it('撮影日時が無ければ空にする', () => {
    expect(formatCapturedAt(null)).toBe('')
    expect(formatCapturedAt(Number.NaN)).toBe('')
  })
})

describe('buildShortcutPayload', () => {
  it('1 行 1 枚の TSV にする', () => {
    const payload = buildShortcutPayload([
      { name: 'IMG_0001.HEIC', rating: 5, capturedAt: Date.UTC(2026, 5, 30, 18, 19, 32) },
      { name: 'IMG_0002.HEIC', rating: 4, capturedAt: Date.UTC(2026, 5, 30, 18, 20, 0) }
    ])
    expect(payload.split('\n')).toEqual([
      'IMG_0001.HEIC\t2026-06-30 18:19:32\t5',
      'IMG_0002.HEIC\t2026-06-30 18:20:00\t4'
    ])
  })

  it('撮影日時が無くても列の数は変わらない', () => {
    // ショートカット側が列位置で読むので、欠けても列を詰めない。
    const payload = buildShortcutPayload([{ name: 'a.jpg', rating: 2, capturedAt: null }])
    expect(payload).toBe('a.jpg\t\t2')
    expect(payload.split('\t')).toHaveLength(3)
  })

  it('空なら空文字', () => {
    expect(buildShortcutPayload([])).toBe('')
  })
})
