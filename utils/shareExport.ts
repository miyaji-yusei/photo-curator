/**
 * 選別の結果をライブラリ側へ渡す 2 つの出口。
 *
 * **前提**: ブラウザから iPadOS の写真ライブラリは書き換えられない。
 * さらに Apple の写真アプリに星は無い。だから「星をライブラリに反映する」ことは
 * 原理的にできず、出口はどちらも**利用者の操作を経由する**形になる。
 *
 * - 共有シート … 選んだ写真を渡す。写真アプリには**重複として**入る
 * - ZIP        … `star-N/` に分けてファイルアプリへ。あとで PC に渡しやすい
 *
 * Shortcuts でアルバムへ入れる経路も試したが、**写真ライブラリをファイル名で
 * 辿る手立てが実機に無く**（相当するアクションが見当たらず、写真アプリの
 * 「検索」はファイル名で検索できない）、成立しないので取り下げた。
 * 星はこのアプリが持ち続ける。
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
