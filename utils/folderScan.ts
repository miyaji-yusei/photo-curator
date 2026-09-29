/**
 * 出所のフォルダを再帰で走査して、準備する写真の一覧を作る。
 *
 * 除くもの: 名前が `.` で始まるもの（隠しフォルダ・`.photo-curator` など）と動画。
 * PC の走査（`src-tauri` の `filter_entry`）と同じ考え方。
 * `SourceIO` にだけ頼るので、Node 上でテストできる。
 */
import type { SourceIO } from '~/composables/backends/web/sourceIO'
import { joinPath } from '~/composables/backends/web/sourceIO'

const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'm4v', 'avi', 'mts', 'm2ts', '3gp', 'mkv'])

export function isVideoName(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return VIDEO_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
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
  io: SourceIO, options: { isCancelled?: () => boolean, onFound?: (count: number) => void } = {}
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
      if (isVideoName(entry.name)) continue
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
  return found
}
