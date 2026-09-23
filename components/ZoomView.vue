<script setup lang="ts">
// 1枚を大きく見る（設計 02 章 zoom シート）。app-android の Zoom.kt が見本。
// 表示用画像を先に出し、原本が届いたら黙って差し替える。左右で前後に送れる。
// 「残す」は選別から来たときだけ（showKeep）。
import { computed, onMounted, ref, watch } from 'vue'

const props = defineProps<{
  paths: string[]
  index: number
  displayUrls: Record<string, string>
  resolveOriginal: (path: string) => Promise<string | null>
  fileSize?: (path: string) => number | null | undefined
  showKeep?: boolean
}>()

const emit = defineEmits<{
  close: []
  keep: [path: string]
  move: [index: number]
}>()

const root = ref<HTMLElement | null>(null)
onMounted(() => root.value?.focus())

const path = computed(() => props.paths[props.index] ?? null)
const originalUrl = ref<string | null>(null)
const originalState = ref<'loading' | 'ready' | 'error'>('loading')

async function loadOriginal() {
  originalUrl.value = null
  originalState.value = 'loading'
  const target = path.value
  if (!target) return
  try {
    const url = await props.resolveOriginal(target)
    if (path.value !== target) return // 送り済み。古い答えは捨てる。
    if (url) {
      originalUrl.value = url
      originalState.value = 'ready'
    } else {
      originalState.value = 'error'
    }
  } catch {
    if (path.value === target) originalState.value = 'error'
  }
}
watch(path, loadOriginal, { immediate: true })

function move(step: number) {
  const next = props.index + step
  if (next >= 0 && next < props.paths.length) emit('move', next)
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowLeft') move(-1)
  else if (event.key === 'ArrowRight') move(1)
  else if (event.key === 'Escape') emit('close')
  else if (event.key === 'Enter' && props.showKeep && path.value) emit('keep', path.value)
}

const sizeText = computed(() => {
  const target = path.value
  if (!target || !props.fileSize) return null
  const bytes = props.fileSize(target)
  if (!bytes || bytes <= 0) return null
  return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)}KB` : `${(bytes / 1024 / 1024).toFixed(1)}MB`
})
</script>

<template>
  <div
    ref="root"
    class="zoom-view"
    style="position: fixed; inset: 0; background: #000; z-index: 2000; display: flex; flex-direction: column; outline: none"
    tabindex="0"
    @keydown="onKeydown"
  >
    <div class="d-flex align-center px-2" style="height: 48px; flex-shrink: 0; background: rgba(0,0,0,.5)">
      <v-btn icon="mdi-close" variant="text" color="white" @click="emit('close')" />
      <span class="flex-grow-1 text-caption" style="color: rgba(255,255,255,.85)">
        <template v-if="originalState === 'ready'">原本</template>
        <template v-else-if="originalState === 'error'">原本を読めません · 表示用画像で表示中</template>
        <template v-else>原本を読み込み中{{ sizeText ? ' ' + sizeText : '' }} · 表示用画像で先に表示</template>
      </span>
      <span v-if="paths.length > 1" class="text-caption mr-2" style="color: rgba(255,255,255,.85)">
        {{ index + 1 }} / {{ paths.length }}
      </span>
    </div>
    <!-- 写真・背景をタップで閉じる（app-android Zoom.kt の「等倍なら閉じる」と同じ）。
         左右の矢印・上下のバーは自分の click で止め、ここまで伝わらないようにする。 -->
    <div class="flex-grow-1" style="position: relative; min-height: 0; cursor: pointer" @click="emit('close')">
      <img
        v-if="path && displayUrls[path]"
        :src="displayUrls[path]"
        style="position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain"
      >
      <img
        v-if="originalState === 'ready' && originalUrl"
        :src="originalUrl"
        style="position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain"
      >
      <div v-if="originalState === 'loading'" style="position: absolute; inset: 0; display: flex; align-items: center; justify-content: center">
        <v-progress-circular indeterminate color="primary" size="44" />
      </div>
      <v-btn
        v-if="index > 0"
        icon="mdi-chevron-left"
        variant="tonal"
        style="position: absolute; left: 12px; top: 50%; transform: translateY(-50%)"
        @click.stop="move(-1)"
      />
      <v-btn
        v-if="index < paths.length - 1"
        icon="mdi-chevron-right"
        variant="tonal"
        style="position: absolute; right: 12px; top: 50%; transform: translateY(-50%)"
        @click.stop="move(1)"
      />
    </div>
    <div v-if="showKeep || $slots.extra" class="d-flex justify-center align-center ga-2 pa-4" style="flex-shrink: 0">
      <v-btn v-if="showKeep" color="primary" size="large" @click="path && emit('keep', path)">この1枚を残す</v-btn>
      <slot name="extra" :path="path" />
    </div>
  </div>
</template>
