/**
 * 「ピントが合っている所を見る」（U60）の、画面に触らない部分。
 *
 * どこがくっきりしているか（四角）は core の `focusRegions` が決める。ここに置くのは、
 * 入力の縮め方・四角のかぎ括弧の形・札の文言・結果のメモリキャッシュだけ。
 * （名前は初期の「ピーキング」のまま。いまは輪郭ではなく領域の枠を出す。）
 */
import type { FocusRegion, FocusRegions } from '~/lib/core'

/** 枠の色。アプリのアクセント（primary `#d6ff73`）に合わせる。 */
export const FOCUS_COLOR = '#d6ff73'
/** 白い写真でも見えるよう、枠の外側に敷く細い暗い縁取り。 */
export const FOCUS_OUTLINE_COLOR = 'rgba(0, 0, 0, 0.7)'

/** 「くっきりした所なし」の札。選別のタイルなど狭い所では短いほう。Android と同じ文言。 */
export const FOCUS_NONE_LABEL = 'くっきりした所が見つかりません'
export const FOCUS_NONE_LABEL_COMPACT = 'ピント甘め？'

/** core に渡す輝度の長辺。表示用画像（縮小）と原本とで分ける。1 枚 50ms 前後に収める。 */
export const FOCUS_INPUT_SIDE = { display: 1024, original: 1536 } as const

export type FocusKind = keyof typeof FOCUS_INPUT_SIDE

/** 長辺が `maxSide` に収まるように縮めた大きさ。拡大はしない。 */
export function fitWithin(width: number, height: number, maxSide: number): { width: number, height: number } {
  if (!(width > 0) || !(height > 0)) return { width: 0, height: 0 }
  const scale = Math.min(1, maxSide / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

/** 解析した画像の大きさ（四角を画面に置くときの縦横比）と、見つかった四角。 */
export interface FocusResult {
  width: number
  height: number
  regions: FocusRegion[]
}

/** かぎ括弧の腕の長さ（四角の短辺に対する割合）。 */
const BRACKET_ARM = 0.3

/**
 * 四角の**四隅だけ**を描くかぎ括弧の SVG パス。座標は解析した画像の画素（`width` x `height`）。
 * 四角が小さくても腕が重ならないよう、腕は短辺の 30 %。
 */
export function bracketPath(region: FocusRegion, width: number, height: number): string {
  const x0 = region.x * width
  const y0 = region.y * height
  const x1 = (region.x + region.w) * width
  const y1 = (region.y + region.h) * height
  const arm = Math.min(x1 - x0, y1 - y0) * BRACKET_ARM
  const f = (value: number) => Number(value.toFixed(2))
  return [
    `M${f(x0)} ${f(y0 + arm)}V${f(y0)}H${f(x0 + arm)}`,
    `M${f(x1 - arm)} ${f(y0)}H${f(x1)}V${f(y0 + arm)}`,
    `M${f(x1)} ${f(y1 - arm)}V${f(y1)}H${f(x1 - arm)}`,
    `M${f(x0 + arm)} ${f(y1)}H${f(x0)}V${f(y1 - arm)}`
  ].join('')
}

/** 順位ごとの線の太さ（画面の px）と濃さ。いちばん強い 1 個は太く、2・3 個目は細く薄く。 */
export function bracketStyle(rank: number): { width: number, opacity: number } {
  return rank === 0 ? { width: 3, opacity: 1 } : { width: 1.5, opacity: 0.6 }
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

export interface FocusLoaderDeps {
  /** 画像を読んで、長辺 `maxSide` 以下の輝度にする。読めなければ null。 */
  read: (src: string, maxSide: number) => Promise<GrayPixels | null>
  /** core の `focusRegions`。 */
  compute: (gray: Uint8Array, width: number, height: number) => FocusRegions
  /** 計算と計算のあいだに画面へ手を空ける。 */
  yieldToUi?: () => Promise<void>
  cacheLimit?: number
}

export interface FocusRequest {
  photoId: string
  kind: FocusKind
  src: string
  /** 順番が来た時点でもう要らなければ false。要らないものは計算しない。 */
  wanted?: () => boolean
}

export interface FocusLoader {
  /** 失敗（読めない・取り出せない）は null。**四角が 0 個なのは失敗ではない**（空の `regions` で返す）。 */
  load: (request: FocusRequest) => Promise<FocusResult | null>
  cached: (photoId: string, kind: FocusKind) => FocusResult | undefined
  readonly cacheSize: number
}

/**
 * 四角を 1 枚ずつ（同時に 2 枚は計算しない）求め、写真 id ごとにキャッシュする。
 * 失敗（読めない・取り出せない）は null で返し、キャッシュには入れない。
 */
export function createFocusLoader(deps: FocusLoaderDeps): FocusLoader {
  const cache = createMaskCache<FocusResult>(deps.cacheLimit ?? 120)
  const yieldToUi = deps.yieldToUi ?? (() => new Promise<void>(resolve => setTimeout(resolve, 0)))
  let tail: Promise<unknown> = Promise.resolve()

  async function run(request: FocusRequest): Promise<FocusResult | null> {
    const key = maskKey(request.photoId, request.kind)
    const hit = cache.get(key)
    if (hit) return hit
    if (request.wanted && !request.wanted()) return null
    try {
      const pixels = await deps.read(request.src, FOCUS_INPUT_SIDE[request.kind])
      if (!pixels || !pixels.width || !pixels.height) return null
      await yieldToUi()
      if (request.wanted && !request.wanted()) return null
      const found = deps.compute(pixels.gray, pixels.width, pixels.height)
      const result: FocusResult = { width: pixels.width, height: pixels.height, regions: found.regions }
      cache.set(key, result)
      return result
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
