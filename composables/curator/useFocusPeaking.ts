import { computed, ref } from 'vue'
import * as core from '~/lib/core'
import { lumaFromRgba } from '~/utils/dhash'
import { createFocusMaskLoader, fitWithin, type GrayPixels } from '~/utils/focusPeaking'

/**
 * ピントの位置の重ね表示（U60）。オン・オフは**アプリを開いているあいだだけ**覚える
 * （設定の同期には入れない）。選別画面と拡大画面で同じ状態を見る。
 */
const focusOn = ref(false)

export function useFocusPeaking() {
  return {
    focusOn: computed(() => focusOn.value),
    toggleFocus() { focusOn.value = !focusOn.value }
  }
}

/**
 * 画像を読んで輝度にする。`crossOrigin` を付けて読むので、Tauri の asset プロトコル
 * （応答に Access-Control-Allow-Origin を付ける）でも canvas は汚れない。
 * 汚れて読めなければ（SecurityError）null＝ピントの印は出ない。
 */
async function readGray(src: string, maxSide: number): Promise<GrayPixels | null> {
  if (typeof document === 'undefined') return null
  await core.init()
  const image = new Image()
  image.crossOrigin = 'anonymous'
  image.decoding = 'async'
  image.src = src
  await image.decode()
  const size = fitWithin(image.naturalWidth, image.naturalHeight, maxSide)
  if (!size.width) return null
  const canvas = document.createElement('canvas')
  canvas.width = size.width
  canvas.height = size.height
  const context = canvas.getContext('2d', { willReadFrequently: true })
  if (!context) return null
  context.drawImage(image, 0, 0, size.width, size.height)
  const rgba = context.getImageData(0, 0, size.width, size.height).data
  return { gray: lumaFromRgba(rgba, size.width * size.height), width: size.width, height: size.height }
}

let loader: ReturnType<typeof createFocusMaskLoader> | null = null
/** アプリで 1 つ。計算は 1 枚ずつ、結果は写真 id ごとにキャッシュ（上限つき）。 */
export function focusMaskLoader() {
  loader ??= createFocusMaskLoader({
    read: readGray,
    compute: (gray, width, height) => core.focusMask(gray, width, height)
  })
  return loader
}
