<script setup lang="ts">
import { useFocusPeaking, focusLoader } from '~/composables/curator/useFocusPeaking'
import {
  bracketPath, bracketStyle, FOCUS_COLOR, FOCUS_NONE_LABEL, FOCUS_NONE_LABEL_COMPACT, FOCUS_OUTLINE_COLOR,
  type FocusKind, type FocusResult
} from '~/utils/focusPeaking'

/**
 * くっきりした所を囲む枠の層（U60）。カメラの AF 枠のように、四角の**四隅だけ**のかぎ括弧を重ねる。
 * 写真と同じ枠に重ね（SVG は `meet` で写真の `object-fit: contain` と同じ置き方）、
 * 押しても反応しない（pointer-events: none）。オンのときだけ計算する。
 * 四角が 0 個のときは写真の下端に札を出す（計算中は何も出さない）。
 *
 * **別の写真の枠は出さない**: 出すのは「いま `photoId` の写真」の結果だけ。
 * 同じ写真で画像が替わったとき（表示用 → 原本）は、新しいのが届くまで前のを残す（位置は同じ）。
 */
const props = defineProps<{
  photoId: string
  /** 画像の URL。空なら何も出さない（まだ読み込み前）。 */
  src: string
  kind: FocusKind
  /** 写真を押している間などに一時的に隠す。 */
  hidden?: boolean
  /** 札を短い文言にする（選別のタイルなど狭い所）。 */
  compact?: boolean
}>()

const { focusOn } = useFocusPeaking()
const current = shallowRef<{ photoId: string, result: FocusResult } | null>(null)
const shown = computed(() => {
  const value = current.value
  return focusOn.value && value && value.photoId === props.photoId ? value.result : null
})

let token = 0
async function refresh() {
  const run = ++token
  if (!focusOn.value || !props.src) return
  const { photoId, kind, src } = props
  const wanted = () => run === token
  const result = await focusLoader().load({ photoId, kind, src, wanted })
  if (run === token && result) current.value = { photoId, result }
}
watch(() => [focusOn.value, props.photoId, props.src, props.kind] as const, refresh, { immediate: true })
onBeforeUnmount(() => { token += 1 })

const brackets = computed(() => {
  const result = shown.value
  if (!result) return []
  return result.regions.map((region, rank) => ({
    d: bracketPath(region, result.width, result.height),
    ...bracketStyle(rank)
  }))
})
</script>

<template>
  <div v-if="shown" v-show="!hidden" class="focus-layer" aria-hidden="true">
    <svg
      v-if="shown.regions.length"
      class="focus-layer__svg"
      :viewBox="`0 0 ${shown.width} ${shown.height}`"
      preserveAspectRatio="xMidYMid meet"
    >
      <!-- 暗い縁取りを先に太く敷いてから、黄緑を重ねる。 -->
      <g v-for="(bracket, index) in [...brackets].reverse()" :key="index" fill="none" stroke-linecap="square" stroke-linejoin="miter" :opacity="bracket.opacity">
        <path :d="bracket.d" :stroke="FOCUS_OUTLINE_COLOR" :stroke-width="bracket.width + 2" vector-effect="non-scaling-stroke" />
        <path :d="bracket.d" :stroke="FOCUS_COLOR" :stroke-width="bracket.width" vector-effect="non-scaling-stroke" />
      </g>
    </svg>
    <span v-else class="focus-layer__none">{{ compact ? FOCUS_NONE_LABEL_COMPACT : FOCUS_NONE_LABEL }}</span>
  </div>
</template>
