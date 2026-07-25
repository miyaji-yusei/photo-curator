/**
 * 選別の結果をライブラリ側へ渡す 3 つの出口。
 *
 * **前提**: ブラウザから iPadOS の写真ライブラリは書き換えられない。
 * さらに Apple の写真アプリに星は無く、あるのは「お気に入り(♡)」だけ。
 * だから「星をライブラリに反映する」ことは原理的にできず、
 * 出口はどれも**利用者の操作を経由する**形になる。
 *
 * - 共有シート … 選んだ写真を渡す。写真アプリには**重複として**入る
 * - ZIP        … `star-N/` に分けてファイルアプリへ。あとで PC に渡しやすい
 * - Shortcuts  … 利用者が入れたショートカットに一覧を渡し、
 *                お気に入りやアルバムへ反映してもらう（唯一ライブラリを変えられる道）
 */
import type { ZipEntry } from '~/utils/zip'
import { uniquePath } from '~/utils/zip'

/**
 * 共有シートに一度で渡す枚数の上限。多すぎると iOS 側が黙って失敗するので、
 * これを超えるときは ZIP に誘導する。
 */
export const SHARE_FILE_LIMIT = 30

export type ShareOutcome = 'shared' | 'cancelled' | 'unsupported'

/** この端末がファイルの共有に対応しているか。 */
export function canShareFiles(files: File[]): boolean {
  if (typeof navigator === 'undefined' || !navigator.canShare || !navigator.share) return false
  if (!files.length) return false
  try {
    return navigator.canShare({ files })
  } catch {
    return false
  }
}

/**
 * 共有シートを開く。利用者が「画像を保存」を選べば写真アプリへ、
 * 「ファイルに保存」ならファイルアプリへ入る。
 */
export async function shareFiles(files: File[], title: string): Promise<ShareOutcome> {
  if (!canShareFiles(files)) return 'unsupported'
  try {
    await navigator.share({ files, title })
    return 'shared'
  } catch (cause) {
    // 利用者が閉じただけのときは失敗として扱わない。
    if (cause instanceof DOMException && cause.name === 'AbortError') return 'cancelled'
    return 'unsupported'
  }
}

/** Blob をダウンロードさせる。iPad では「ファイルに保存」の経路になる。 */
export function downloadBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  // すぐ revoke するとダウンロードが始まらない端末があるので少し待つ。
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

export interface ExportCandidate {
  name: string
  rating: number
  blob: Blob
  modifiedAt?: number
}

/**
 * 星ごとのフォルダに振り分けた ZIP の中身を作る。
 * デスクトップの `export_by_rating` と同じ `star-N/` 構造にして、
 * PC 側で受け取ったときに同じ形に見えるようにする。
 */
export function zipEntriesByRating(candidates: ExportCandidate[]): ZipEntry[] {
  const taken = new Set<string>()
  return candidates.map(candidate => ({
    path: uniquePath(taken, `star-${candidate.rating}/${candidate.name}`),
    blob: candidate.blob,
    modifiedAt: candidate.modifiedAt
  }))
}

export interface ShortcutRow {
  name: string
  rating: number
  capturedAt: number | null
}

/**
 * Shortcuts に渡す一覧。1 行 1 枚の TSV で `ファイル名 / 撮影日時 / 星`。
 *
 * **照合の鍵はファイル名。** 写真ライブラリから選んだファイルは、iOS が
 * 元の資産名（`IMG_1234.HEIC` など）を返すので、ショートカットの
 * 「名前が◯◯を含む写真を検索」で辿れる。
 * 撮影日時は人が見て取り違えを確かめるための補助で、EXIF に時差が
 * 書かれていた写真ではその分ずれることがある。
 */
export function buildShortcutPayload(rows: ShortcutRow[]): string {
  return rows
    .map(row => [row.name, formatCapturedAt(row.capturedAt), String(row.rating)].join('\t'))
    .join('\n')
}

/** 保存してある値（UTC 基準のミリ秒）を `YYYY-MM-DD HH:MM:SS` に戻す。 */
export function formatCapturedAt(capturedAt: number | null): string {
  if (capturedAt === null || !Number.isFinite(capturedAt)) return ''
  const pad = (value: number) => String(value).padStart(2, '0')
  const date = new Date(capturedAt)
  return [
    `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`,
    `${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}`
  ].join(' ')
}

/**
 * ショートカットを起動する。一覧はクリップボード経由で渡す。
 * URL に載せると件数が増えたときに長さの上限に当たるため。
 */
export async function runShortcut(name: string, payload: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(payload)
  } catch {
    // クリップボードが使えないときは起動しない。黙って動かすと
    // 前回の内容で処理されてしまう。
    return false
  }
  window.location.href = `shortcuts://run-shortcut?name=${encodeURIComponent(name)}`
  return true
}
