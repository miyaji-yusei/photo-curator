/**
 * 出所のフォルダを再帰で走査して、準備する写真の一覧を作る。
 *
 * 除くもの: 名前が `.` で始まるもの（隠しフォルダ・`.photo-curator` など）、動画、画像・RAW の拡張子でないもの
 * （`.xmp`・`.txt`・`Thumbs.db`・`.AAE` など。B4）。拡張子が無い名前は候補に残す（中身で決める）。
 * PC の走査（`src-tauri` の `filter_entry`・`is_supported`）と同じ考え方。
 * `SourceIO` にだけ頼るので、Node 上でテストできる。組の RAW を除く規則は core（wasm）が持つ（R10）。
 */
import type { SourceIO } from '~/composables/backends/web/sourceIO'
import { joinPath } from '~/composables/backends/web/sourceIO'
import { init as initCore, pairedRawMask } from '~/lib/core'

const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'm4v', 'avi', 'mts', 'm2ts', '3gp', 'mkv'])

export function isVideoName(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return VIDEO_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
}

/**
 * 写真の候補にする拡張子。PC の `IMAGE_EXTENSIONS`（`src-tauri/src/scan.rs`）に揃える。
 * rw2・pef・srw は PC には無いが、Web・Android は候補にする（B8 は今回は揃えない）。
 */
const PHOTO_EXTENSIONS = new Set([
  'jpg', 'jpeg', 'png', 'webp', 'heic', 'heif',
  'cr2', 'cr3', 'nef', 'arw', 'dng', 'raf', 'orf', 'rw2', 'pef', 'srw'
])

/** 写真の候補の名前か。動画は除く。拡張子が無い名前は候補（中身で決める）。PC の `is_supported` と同じ。 */
export function isPhotoName(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return true
  return PHOTO_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
}

export interface ScannedFile {
  /** 出所の根からの相対パス。写真の鍵（`relativePath`）になる。 */
  relativePath: string
  subPath: string
  name: string
  size: number
  mtimeMs: number
}

/** 走査の結果。並びは名前順（同じフォルダなら何度走査しても同じ順にする）。 */
export async function scanFolder(
  io: SourceIO, options: {
    isCancelled?: () => boolean
    onFound?: (count: number) => void
    /** 同名の JPEG と RAW を 1 枚の写真として扱う（プロジェクトの設定。既定 true。U46）。 */
    pairRawJpeg?: boolean
  } = {}
): Promise<ScannedFile[]> {
  const found: ScannedFile[] = []
  const walk = async (subPath: string): Promise<void> => {
    if (options.isCancelled?.()) return
    const entries = (await io.list(subPath)).sort((left, right) => left.name.localeCompare(right.name))
    for (const entry of entries) {
      if (entry.name.startsWith('.')) continue
      if (entry.isDirectory) {
        await walk(joinPath(subPath, entry.name))
        continue
      }
      if (isVideoName(entry.name) || !isPhotoName(entry.name)) continue
      found.push({
        relativePath: joinPath(subPath, entry.name),
        subPath,
        name: entry.name,
        size: entry.size,
        mtimeMs: entry.mtimeMs
      })
    }
    options.onFound?.(found.length)
  }
  await walk('')
  // RAW+JPEG 同時撮影の組の RAW は写真に数えない（U46）。規則は core が持つので、先に読み込んでおく。
  const pairRawJpeg = options.pairRawJpeg ?? true
  if (pairRawJpeg) await initCore()
  return skipPairedRaw(found, pairRawJpeg)
}

/**
 * RAW＋JPEG 同時撮影の「組」の RAW を除く（U46）。同じフォルダ（`subPath`）に、拡張子を除いた
 * 名前が大文字小文字を無視して一致する JPEG（.jpg・.jpeg）がある RAW は、写真に数えない。
 * 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組は除かない。
 * 規則は core の `paired_raw_mask`（R10。PC・Android と同じ 1 か所）。入力の並びは保つ。
 * `enabled` が false なら何も除かない。true のときは core の初期化が済んでいること。
 */
export function skipPairedRaw<T extends { subPath: string, name: string }>(files: T[], enabled = true): T[] {
  if (!enabled) return files
  const mask = pairedRawMask(files.map(file => `${file.subPath}/${file.name}`))
  return files.filter((_, index) => !mask[index])
}
