<script setup lang="ts">
// 連写の中身を選別（設計 02 章 burst-review。app-android BurstReview.kt が見本）。
// 結果画面の ⧉ タイルから。ラウンドにしない。1画面に全部並べて、選んだものだけ
// **いまの星 + 1**（★5 が上限）。選ばなかった分はいまの星のまま。
// 決めるまでデータは変えない。
//
// UIは通常の選別（pages/project/[id]/cull.vue のトーナメント画面）と同じ
// タイルグリッド・同じ操作感に揃える（07章 item 5）: 実測したコンテナの
// 縦横比で行×列を決める CSS グリッド、拡大鏡アイコン、object-fit: contain。
import { computed, onBeforeUnmount, ref, watch } from 'vue'

const props = defineProps<{
  members: string[] // 撮影順（代表を含む）
  baseStar: number
  displayUrls: Record<string, string>
}>()
const emit = defineEmits<{ close: []; apply: [changes: Record<string, number>]; zoom: [index: number] }>()

const picked = ref<Set<string>>(new Set())
function toggle(path: string) {
  const set = new Set(picked.value)
  if (set.has(path)) set.delete(path); else set.add(path)
  picked.value = set
}
const nextStar = computed(() => Math.min(5, props.baseStar + 1))

function confirm() {
  const changes: Record<string, number> = {}
  for (const path of picked.value) changes[path] = nextStar.value
  emit('apply', changes)
}

// cull.vue と同じ枠の行×列決め（GRID表・gridFor）。ここは連写の中身なので
// 「一度に見比べる枚数」の設定は関係なく、常に全部を1画面に並べる。
const GRID: Record<number, [number, number]> = {
  1: [1, 1], 2: [1, 2], 3: [1, 3], 4: [2, 2], 5: [2, 3],
  6: [2, 3], 7: [2, 4], 8: [2, 4], 9: [3, 3], 10: [2, 5]
}
function gridFor(count: number, landscape: boolean): [number, number] {
  const found = GRID[count]
  const [rows, cols] = found ?? [Math.ceil(count / 5), Math.min(5, Math.max(1, count))]
  return landscape ? [rows, cols] : [cols, rows]
}
const stageEl = ref<HTMLElement | null>(null)
const stageSize = ref({ width: 0, height: 0 })
let stageObserver: ResizeObserver | null = null
watch(stageEl, (el) => {
  stageObserver?.disconnect()
  stageObserver = null
  if (el) {
    stageObserver = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      if (rect) stageSize.value = { width: rect.width, height: rect.height }
    })
    stageObserver.observe(el)
  }
})
onBeforeUnmount(() => stageObserver?.disconnect())
const gridDims = computed<[number, number]>(() => {
  const landscape = stageSize.value.width >= stageSize.value.height
  return gridFor(props.members.length, landscape)
})
</script>

<template>
  <div class="d-flex flex-column" style="position: fixed; inset: 0; background: #101114; z-index: 1500">
    <div class="d-flex align-center px-3" style="height: 56px; flex-shrink: 0">
      <v-btn icon="mdi-arrow-left" variant="text" @click="emit('close')" />
      <div class="flex-grow-1 ml-2 text-center">
        <span class="text-body-2">連写の中身を選別 · {{ members.length }} 枚 · いまは全部 ★{{ baseStar }}</span>
      </div>
      <span class="text-caption text-medium-emphasis mr-3">
        {{ picked.size === 0 ? `選ぶと ★${nextStar} に上がります` : `${picked.size} 枚が ★${nextStar} に上がります` }}
      </span>
      <v-btn color="primary" variant="flat" :disabled="picked.size === 0" @click="confirm">この結果にする</v-btn>
    </div>
    <div
      ref="stageEl"
      class="flex-grow-1 pa-2"
      style="min-height: 0; overflow: hidden; display: grid; gap: 6px"
      :style="{ gridTemplateColumns: `repeat(${gridDims[1]}, 1fr)`, gridTemplateRows: `repeat(${gridDims[0]}, 1fr)` }"
    >
      <div
        v-for="(path, index) in members"
        :key="path"
        style="position: relative; min-width: 0; min-height: 0; background: #16181d; border-radius: 6px; overflow: hidden; cursor: pointer"
        :style="picked.has(path) ? 'outline: 3px solid #d6ff73' : ''"
        @click="toggle(path)"
      >
        <img v-if="displayUrls[path]" :src="displayUrls[path]" style="width: 100%; height: 100%; object-fit: contain">
        <span class="text-caption" style="position: absolute; left: 4px; top: 4px; background: rgba(0,0,0,.6); padding: 0 4px">{{ index + 1 }}</span>
        <div style="position: absolute; right: 4px; top: 4px; display: flex; flex-direction: column; gap: 4px">
          <v-btn icon="mdi-magnify" size="x-small" variant="text" style="background: rgba(0,0,0,.4)" @click.stop="emit('zoom', index)" />
        </div>
        <v-icon v-if="picked.has(path)" icon="mdi-check-circle" color="#d6ff73" style="position: absolute; right: 4px; bottom: 4px" />
      </div>
    </div>
    <p class="text-center text-caption text-medium-emphasis my-1" style="flex-shrink: 0">
      選んだ写真だけ ★{{ nextStar }} に上がります（選ばなければ ★{{ baseStar }} のまま）
    </p>
  </div>
</template>
