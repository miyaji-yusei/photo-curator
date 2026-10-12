/**
 * 「ピントが合っている所を見る」（U60）の、画面に触らない部分。
 *
 * マスクそのもの（どこがピントか）は core の `focusMask` が決める。ここに置くのは、
 * 入力の縮め方・マスクの色付け・結果のメモリキャッシュだけ。
 */
import type { FocusMask } from '~/lib/core'

/** 重ねる色。アプリのアクセント（primary `#d6ff73`）に合わせる。 */
export const FOCUS_COLOR: readonly [number, number, number] = [214, 255, 115]

/** core に渡す輝度の長辺。表示用画像（縮小）と原本とで分ける。1 枚 50ms 前後に収める。 */
export const FOCUS_INPUT_SIDE = { display: 1024, original: 1536 } as const

export type FocusKind = keyof typeof FOCUS_INPUT_SIDE

/** 長辺が `maxSide` に収まるように縮めた大きさ。拡大はしない。 */
export function fitWithin(width: number, height: number, maxSide: number): { width: number, height: number } {
  if (!(width > 0) || !(height > 0)) return { width: 0, height: 0 }
  const scale = Math.min(1, maxSide / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

/** マスクの強さ（0〜255）をアルファにして、`color` で塗った RGBA を作る。 */
export function maskToRgba(mask: FocusMask, color: readonly [number, number, number] = FOCUS_COLOR): Uint8ClampedArray<ArrayBuffer> {
  const rgba = new Uint8ClampedArray(mask.width * mask.height * 4)
  const count = Math.min(mask.data.length, mask.width * mask.height)
  for (let index = 0; index < count; index += 1) {
    const at = index * 4
    rgba[at] = color[0]
    rgba[at + 1] = color[1]
    rgba[at + 2] = color[2]
    rgba[at + 3] = mask.data[index] ?? 0
  }
  return rgba
}

/** 写真 id ごとのキャッシュの鍵。表示用画像から出したものと原本から出したものは別。 */
export function maskKey(photoId: string, kind: FocusKind): string {
  return `${photoId}\u0001${kind}`
}

/** 入れた順ではなく**使った順**に古いものから捨てる、上限つきのキャッシュ。 */
export interface MaskCache<T> {
  get: (key: string) => T | undefined
  set: (key: string, value: T) => void
  readonly size: number
}

export function createMaskCache<T>(limit: number): MaskCache<T> {
  const entries = new Map<string, T>()
  const cap = Math.max(1, Math.floor(limit))
  return {
    get(key) {
      if (!entries.has(key)) return undefined
      const value = entries.get(key) as T
      entries.delete(key)
      entries.set(key, value)
      return value
    },
    set(key, value) {
      entries.delete(key)
      entries.set(key, value)
      while (entries.size > cap) {
        const oldest = entries.keys().next()
        if (oldest.done) break
        entries.delete(oldest.value)
      }
    },
    get size() {
      return entries.size
    }
  }
}

export interface GrayPixels {
  gray: Uint8Array
  width: number
  height: number
}

export interface FocusMaskLoaderDeps {
  /** 画像を読んで、長辺 `maxSide` 以下の輝度にする。読めなければ null。 */
  read: (src: string, maxSide: number) => Promise<GrayPixels | null>
  /** core の `focusMask`。 */
  compute: (gray: Uint8Array, width: number, height: number) => FocusMask
  /** 計算と計算のあいだに画面へ手を空ける。 */
  yieldToUi?: () => Promise<void>
  cacheLimit?: number
}

export interface FocusMaskRequest {
  photoId: string
  kind: FocusKind
  src: string
  /** 順番が来た時点でもう要らなければ false。要らないものは計算しない。 */
  wanted?: () => boolean
}

export interface FocusMaskLoader {
  load: (request: FocusMaskRequest) => Promise<FocusMask | null>
  cached: (photoId: string, kind: FocusKind) => FocusMask | undefined
  readonly cacheSize: number
}

/**
 * マスクを 1 枚ずつ（同時に 2 枚は計算しない）作り、写真 id ごとにキャッシュする。
 * 失敗（読めない・取り出せない）は null で返し、キャッシュには入れない。
 */
export function createFocusMaskLoader(deps: FocusMaskLoaderDeps): FocusMaskLoader {
  const cache = createMaskCache<FocusMask>(deps.cacheLimit ?? 120)
  const yieldToUi = deps.yieldToUi ?? (() => new Promise<void>(resolve => setTimeout(resolve, 0)))
  let tail: Promise<unknown> = Promise.resolve()

  async function run(request: FocusMaskRequest): Promise<FocusMask | null> {
    const key = maskKey(request.photoId, request.kind)
    const hit = cache.get(key)
    if (hit) return hit
    if (request.wanted && !request.wanted()) return null
    try {
      const pixels = await deps.read(request.src, FOCUS_INPUT_SIDE[request.kind])
      if (!pixels) return null
      await yieldToUi()
      if (request.wanted && !request.wanted()) return null
      const mask = deps.compute(pixels.gray, pixels.width, pixels.height)
      if (!mask.width || !mask.height) return null
      cache.set(key, mask)
      return mask
    } catch {
      return null
    }
  }

  return {
    load(request) {
      const result = tail.then(() => run(request))
      tail = result.then(() => yieldToUi(), () => undefined)
      return result
    },
    cached(photoId, kind) {
      return cache.get(maskKey(photoId, kind))
    },
    get cacheSize() {
      return cache.size
    }
  }
}
