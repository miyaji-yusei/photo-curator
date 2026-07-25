import { describe, expect, it } from 'vitest'
import {
  civilTimestampMs, daysInMonth, digitGroups, filenameCaptureTime,
  isWeakSource, resolveCaptureTime, timestampFromGroups
} from '~/utils/captureTime'

describe('civilTimestampMs', () => {
  it('暦の日時を UTC のミリ秒にする', () => {
    expect(civilTimestampMs(2026, 6, 30, 18, 19, 32))
      .toBe(Date.UTC(2026, 5, 30, 18, 19, 32))
  })

  it('暦として不正な値は採らない', () => {
    expect(civilTimestampMs(2026, 13, 1, 0, 0, 0)).toBeNull()
    expect(civilTimestampMs(2026, 0, 1, 0, 0, 0)).toBeNull()
    expect(civilTimestampMs(2026, 2, 30, 0, 0, 0)).toBeNull()
    expect(civilTimestampMs(2026, 4, 31, 0, 0, 0)).toBeNull()
    expect(civilTimestampMs(2026, 6, 30, 24, 0, 0)).toBeNull()
    expect(civilTimestampMs(2026, 6, 30, 0, 60, 0)).toBeNull()
  })

  it('うるう年の 2 月 29 日は年によって変わる', () => {
    expect(civilTimestampMs(2024, 2, 29, 0, 0, 0)).not.toBeNull()
    expect(civilTimestampMs(2026, 2, 29, 0, 0, 0)).toBeNull()
    // 100 年単位・400 年単位の例外も見る。
    expect(daysInMonth(1900, 2)).toBe(28)
    expect(daysInMonth(2000, 2)).toBe(29)
  })

  it('うるう秒の 60 秒だけは許す', () => {
    expect(civilTimestampMs(2026, 6, 30, 18, 19, 60)).not.toBeNull()
    expect(civilTimestampMs(2026, 6, 30, 18, 19, 61)).toBeNull()
  })

  it('現実的でない年は採らない', () => {
    expect(civilTimestampMs(1899, 1, 1, 0, 0, 0)).toBeNull()
    expect(civilTimestampMs(3000, 1, 1, 0, 0, 0)).toBeNull()
  })
})

describe('digitGroups', () => {
  it('数字の並びだけを取り出す', () => {
    expect(digitGroups('IMG_20260630_181932')).toEqual(['20260630', '181932'])
    expect(digitGroups('2026-06-30_18-19-32')).toEqual(['2026', '06', '30', '18', '19', '32'])
    expect(digitGroups('photo')).toEqual([])
  })
})

describe('timestampFromGroups', () => {
  const expected = Date.UTC(2026, 5, 30, 18, 19, 32)

  it('3 つの形を読む', () => {
    expect(timestampFromGroups(['20260630181932'])).toBe(expected)
    expect(timestampFromGroups(['20260630', '181932'])).toBe(expected)
    expect(timestampFromGroups(['2026', '06', '30', '18', '19', '32'])).toBe(expected)
  })

  it('形が合わなければ読まない', () => {
    expect(timestampFromGroups(['1234'])).toBeNull()
    expect(timestampFromGroups([])).toBeNull()
  })

  it('数字として並んでいても暦が不正なら読まない', () => {
    expect(timestampFromGroups(['20261330181932'])).toBeNull()
  })
})

describe('filenameCaptureTime', () => {
  const expected = Date.UTC(2026, 5, 30, 18, 19, 32)

  it('代表的なファイル名から読む', () => {
    expect(filenameCaptureTime('IMG_20260630_181932.jpg')).toBe(expected)
    expect(filenameCaptureTime('20260630_181932.HEIC')).toBe(expected)
    expect(filenameCaptureTime('2026-06-30_18-19-32.png')).toBe(expected)
    expect(filenameCaptureTime('20260630181932.jpg')).toBe(expected)
  })

  it('先頭に無関係な数字があっても後ろから見つける', () => {
    expect(filenameCaptureTime('12_IMG_20260630_181932.jpg')).toBe(expected)
  })

  it('拡張子の数字に引っぱられない', () => {
    expect(filenameCaptureTime('holiday.jpg')).toBeNull()
    expect(filenameCaptureTime('IMG_1234.HEIC')).toBeNull()
  })
})

describe('resolveCaptureTime', () => {
  it('EXIF があればそれを使う', () => {
    const exif = { at: 111, source: 'exif_original' as const }
    expect(resolveCaptureTime(exif, 'IMG_20260630_181932.jpg', 999)).toEqual(exif)
  })

  it('EXIF が無ければファイル名を使う。更新時刻より優先する', () => {
    // 書き出しや転送で EXIF が落ちても、ファイル名の日時は撮影時刻に近い。
    const resolved = resolveCaptureTime(null, 'IMG_20260630_181932.jpg', 999)
    expect(resolved).toEqual({ at: Date.UTC(2026, 5, 30, 18, 19, 32), source: 'filename_inferred' })
  })

  it('どちらも無ければ更新時刻に落ちる', () => {
    expect(resolveCaptureTime(null, 'holiday.jpg', 999))
      .toEqual({ at: 999, source: 'filesystem_mtime' })
  })

  it('更新時刻も無ければ諦める', () => {
    expect(resolveCaptureTime(null, 'holiday.jpg', null)).toBeNull()
  })
})

describe('isWeakSource', () => {
  it('更新時刻と不明だけが弱い根拠', () => {
    // 更新時刻はコピーや展開で簡単に揃うため、連写の根拠にすると
    // 取り込んだぶんが丸ごと候補になってしまう。
    expect(isWeakSource('filesystem_mtime')).toBe(true)
    expect(isWeakSource('unknown')).toBe(true)
    expect(isWeakSource('exif_original')).toBe(false)
    expect(isWeakSource('exif_datetime')).toBe(false)
    expect(isWeakSource('filename_inferred')).toBe(false)
  })
})
