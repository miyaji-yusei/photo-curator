import { describe, expect, it } from 'vitest'
import { findTiffStart, parseExifDateTime, parseOffsetMinutes, readExifCapture } from '~/utils/exifReader'

const ASCII = 2
const LONG = 4

/** ASCII 文字列を末尾 NUL 付きのバイト列にする。 */
const ascii = (text: string) => [...text].map(char => char.charCodeAt(0)).concat(0)

interface Entry { tag: number, type: number, value: number[] | number }

/**
 * 最小構成の TIFF ブロックを組む。値が 4 バイトを超えるものは
 * IFD の後ろに置き、オフセットで参照させる（EXIF の実際の形）。
 */
function buildTiff(ifd0: Entry[], exifIfd: Entry[] = [], little = true): Uint8Array {
  const header = 8
  // IFD0 → Exif IFD → ヒープ の順に置く。
  const ifd0Size = 2 + ifd0.length * 12 + 4
  const exifAt = header + ifd0Size
  const exifSize = 2 + exifIfd.length * 12 + 4
  let heapAt = exifAt + (exifIfd.length ? exifSize : 0)

  const bytes: number[] = []
  const heap: number[] = []
  const u16 = (value: number) => little ? [value & 0xff, value >> 8] : [value >> 8, value & 0xff]
  const u32 = (value: number) => little
    ? [value & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff, (value >> 24) & 0xff]
    : [(value >> 24) & 0xff, (value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff]

  bytes.push(...(little ? [0x49, 0x49, 0x2a, 0x00] : [0x4d, 0x4d, 0x00, 0x2a]))
  bytes.push(...u32(header))

  const writeIfd = (entries: Entry[]) => {
    bytes.push(...u16(entries.length))
    for (const entry of entries) {
      bytes.push(...u16(entry.tag), ...u16(entry.type))
      if (typeof entry.value === 'number') {
        bytes.push(...u32(1), ...u32(entry.value))
        continue
      }
      bytes.push(...u32(entry.value.length))
      if (entry.value.length <= 4) {
        const padded = [...entry.value, 0, 0, 0, 0].slice(0, 4)
        bytes.push(...padded)
      } else {
        bytes.push(...u32(heapAt + heap.length))
        heap.push(...entry.value)
      }
    }
    bytes.push(...u32(0)) // 次の IFD は無い
  }

  // Exif IFD ポインタは実際の位置に差し替える。
  writeIfd(ifd0.map(entry =>
    entry.tag === 0x8769 ? { ...entry, value: exifAt } : entry
  ))
  if (exifIfd.length) writeIfd(exifIfd)
  bytes.push(...heap)
  return new Uint8Array(bytes)
}

/** TIFF を JPEG の APP1 セグメントに包む。 */
function wrapJpeg(tiff: Uint8Array, extraSegments = true): Uint8Array {
  const bytes: number[] = [0xff, 0xd8]
  if (extraSegments) {
    // APP0(JFIF) を前に置き、セグメントを正しく飛ばせているか確かめる。
    const jfif = [0x4a, 0x46, 0x49, 0x46, 0x00, 1, 1, 0, 0, 1, 0, 1, 0, 0]
    bytes.push(0xff, 0xe0, ((jfif.length + 2) >> 8) & 0xff, (jfif.length + 2) & 0xff, ...jfif)
  }
  const payload = [0x45, 0x78, 0x69, 0x66, 0x00, 0x00, ...tiff]
  bytes.push(0xff, 0xe1, ((payload.length + 2) >> 8) & 0xff, (payload.length + 2) & 0xff, ...payload)
  bytes.push(0xff, 0xda, 0x00, 0x02, 0x11, 0x22) // SOS と画素データのつもり
  bytes.push(0xff, 0xd9)
  return new Uint8Array(bytes)
}

/** HEIC のように、box の中に EXIF が埋まっている形を模す。 */
function wrapIsobmff(tiff: Uint8Array): Uint8Array {
  const ftyp = [0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70, 0x68, 0x65, 0x69, 0x63, 0, 0, 0, 0, 0, 0, 0, 0]
  return new Uint8Array([...ftyp, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00, ...tiff])
}

describe('parseExifDateTime', () => {
  it('規格どおりの形を読む', () => {
    expect(parseExifDateTime('2026:06:30 18:19:32')).toBe(Date.UTC(2026, 5, 30, 18, 19, 32))
  })

  it('ハイフン区切りの機材も読む', () => {
    expect(parseExifDateTime('2026-06-30 18:19:32')).toBe(Date.UTC(2026, 5, 30, 18, 19, 32))
  })

  it('オフセットがあれば UTC に直す', () => {
    // +09:00 の 18:19 は UTC の 09:19。
    expect(parseExifDateTime('2026:06:30 18:19:32', 540))
      .toBe(Date.UTC(2026, 5, 30, 9, 19, 32))
    expect(parseExifDateTime('2026:06:30 18:19:32', -300))
      .toBe(Date.UTC(2026, 5, 30, 23, 19, 32))
  })

  it('暦として不正な値は捨てる', () => {
    expect(parseExifDateTime('2026:13:30 18:19:32')).toBeNull()
    expect(parseExifDateTime('0000:00:00 00:00:00')).toBeNull()
    expect(parseExifDateTime('')).toBeNull()
  })
})

describe('parseOffsetMinutes', () => {
  it('符号付きの時差を分にする', () => {
    expect(parseOffsetMinutes('+09:00')).toBe(540)
    expect(parseOffsetMinutes('-05:30')).toBe(-330)
    expect(parseOffsetMinutes('+00:00')).toBe(0)
  })

  it('読めない形は null', () => {
    expect(parseOffsetMinutes('09:00')).toBeNull()
    expect(parseOffsetMinutes('+9:00')).toBeNull()
    expect(parseOffsetMinutes('')).toBeNull()
  })
})

describe('findTiffStart', () => {
  it('JPEG は APP1 を辿って見つける', () => {
    const jpeg = wrapJpeg(buildTiff([{ tag: 0x0132, type: ASCII, value: ascii('2026:06:30 18:19:32') }]))
    const start = findTiffStart(jpeg)
    expect(start).not.toBeNull()
    // 見つけた位置は TIFF ヘッダそのもの。
    expect(jpeg[start!]).toBe(0x49)
  })

  it('box の中に埋まっていても見つける（HEIC 相当）', () => {
    const heic = wrapIsobmff(buildTiff([{ tag: 0x0132, type: ASCII, value: ascii('2026:06:30 18:19:32') }]))
    expect(findTiffStart(heic)).not.toBeNull()
  })

  it('EXIF が無ければ null', () => {
    expect(findTiffStart(new Uint8Array([0xff, 0xd8, 0xff, 0xd9]))).toBeNull()
    expect(findTiffStart(new Uint8Array(0))).toBeNull()
  })

  it('Exif の署名だけあって TIFF ヘッダが続かなければ拾わない', () => {
    // 画素データが偶然一致しても誤検出しないことの確認。
    const fake = new Uint8Array([0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04])
    expect(findTiffStart(fake)).toBeNull()
  })
})

describe('readExifCapture', () => {
  const original = ascii('2026:06:30 18:19:32')
  const dateTime = ascii('2020:01:02 03:04:05')

  it('DateTimeOriginal を最優先で読む', () => {
    const tiff = buildTiff(
      [{ tag: 0x0132, type: ASCII, value: dateTime }, { tag: 0x8769, type: LONG, value: 0 }],
      [{ tag: 0x9003, type: ASCII, value: original }]
    )
    expect(readExifCapture(wrapJpeg(tiff)))
      .toEqual({ at: Date.UTC(2026, 5, 30, 18, 19, 32), source: 'exif_original' })
  })

  it('DateTimeOriginal が無ければ DateTime に落ちる', () => {
    const tiff = buildTiff([{ tag: 0x0132, type: ASCII, value: dateTime }])
    expect(readExifCapture(wrapJpeg(tiff)))
      .toEqual({ at: Date.UTC(2020, 0, 2, 3, 4, 5), source: 'exif_datetime' })
  })

  it('OffsetTimeOriginal があれば UTC に直す', () => {
    const tiff = buildTiff(
      [{ tag: 0x8769, type: LONG, value: 0 }],
      [
        { tag: 0x9003, type: ASCII, value: original },
        { tag: 0x9011, type: ASCII, value: ascii('+09:00') }
      ]
    )
    expect(readExifCapture(wrapJpeg(tiff))?.at).toBe(Date.UTC(2026, 5, 30, 9, 19, 32))
  })

  it('オフセットが壊れていても日時そのものは使う', () => {
    const tiff = buildTiff(
      [{ tag: 0x8769, type: LONG, value: 0 }],
      [
        { tag: 0x9003, type: ASCII, value: original },
        { tag: 0x9011, type: ASCII, value: ascii('bogus') }
      ]
    )
    expect(readExifCapture(wrapJpeg(tiff))?.at).toBe(Date.UTC(2026, 5, 30, 18, 19, 32))
  })

  it('ビッグエンディアン（MM）も読む', () => {
    const tiff = buildTiff([{ tag: 0x0132, type: ASCII, value: dateTime }], [], false)
    expect(readExifCapture(wrapJpeg(tiff))?.at).toBe(Date.UTC(2020, 0, 2, 3, 4, 5))
  })

  it('HEIC 相当の埋め込みからも読む', () => {
    const tiff = buildTiff(
      [{ tag: 0x8769, type: LONG, value: 0 }],
      [{ tag: 0x9003, type: ASCII, value: original }]
    )
    expect(readExifCapture(wrapIsobmff(tiff))?.source).toBe('exif_original')
  })

  it('壊れたバイト列でも例外を投げず null を返す', () => {
    // ここで落ちると取り込み全体が止まる。
    expect(readExifCapture(new Uint8Array([0xff, 0xd8, 0xff, 0xe1, 0x00, 0x08, 0x45, 0x78]))).toBeNull()
    expect(readExifCapture(new Uint8Array(0))).toBeNull()
    const truncated = wrapJpeg(buildTiff([{ tag: 0x0132, type: ASCII, value: ascii('2026:06:30 18:19:32') }]))
    expect(readExifCapture(truncated.slice(0, 12))).toBeNull()
  })
})
