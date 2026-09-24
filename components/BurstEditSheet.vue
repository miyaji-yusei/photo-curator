<script setup lang="ts">
// まとまり編集シート（設計 02 章）。時間軸の帯。線をドラッグして分ける/つなげる
// （Android版 Burst.kt の BurstEditSheet と同じ操作感。44px相当のハンドルを
// ドラッグすると境目が隣へ動き、タップだけでも切る/繋ぐを切り替えられる）。
// 写真はサムネイルで見せ、タップで代表を選び直す。3色巡回（単独は色を持たず
// 巡回も進めない）。確定前に「元のn枚 → まとまりn組 ＋ 単独n枚」。
import { computed, onMounted, ref, watch } from 'vue'
import { boundaryCount, blocksFromCuts } from '~/utils/burstEdit'
import * as core from '~/lib/core'
import type { Session, PhotoRef, PairOverride, BurstThreshold } from '~/lib/core'
import { useBackend } from '~/composables/useBackend'
import { useRoute } from 'vue-router'

const props = defineProps<{
  session: Session
  representative: string
  photos: PhotoRef[]
  // いまのプロジェクトの連写の基準・連写まとめの on/off。**既定値で上書きしない**
  // （学習済みの基準を無視すると、確定のたびに違う組み方に戻ってしまう不具合の元）。
  threshold: BurstThreshold
  groupBursts: boolean
}>()
const emit = defineEmits<{ close: []; apply: [session: Session] }>()

const backend = useBackend()
const route = useRoute()
const projectId = String(route.params.id)

const members = computed(() => props.session.members[props.representative] ?? [props.representative])
// 撮影順に並べる。
const run = computed(() => {
  const order = new Map(props.photos.map((p, i) => [p.relative_path, i]))
  return [...members.value].sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0))
})
const cuts = ref<boolean[]>(noCutsFor(run.value))
function noCutsFor(r: string[]) {
  return Array.from({ length: boundaryCount(r) }, () => false)
}
const blocks = computed(() => blocksFromCuts(run.value, cuts.value))

// サムネイル。バグ修正（07章）: 以前はファイル名のテキストしか出ておらず、
// 表示用画像のURLを取得していなかった。cull.vue と同じ backend.displayUrl で取る。
const displayUrls = ref<Record<string, string>>({})
async function loadThumbs() {
  const missing = run.value.filter(p => !displayUrls.value[p])
  if (missing.length === 0) return
  const entries = await Promise.all(missing.map(async p => [p, await backend.displayUrl(projectId, p)] as const))
  for (const [p, url] of entries) if (url) displayUrls.value[p] = url
}
onMounted(loadThumbs)
watch(run, loadThumbs)

// run の各位置がどのまとまり（blocks の何番目）に属するか。
const blockSeqOfRunIndex = computed<number[]>(() => {
  const map: number[] = []
  blocks.value.forEach((block, seq) => {
    for (let i = 0; i < block.length; i += 1) map.push(seq)
  })
  return map
})
// 3色巡回の色番号。**単独（1枚だけの塊）は色を持たず、巡回も進めない。**
const BLOCK_COLORS = ['#2a3320', '#1f2a33', '#332025']
const BLOCK_TINTS = ['#d6ff73', '#a7c8ff', '#ffb3e6']
const colorNumOfRunIndex = computed<number[]>(() => {
  const map: number[] = []
  let cursor = 0
  blocks.value.forEach((block) => {
    const color = block.length > 1 ? cursor % BLOCK_COLORS.length : -1
    for (let i = 0; i < block.length; i += 1) map.push(color)
    if (block.length > 1) cursor += 1
  })
  return map
})
function tileStyle(runIndex: number): string {
  const color = colorNumOfRunIndex.value[runIndex]!
  const grouped = color >= 0
  const border = grouped ? `2px solid ${BLOCK_TINTS[color]}` : '2px solid transparent'
  return `background: ${grouped ? BLOCK_COLORS[color] : '#16181d'}; border: ${border}; opacity: ${grouped ? 1 : 0.55}`
}
function tintOf(runIndex: number): string {
  const color = colorNumOfRunIndex.value[runIndex]!
  return color >= 0 ? BLOCK_TINTS[color]! : '#5f6572'
}

// 写真をタップで代表を選び直す（塊が2枚以上のときだけ意味がある）。
const leaderOverride = ref<Record<number, string>>({})
function pickLeader(runIndex: number, path: string) {
  const seq = blockSeqOfRunIndex.value[runIndex]!
  const block = blocks.value[seq]!
  if (block.length < 2) return
  leaderOverride.value = { ...leaderOverride.value, [seq]: path }
}
function isLeader(runIndex: number, path: string): boolean {
  const seq = blockSeqOfRunIndex.value[runIndex]!
  const block = blocks.value[seq]!
  if (block.length < 2) return false
  return (leaderOverride.value[seq] ?? block[0]) === path
}

function toggleCut(index: number) {
  const next = [...cuts.value]
  next[index] = !next[index]
  cuts.value = next
}

// ---- ドラッグで境目を隣へ動かす（Android版 Burst.kt の Divider.onDrag と同じ考え方）。
// **切れている境目だけ動かせる。** 繋がっている境目はタップで切ってから動かす。
const TILE_STRIDE = 152 // タイル144px + 間隔8px（下の style と合わせる）。
let dragIndex = -1
let dragStartX = 0
function onHandlePointerDown(index: number, event: PointerEvent) {
  if (!cuts.value[index]) return
  dragIndex = index
  dragStartX = event.clientX
  ;(event.target as HTMLElement).setPointerCapture(event.pointerId)
}
function onHandlePointerMove(event: PointerEvent) {
  if (dragIndex < 0) return
  const delta = event.clientX - dragStartX
  if (Math.abs(delta) < TILE_STRIDE * 0.5) return
  const toRight = delta > 0
  const target = toRight ? dragIndex + 1 : dragIndex - 1
  if (target < 0 || target > run.value.length - 2) return
  if (cuts.value[target]) return // 隣にも境目があるなら重ねない（既に切れている）。
  const next = [...cuts.value]
  next[dragIndex] = false
  next[target] = true
  cuts.value = next
  dragIndex = target
  dragStartX = event.clientX
}
function onHandlePointerUp() {
  dragIndex = -1
}

async function confirm() {
  // cuts から、隣どうしの override（join/split）を作る。
  const overrides: PairOverride[] = []
  for (let i = 0; i < run.value.length - 1; i += 1) {
    overrides.push({
      left: run.value[i]!,
      right: run.value[i + 1]!,
      decision: cuts.value[i] ? 'split' : 'join'
    })
  }
  const existing = await backend.loadOverrides(projectId)
  const merged = existing.filter(o => !overrides.some(n => n.left === o.left && n.right === o.right))
  merged.push(...overrides)
  await backend.saveOverrides(projectId, merged)

  // **いまのプロジェクトの基準をそのまま使う。** 既定値(distance=9)で
  // 上書きすると、学習した基準と違う組み方になって選別のやり直しに近い
  // 見え方になってしまう。
  let next = core.regroup(props.session, props.photos, props.groupBursts, props.threshold, merged)

  // 写真をタップで選んだ代表を反映する。**塊の先頭が既定の代表**なので、
  // それと違う写真が選ばれていたときだけ core.setRepresentative で差し替える。
  blocks.value.forEach((block, seq) => {
    if (block.length < 2) return
    const wanted = leaderOverride.value[seq]
    const head = block[0]!
    if (!wanted || wanted === head) return
    const swapped = core.setRepresentative(next, head, wanted)
    if (swapped) next = swapped
  })

  emit('apply', next)
}
</script>

<template>
  <v-dialog :model-value="true" max-width="960" @update:model-value="() => emit('close')">
    <v-card title="まとまりを編集" style="max-height: 88vh; display: flex; flex-direction: column">
      <v-card-text style="overflow-y: auto">
        <p class="text-caption text-medium-emphasis mb-2">
          写真をタップすると、その塊の代表に選び直せます（★印）。境目のハンドルはタップで切る/繋ぐ、
          左右にドラッグすると隣の境目へ動かせます
        </p>
        <div class="d-flex align-center mb-4" style="overflow-x: auto; padding-bottom: 8px">
          <template v-for="(id, index) in run" :key="id">
            <div
              :style="`${tileStyle(index)}; width: 144px; height: 144px; border-radius: 6px; flex-shrink: 0; position: relative; overflow: hidden; cursor: pointer`"
              @click="pickLeader(index, id)"
            >
              <img
                v-if="displayUrls[id]"
                :src="displayUrls[id]"
                :alt="id.split('/').pop()"
                style="width: 100%; height: 100%; object-fit: cover"
              >
              <div v-else class="d-flex align-center justify-center text-caption text-medium-emphasis" style="width: 100%; height: 100%">
                {{ id.split('/').pop() }}
              </div>
              <v-chip v-if="isLeader(index, id)" size="x-small" color="lime" class="text-black" style="position: absolute; top: 4px; right: 4px">
                <v-icon icon="mdi-star" size="12" start />代表
              </v-chip>
            </div>
            <!-- 境目のハンドル。Android版の 44dp 丸ハンドルに相当（タップ=切る/繋ぐ、ドラッグ=移動）。 -->
            <div
              v-if="index < run.length - 1"
              style="width: 32px; flex-shrink: 0; display: flex; align-items: center; justify-content: center; height: 144px; cursor: pointer; touch-action: none"
              @click="toggleCut(index)"
              @pointerdown="onHandlePointerDown(index, $event)"
              @pointermove="onHandlePointerMove"
              @pointerup="onHandlePointerUp"
              @pointercancel="onHandlePointerUp"
            >
              <div
                v-if="cuts[index]"
                style="width: 32px; height: 32px; border-radius: 50%; display: flex; align-items: center; justify-content: center"
                :style="`background: ${tintOf(index - 1 >= 0 ? index - 1 : index)}`"
              >
                <v-icon icon="mdi-content-cut" size="16" color="black" />
              </div>
              <div v-else style="width: 2px; height: 100%; background: #5f6572" />
            </div>
          </template>
        </div>
        <p class="text-body-2 text-medium-emphasis">
          元の {{ run.length }} 枚 → まとまり {{ blocks.filter(b => b.length > 1).length }} 組 ＋
          単独 {{ blocks.filter(b => b.length === 1).length }} 枚
        </p>
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn @click="emit('close')">キャンセル</v-btn>
        <v-btn color="primary" @click="confirm">確定</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
