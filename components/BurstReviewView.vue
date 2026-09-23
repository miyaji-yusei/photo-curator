<script setup lang="ts">
// 連写の中身を選別（設計 02 章 burst-review。app-android BurstReview.kt が見本）。
// 結果画面の ⧉ タイルから。ラウンドにしない。1画面に全部並べて、選んだものだけ
// **いまの星 + 1**（★5 が上限）。選ばなかった分はいまの星のまま。
// 決めるまでデータは変えない。
import { computed, ref } from 'vue'

const props = defineProps<{
  members: string[] // 撮影順（代表を含む）
  baseStar: number
  displayUrls: Record<string, string>
}>()
const emit = defineEmits<{ close: []; apply: [changes: Record<string, number>] }>()

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
</script>

<template>
  <div style="position: fixed; inset: 0; background: #000; z-index: 1500; display: flex; flex-direction: column">
    <div class="d-flex align-center px-3" style="height: 64px; flex-shrink: 0">
      <v-btn icon="mdi-arrow-left" variant="text" color="white" @click="emit('close')" />
      <div class="flex-grow-1 ml-2">
        <div class="text-caption" style="color: #d6ff73">連写の中身を選別</div>
        <div class="text-caption" style="color: rgba(255,255,255,.7)">{{ members.length }} 枚 · いまは全部 ★{{ baseStar }}</div>
      </div>
      <span class="text-caption mr-3" style="color: rgba(255,255,255,.7)">
        {{ picked.size === 0 ? `選ぶと ★${nextStar} に上がります` : `${picked.size} 枚が ★${nextStar} に上がります` }}
      </span>
      <v-btn color="primary" :disabled="picked.size === 0" @click="confirm">この結果にする</v-btn>
    </div>
    <div class="flex-grow-1 pa-2" style="overflow-y: auto">
      <div class="d-flex flex-wrap ga-2">
        <div
          v-for="(path, index) in members"
          :key="path"
          style="position: relative; width: 140px; height: 140px; background: #16181d; border-radius: 6px; overflow: hidden; cursor: pointer"
          :style="picked.has(path) ? 'outline: 3px solid #d6ff73' : ''"
          @click="toggle(path)"
        >
          <img v-if="displayUrls[path]" :src="displayUrls[path]" style="width: 100%; height: 100%; object-fit: contain">
          <span class="text-caption" style="position: absolute; left: 4px; top: 4px; background: rgba(0,0,0,.6); padding: 0 4px; color: white">{{ index + 1 }}</span>
          <v-icon v-if="picked.has(path)" icon="mdi-check-circle" color="#d6ff73" style="position: absolute; right: 4px; top: 4px" />
        </div>
      </div>
    </div>
    <p class="text-caption pa-3" style="color: rgba(255,255,255,.6); flex-shrink: 0">
      選んだ写真だけ ★{{ nextStar }} に上がります（選ばなければ ★{{ baseStar }} のまま）。多い分は下にスクロールして見られます
    </p>
  </div>
</template>
