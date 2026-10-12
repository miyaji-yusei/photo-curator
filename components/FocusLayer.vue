<script setup lang="ts">
import type { FocusMask } from '~/lib/core'
import { useFocusPeaking, focusMaskLoader } from '~/composables/curator/useFocusPeaking'
import { maskToRgba, type FocusKind } from '~/utils/focusPeaking'

/**
 * ピントが合っている所の輪郭に色を重ねる層（U60）。写真と同じ枠に `object-fit: contain` で重ね、
 * 押しても反応しない（pointer-events: none）。オンのときだけ計算する。
 *
 * **別の写真のマスクは出さない**: 出すのは「いま `photoId` の写真」のマスクだけ。
 * 同じ写真で画像が替わったとき（表示用 → 原本）は、新しいのが届くまで前のを残す（位置は同じ）。
 */
const props = defineProps<{
  photoId: string
  /** 画像の URL。空なら何も出さない（まだ読み込み前）。 */
  src: string
  kind: FocusKind
  /** 写真を押している間などに一時的に隠す。 */
  hidden?: boolean
}>()

const { focusOn } = useFocusPeaking()
const current = shallowRef<{ photoId: string, mask: FocusMask } | null>(null)
const canvas = ref<HTMLCanvasElement | null>(null)
const shown = computed(() => {
  const value = current.value
  return focusOn.value && value && value.photoId === props.photoId ? value.mask : null
})

let token = 0
async function refresh() {
  const run = ++token
  if (!focusOn.value || !props.src) return
  const { photoId, kind, src } = props
  const wanted = () => run === token
  const mask = await focusMaskLoader().load({ photoId, kind, src, wanted })
  if (run === token && mask) current.value = { photoId, mask }
}
watch(() => [focusOn.value, props.photoId, props.src, props.kind] as const, refresh, { immediate: true })
onBeforeUnmount(() => { token += 1 })

function draw() {
  const el = canvas.value
  const mask = shown.value
  if (!el || !mask) return
  el.width = mask.width
  el.height = mask.height
  el.getContext('2d')?.putImageData(new ImageData(maskToRgba(mask), mask.width, mask.height), 0, 0)
}
watch(shown, () => nextTick(draw), { flush: 'post' })
onMounted(draw)
</script>

<template>
  <canvas
    v-if="shown"
    v-show="!hidden"
    ref="canvas"
    class="focus-layer"
    aria-hidden="true"
  />
</template>
