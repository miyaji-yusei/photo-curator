/**
 * ZIP を組み立てる。**無圧縮（stored）**だけを扱う小さな実装。
 *
 * JPEG や HEIC は既に圧縮済みなので、deflate を掛けても縮まらないうえ
 * 時間がかかる。無圧縮に絞れば、CRC32 とヘッダを書くだけで済み、
 * 圧縮ライブラリへの依存も要らない。
 *
 * ファイルの中身は `Blob` の参照としてだけ持つ。全部をメモリへ展開しないので、
 * 数百枚をまとめても実メモリは 1 枚ぶんの読み込みで済む。
 */

/** ZIP のファイル名は UTF-8 だと宣言するフラグ（bit 11）。日本語名のため。 */
const FLAG_UTF8 = 0x0800
const METHOD_STORED = 0

const crcTable = (() => {
  const table = new Uint32Array(256)
  for (let index = 0; index < 256; index += 1) {
    let value = index
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
    }
    table[index] = value >>> 0
  }
  return table
})()

/** ZIP が要求する CRC-32（IEEE 802.3、反転あり）。 */
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff
  for (let index = 0; index < bytes.length; index += 1) {
    crc = crcTable[(crc ^ bytes[index]!) & 0xff]! ^ (crc >>> 8)
  }
  return (crc ^ 0xffffffff) >>> 0
}

/** DOS 形式の日時。1980 年より前は表現できないので下限で丸める。 */
export function dosDateTime(date: Date): { time: number, date: number } {
  const year = Math.max(1980, date.getFullYear())
  return {
    time: (date.getHours() << 11) | (date.getMinutes() << 5) | Math.floor(date.getSeconds() / 2),
    date: ((year - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate()
  }
}

class Writer {
  private readonly bytes: number[] = []

  u16(value: number) { this.bytes.push(value & 0xff, (value >>> 8) & 0xff) }
  u32(value: number) {
    this.bytes.push(value & 0xff, (value >>> 8) & 0xff, (value >>> 16) & 0xff, (value >>> 24) & 0xff)
  }
  raw(values: Uint8Array) { for (const value of values) this.bytes.push(value) }
  /**
   * `Blob` に渡せるのは `ArrayBuffer` を裏に持つ view だけなので、
   * 実体を確保してから詰める（`SharedArrayBuffer` 由来だと型が合わない）。
   */
  toBytes(): Uint8Array<ArrayBuffer> {
    const view = new Uint8Array(new ArrayBuffer(this.bytes.length))
    view.set(this.bytes)
    return view
  }
  get length() { return this.bytes.length }
}

export interface ZipEntry {
  /** ZIP の中でのパス。`star-5/IMG_0001.jpg` のように区切りは `/`。 */
  path: string
  blob: Blob
  /** 無ければ現在時刻を使う。 */
  modifiedAt?: number
}

interface Prepared {
  nameBytes: Uint8Array
  crc: number
  size: number
  offset: number
  time: number
  date: number
}

function localHeader(entry: Prepared): Uint8Array<ArrayBuffer> {
  const writer = new Writer()
  writer.u32(0x04034b50)
  writer.u16(20)            // 展開に必要なバージョン
  writer.u16(FLAG_UTF8)
  writer.u16(METHOD_STORED)
  writer.u16(entry.time)
  writer.u16(entry.date)
  writer.u32(entry.crc)
  writer.u32(entry.size)    // 無圧縮なので圧縮後も同じ
  writer.u32(entry.size)
  writer.u16(entry.nameBytes.length)
  writer.u16(0)             // extra field は無し
  writer.raw(entry.nameBytes)
  return writer.toBytes()
}

function centralEntry(entry: Prepared): Uint8Array<ArrayBuffer> {
  const writer = new Writer()
  writer.u32(0x02014b50)
  writer.u16(20)            // 作成したバージョン
  writer.u16(20)
  writer.u16(FLAG_UTF8)
  writer.u16(METHOD_STORED)
  writer.u16(entry.time)
  writer.u16(entry.date)
  writer.u32(entry.crc)
  writer.u32(entry.size)
  writer.u32(entry.size)
  writer.u16(entry.nameBytes.length)
  writer.u16(0)             // extra
  writer.u16(0)             // comment
  writer.u16(0)             // 開始ディスク
  writer.u16(0)             // 内部属性
  writer.u32(0)             // 外部属性
  writer.u32(entry.offset)
  writer.raw(entry.nameBytes)
  return writer.toBytes()
}

function endOfCentralDirectory(count: number, size: number, offset: number): Uint8Array<ArrayBuffer> {
  const writer = new Writer()
  writer.u32(0x06054b50)
  writer.u16(0)             // このディスク
  writer.u16(0)             // 中央ディレクトリの開始ディスク
  writer.u16(count)
  writer.u16(count)
  writer.u32(size)
  writer.u32(offset)
  writer.u16(0)             // コメント無し
  return writer.toBytes()
}

/**
 * 無圧縮の ZIP を作る。
 *
 * CRC32 を出すために中身は 1 度読むが、**1 ファイルずつ読んで捨てる**。
 * 出来上がった Blob は元の Blob を参照するだけなので、
 * 全体をメモリに抱えることはない。
 */
export async function createStoredZip(entries: ZipEntry[]): Promise<Blob> {
  const parts: BlobPart[] = []
  const prepared: Prepared[] = []
  const encoder = new TextEncoder()
  let offset = 0

  for (const entry of entries) {
    const bytes = new Uint8Array(await entry.blob.arrayBuffer())
    const stamp = dosDateTime(new Date(entry.modifiedAt ?? Date.now()))
    const record: Prepared = {
      nameBytes: encoder.encode(entry.path),
      crc: crc32(bytes),
      size: bytes.length,
      offset,
      time: stamp.time,
      date: stamp.date
    }
    const header = localHeader(record)
    parts.push(header, entry.blob)
    offset += header.length + record.size
    prepared.push(record)
    // bytes はここで参照が切れる。次の 1 枚ぶんしか同時に抱えない。
  }

  const central = prepared.map(centralEntry)
  const centralSize = central.reduce((total, chunk) => total + chunk.length, 0)
  parts.push(...central, endOfCentralDirectory(prepared.length, centralSize, offset))
  return new Blob(parts, { type: 'application/zip' })
}

/**
 * 同じ名前が来ても上書きしないよう `name (2).jpg` のように連番を付ける。
 * デスクトップの `unique_destination` と同じ考え方。
 */
export function uniquePath(taken: Set<string>, path: string): string {
  if (!taken.has(path)) {
    taken.add(path)
    return path
  }
  const dot = path.lastIndexOf('.')
  const slash = path.lastIndexOf('/')
  const hasExtension = dot > slash + 1
  const stem = hasExtension ? path.slice(0, dot) : path
  const extension = hasExtension ? path.slice(dot) : ''
  for (let index = 2; ; index += 1) {
    const candidate = `${stem} (${index})${extension}`
    if (!taken.has(candidate)) {
      taken.add(candidate)
      return candidate
    }
  }
}
