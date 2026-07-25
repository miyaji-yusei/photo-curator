/**
 * 1 枚ぶんの解析。**復号はブラウザに任せる**のが要点。
 *
 * デスクトップの Rust は `image` クレートを jpeg/png/webp だけでビルドしており
 * **HEIC を読めない**。iPad の写真は既定で HEIC なので、そのまま移植すると
 * 利用者自身の写真が読めないアプリになる。Safari は HEIC をネイティブに復号
 * できるので、`createImageBitmap` に任せれば JPEG/HEIC/PNG/WebP が無償で入る。
 *
 * 手順はデスクトップと同じ順序にしてある:
 * 長辺 256px へ縮小 → JPEG に符号化 → **その JPEG から dHash**。
 * 生成直後とキャッシュ読み出しで必ず同じ値になるようにするため。
 */
import type { CaptureTime, TimestampSource } from '~/utils/captureTime'
import { resolveCaptureTime } from '~/utils/captureTime'
import { D_HASH_HEIGHT, D_HASH_WIDTH, dHashFromRgba } from '~/utils/dhash'
import { readExifCapture } from '~/utils/exifReader'

/** デスクトップの `THUMBNAIL_MAX_EDGE` / `THUMBNAIL_QUALITY` と同じ値。 */
export const THUMBNAIL_MAX_EDGE = 256
export const THUMBNAIL_QUALITY = 0.82
/** EXIF は先頭付近にある。全体を読み込まずに済ませる。 */
const EXIF_PREFIX_BYTES = 512 * 1024

export interface AnalyzedPhoto {
  thumbnail: Blob | null
  dHash: string | null
  capturedAt: number | null
  timestampSource: TimestampSource
  /** 解析できなかった理由。成功時は null。 */
  error: string | null
}

interface Surface {
  context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D
  toBlob: (type: string, quality: number) => Promise<Blob | null>
}

/**
 * 描画面を用意する。ワーカーの中では `OffscreenCanvas`、
 * それが無い環境ではメインスレッドの `<canvas>` に落ちる。
 */
function createSurface(width: number, height: number): Surface {
  if (typeof OffscreenCanvas !== 'undefined') {
    const canvas = new OffscreenCanvas(width, height)
    const context = canvas.getContext('2d')
    if (!context) throw new Error('描画面を用意できませんでした。')
    return {
      context,
      toBlob: (type, quality) => canvas.convertToBlob({ type, quality })
    }
  }
  if (typeof document === 'undefined') throw new Error('この環境では画像を処理できません。')
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d')
  if (!context) throw new Error('描画面を用意できませんでした。')
  return {
    context,
    toBlob: (type, quality) => new Promise(resolve => canvas.toBlob(resolve, type, quality))
  }
}

/** 長辺を 256px に収める寸法。元が小さければ引き伸ばさない。 */
export function thumbnailSize(width: number, height: number): { width: number, height: number } {
  const longest = Math.max(width, height)
  if (longest <= THUMBNAIL_MAX_EDGE || longest === 0) return { width, height }
  const ratio = THUMBNAIL_MAX_EDGE / longest
  return {
    width: Math.max(1, Math.round(width * ratio)),
    height: Math.max(1, Math.round(height * ratio))
  }
}

async function readCaptureTime(file: File): Promise<CaptureTime | null> {
  let exif: CaptureTime | null = null
  try {
    const head = new Uint8Array(await file.slice(0, EXIF_PREFIX_BYTES).arrayBuffer())
    exif = readExifCapture(head)
  } catch {
    // EXIF が読めなくても他の手がかりで続ける。
  }
  return resolveCaptureTime(exif, file.name, Number.isFinite(file.lastModified) ? file.lastModified : null)
}

/** 保存したサムネイルの画素から dHash を出す。 */
async function hashThumbnail(thumbnail: Blob): Promise<string> {
  const bitmap = await createImageBitmap(thumbnail)
  try {
    const surface = createSurface(D_HASH_WIDTH, D_HASH_HEIGHT)
    surface.context.imageSmoothingEnabled = true
    surface.context.imageSmoothingQuality = 'high'
    surface.context.drawImage(bitmap, 0, 0, D_HASH_WIDTH, D_HASH_HEIGHT)
    const pixels = surface.context.getImageData(0, 0, D_HASH_WIDTH, D_HASH_HEIGHT)
    return dHashFromRgba(pixels.data)
  } finally {
    bitmap.close()
  }
}

/**
 * 撮影日時・サムネイル・dHash を求める。
 * **例外は投げない**。1 枚失敗しても取り込み全体を止めないため、
 * 理由を `error` に入れて返す。
 */
export async function analyzePhotoFile(file: File): Promise<AnalyzedPhoto> {
  const capture = await readCaptureTime(file)
  const base: AnalyzedPhoto = {
    thumbnail: null,
    dHash: null,
    capturedAt: capture?.at ?? null,
    timestampSource: capture?.source ?? 'unknown',
    error: null
  }

  let bitmap: ImageBitmap | null = null
  try {
    bitmap = await createImageBitmap(file)
    const size = thumbnailSize(bitmap.width, bitmap.height)
    const surface = createSurface(size.width, size.height)
    surface.context.imageSmoothingEnabled = true
    surface.context.imageSmoothingQuality = 'high'
    surface.context.drawImage(bitmap, 0, 0, size.width, size.height)
    const thumbnail = await surface.toBlob('image/jpeg', THUMBNAIL_QUALITY)
    if (!thumbnail) return { ...base, error: 'サムネイルを作れませんでした。' }
    return { ...base, thumbnail, dHash: await hashThumbnail(thumbnail) }
  } catch (cause) {
    // 非対応の形式・壊れたファイル・メモリ不足がここに来る。
    const message = cause instanceof Error ? cause.message : '画像を読み込めませんでした。'
    return { ...base, error: message }
  } finally {
    bitmap?.close()
  }
}
