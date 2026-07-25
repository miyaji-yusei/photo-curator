/**
 * EXIF から撮影時刻を読む。**バイト列だけを見る純粋な処理**なので、
 * ブラウザでもテストでも同じように動く。
 *
 * デスクトップ（Rust の `exif_capture_time`）と同じ規則:
 * - `DateTimeOriginal` を優先し、無ければ `DateTime`
 * - `OffsetTimeOriginal`（無ければ `OffsetTime`）があれば UTC へ直す
 * - 壊れた値は捨てる（暦として不正なら採らない。検証は `civilTimestampMs` 側）
 *
 * JPEG は APP1 セグメントを規格どおりに辿る。HEIC/HEIF は EXIF が ISOBMFF の
 * アイテムに入っており、box を全部解くと大掛かりになるので、
 * **`Exif\0\0` と正しい TIFF ヘッダが連続する位置を探す**方法を採る。
 * TIFF ヘッダ（`II*\0` / `MM\0*`）まで一致を求めるので、
 * 画素データが偶然引っかかる余地はほぼ無い。
 */
import type { CaptureTime, TimestampSource } from '~/utils/captureTime'
import { civilTimestampMs } from '~/utils/captureTime'

const EXIF_MARKER = [0x45, 0x78, 0x69, 0x66, 0x00, 0x00] // "Exif\0\0"

const TAG_DATE_TIME = 0x0132
const TAG_EXIF_IFD_POINTER = 0x8769
const TAG_DATE_TIME_ORIGINAL = 0x9003
const TAG_OFFSET_TIME = 0x9010
const TAG_OFFSET_TIME_ORIGINAL = 0x9011

/** そこから TIFF ヘッダが始まっているか。 */
function looksLikeTiff(bytes: Uint8Array, at: number): boolean {
  if (at + 4 > bytes.length) return false
  const [a, b, c, d] = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
  if (a === 0x49 && b === 0x49) return c === 0x2a && d === 0x00
  if (a === 0x4d && b === 0x4d) return c === 0x00 && d === 0x2a
  return false
}

function matchesAt(bytes: Uint8Array, at: number, pattern: number[]): boolean {
  if (at + pattern.length > bytes.length) return false
  return pattern.every((value, index) => bytes[at + index] === value)
}

/**
 * TIFF ブロックの開始位置を探す。見つからなければ null。
 * JPEG はセグメントを辿り、それ以外（HEIC など）は署名を走査する。
 */
export function findTiffStart(bytes: Uint8Array): number | null {
  if (bytes.length >= 4 && bytes[0] === 0xff && bytes[1] === 0xd8) {
    let at = 2
    while (at + 4 <= bytes.length && bytes[at] === 0xff) {
      const marker = bytes[at + 1]!
      // 単独マーカー（長さを持たない）はそのまま次へ。
      if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd9)) {
        at += 2
        continue
      }
      // 画素データに入ったら EXIF はもう無い。
      if (marker === 0xda) break
      const length = (bytes[at + 2]! << 8) | bytes[at + 3]!
      if (length < 2) break
      if (marker === 0xe1 && matchesAt(bytes, at + 4, EXIF_MARKER)) {
        const start = at + 4 + EXIF_MARKER.length
        if (looksLikeTiff(bytes, start)) return start
      }
      at += 2 + length
    }
    // APP1 が見つからない JPEG はここで諦めず、下の走査にも掛ける。
  }
  for (let at = 0; at + EXIF_MARKER.length + 4 <= bytes.length; at += 1) {
    if (bytes[at] !== EXIF_MARKER[0]) continue
    if (!matchesAt(bytes, at, EXIF_MARKER)) continue
    const start = at + EXIF_MARKER.length
    if (looksLikeTiff(bytes, start)) return start
  }
  return null
}

/** `+09:00` 形式を分に直す。読めなければ null。 */
export function parseOffsetMinutes(value: string): number | null {
  const matched = /^([+-])(\d{2}):(\d{2})$/.exec(value.trim())
  if (!matched) return null
  const hours = Number(matched[2])
  const minutes = Number(matched[3])
  if (hours > 23 || minutes > 59) return null
  const total = hours * 60 + minutes
  return matched[1] === '-' ? -total : total
}

/**
 * `YYYY:MM:DD HH:MM:SS` を UTC のミリ秒にする。
 * 区切りは規格どおりのコロンだが、ハイフンで書く機材もあるため両方許す。
 */
export function parseExifDateTime(value: string, offsetMinutes: number | null = null): number | null {
  const matched = /^(\d{4})[:-](\d{2})[:-](\d{2})[ T](\d{2}):(\d{2}):(\d{2})/.exec(value.trim())
  if (!matched) return null
  const at = civilTimestampMs(
    Number(matched[1]), Number(matched[2]), Number(matched[3]),
    Number(matched[4]), Number(matched[5]), Number(matched[6])
  )
  if (at === null) return null
  return offsetMinutes === null ? at : at - offsetMinutes * 60_000
}

interface Reader {
  view: DataView
  little: boolean
  /** TIFF ブロックの先頭。IFD のオフセットはここが基準。 */
  base: number
  limit: number
}

function u16(reader: Reader, at: number) { return reader.view.getUint16(at, reader.little) }
function u32(reader: Reader, at: number) { return reader.view.getUint32(at, reader.little) }

/** ASCII 型のフィールドを読む。4 バイト以下は値の欄に直接入っている。 */
function readAscii(reader: Reader, entryAt: number): string | null {
  const type = u16(reader, entryAt + 2)
  if (type !== 2) return null
  const count = u32(reader, entryAt + 4)
  if (count === 0 || count > 64) return null
  let at = entryAt + 8
  if (count > 4) {
    at = reader.base + u32(reader, entryAt + 8)
    if (at < 0 || at + count > reader.limit) return null
  }
  let text = ''
  for (let index = 0; index < count; index += 1) {
    const code = reader.view.getUint8(at + index)
    if (code === 0) break
    text += String.fromCharCode(code)
  }
  return text
}

/** IFD の全エントリを {tag: エントリ位置} で返す。 */
function readIfd(reader: Reader, ifdAt: number): Map<number, number> {
  const entries = new Map<number, number>()
  if (ifdAt + 2 > reader.limit) return entries
  const count = u16(reader, ifdAt)
  // 壊れたファイルで巨大な件数を読まないよう上限を切る。
  if (count > 512) return entries
  for (let index = 0; index < count; index += 1) {
    const entryAt = ifdAt + 2 + index * 12
    if (entryAt + 12 > reader.limit) break
    entries.set(u16(reader, entryAt), entryAt)
  }
  return entries
}

/**
 * 撮影時刻を EXIF から読む。読めなければ null（呼び出し側が
 * ファイル名や更新時刻へ落とす）。
 */
export function readExifCapture(bytes: Uint8Array): CaptureTime | null {
  try {
    const base = findTiffStart(bytes)
    if (base === null) return null
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    const little = bytes[base] === 0x49
    const reader: Reader = { view, little, base, limit: bytes.byteLength }

    const ifd0At = base + u32(reader, base + 4)
    if (ifd0At < base || ifd0At + 2 > reader.limit) return null
    const ifd0 = readIfd(reader, ifd0At)

    let exifIfd = new Map<number, number>()
    const pointerEntry = ifd0.get(TAG_EXIF_IFD_POINTER)
    if (pointerEntry !== undefined) {
      const exifAt = base + u32(reader, pointerEntry + 8)
      if (exifAt >= base && exifAt + 2 <= reader.limit) exifIfd = readIfd(reader, exifAt)
    }

    const ascii = (ifd: Map<number, number>, tag: number) => {
      const entryAt = ifd.get(tag)
      return entryAt === undefined ? null : readAscii(reader, entryAt)
    }

    const rawOffset = ascii(exifIfd, TAG_OFFSET_TIME_ORIGINAL) ?? ascii(exifIfd, TAG_OFFSET_TIME)
    // オフセットが壊れていても日時そのものは使う。
    const offsetMinutes = rawOffset ? parseOffsetMinutes(rawOffset) : null

    const candidates: [string | null, TimestampSource][] = [
      [ascii(exifIfd, TAG_DATE_TIME_ORIGINAL), 'exif_original'],
      [ascii(ifd0, TAG_DATE_TIME), 'exif_datetime']
    ]
    for (const [raw, source] of candidates) {
      if (!raw) continue
      const at = parseExifDateTime(raw, offsetMinutes)
      if (at !== null) return { at, source }
    }
    return null
  } catch {
    // 壊れた EXIF で落とさない。読めなければ他の経路に任せる。
    return null
  }
}
