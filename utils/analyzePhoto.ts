/**
 * 1 枚ぶんの解析。**復号はブラウザに任せる**のが要点。
 *
 * デスクトップの Rust は `image` クレートを jpeg/png/webp だけでビルドしており
 * **HEIC を読めない**。iPad の写真は既定で HEIC なので、そのまま移植すると
 * 利用者自身の写真が読めないアプリになる。Safari は HEIC をネイティブに復号
 * できるので、`createImageBitmap` に任せれば JPEG/HEIC/PNG/WebP が無償で入る。
 *
 * 手順はデスクトップと同じ順序にしてある:
 * 長辺 256px へ縮小 → JPEG に符号化 → **その JPEG からハッシュ値**。
 * 生成直後とキャッシュ読み出しで必ず同じ値になるようにするため。
 * ハッシュ値は、保存したサムネイルをそのまま輝度にして core の `dHashFromGray` に渡す
 * （9×8 への縮小も core がする。設計 04 章「縮小も core で」）。
 */
import type { CaptureTime, TimestampSource } from '~/utils/captureTime'
import { resolveCaptureTime } from '~/utils/captureTime'
import { dHashFromGray, init as initCore } from '~/lib/core'
import { lumaFromRgba } from '~/utils/dhash'
import { readExifCapture } from '~/utils/exifReader'

/** デスクトップの `THUMBNAIL_MAX_EDGE` / `THUMBNAIL_QUALITY` と同じ値。 */
export const THUMBNAIL_MAX_EDGE = 256
export const THUMBNAIL_QUALITY = 0.82
/**
 * 選別画面に出す表示用の長辺。デスクトップの DISPLAY_EDGE_DEFAULT と同じ値。
 * サムネイル(256px)では良し悪しを判断できず、原本はリロードで失われるので、
 * **これを保存しておくことでリロード後も選別の見えが落ちない。**
 */
export const DISPLAY_EDGE_DEFAULT = 1024
/** EXIF は先頭付近にある。全体を読み込まずに済ませる。 */
const EXIF_PREFIX_BYTES = 512 * 1024

export interface AnalyzedPhoto {
  thumbnail: Blob | null
  /** 選別画面用。原本が無くなっても、これがあれば判断できる。 */
  display: Blob | null
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

/** 長辺を指定の大きさに収める寸法。元が小さければ引き伸ばさない。 */
export function fitLongEdge(width: number, height: number, edge: number): { width: number, height: number } {
  const longest = Math.max(width, height)
  if (longest <= edge || longest === 0) return { width, height }
  const ratio = edge / longest
  return {
    width: Math.max(1, Math.round(width * ratio)),
    height: Math.max(1, Math.round(height * ratio))
  }
}

export const thumbnailSize = (width: number, height: number) =>
  fitLongEdge(width, height, THUMBNAIL_MAX_EDGE)

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

/**
 * 保存したサムネイルの画素からハッシュ値を出す。**縮小はしない**（core がする）。
 * 作れないとき（小さすぎる画像）は null。
 */
export async function hashThumbnail(thumbnail: Blob): Promise<string | null> {
  const bitmap = await createImageBitmap(thumbnail)
  try {
    const { width, height } = bitmap
    const surface = createSurface(width, height)
    surface.context.drawImage(bitmap, 0, 0)
    const pixels = surface.context.getImageData(0, 0, width, height)
    // ワーカーの中でも wasm を読む（2 回目以降は同じ Promise）。
    await initCore()
    return dHashFromGray(lumaFromRgba(pixels.data, width * height), width, height)
  } finally {
    bitmap.close()
  }
}

/**
 * 撮影日時・サムネイル・ハッシュ値を求める。
 * **例外は投げない**。1 枚失敗しても取り込み全体を止めないため、
 * 理由を `error` に入れて返す。
 */
export async function analyzePhotoFile(
  file: File,
  displayEdge: number = DISPLAY_EDGE_DEFAULT
): Promise<AnalyzedPhoto> {
  const capture = await readCaptureTime(file)
  const base: AnalyzedPhoto = {
    thumbnail: null,
    display: null,
    dHash: null,
    capturedAt: capture?.at ?? null,
    timestampSource: capture?.source ?? 'unknown',
    error: null
  }

  let bitmap: ImageBitmap | null = null
  try {
    // **`imageOrientation` を明示する。** 既定値は仕様の改訂で `none` から
    // `from-image` に変わっており、端末によってどちらが効くか分からない。
    // ここを取り違えると、一覧のサムネイルだけが横倒しになる（原本を直接
    // `<img>` に渡す選別画面はブラウザが自動で正立させるため）。
    // デスクトップの `apply_orientation` と同じ結果になる。
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' })
    const size = thumbnailSize(bitmap.width, bitmap.height)
    const surface = createSurface(size.width, size.height)
    surface.context.imageSmoothingEnabled = true
    surface.context.imageSmoothingQuality = 'high'
    surface.context.drawImage(bitmap, 0, 0, size.width, size.height)
    const thumbnail = await surface.toBlob('image/jpeg', THUMBNAIL_QUALITY)
    if (!thumbnail) return { ...base, error: 'サムネイルを作れませんでした。' }
    // 表示用も同じ復号から作る。**原本をもう一度読まない。**
    // iPad では原本がリロードで失われるので、ここで作らないと二度と作れない。
    const displaySize = fitLongEdge(bitmap.width, bitmap.height, displayEdge)
    const displaySurface = createSurface(displaySize.width, displaySize.height)
    displaySurface.context.imageSmoothingEnabled = true
    displaySurface.context.imageSmoothingQuality = 'high'
    displaySurface.context.drawImage(bitmap, 0, 0, displaySize.width, displaySize.height)
    const display = await displaySurface.toBlob('image/jpeg', THUMBNAIL_QUALITY)
    const dHash = await hashThumbnail(thumbnail)
    // ハッシュ値だけ作れなかった写真も、サムネイルと表示用はあるので選別には使える。
    // 理由を残しておかないと「解析待ち」に数え続けてしまう。
    return { ...base, thumbnail, display, dHash, error: dHash ? null : 'ハッシュ値を作れませんでした（画像が小さすぎます）。' }
  } catch (cause) {
    // 非対応の形式・壊れたファイル・メモリ不足がここに来る。
    const message = cause instanceof Error ? cause.message : '画像を読み込めませんでした。'
    return { ...base, error: message }
  } finally {
    bitmap?.close()
  }
}
