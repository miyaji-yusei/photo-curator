<script setup lang="ts">
// まとまり編集シート（設計 02 章）。時間軸の帯。線を動かして分ける/つなげる、
// 写真をタップで代表。3色巡回（単独は色を持たず巡回も進めない）。
// 確定前に「元のn枚 → まとまりn組 ＋ 単独n枚」。
import { computed, ref } from 'vue'
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
  return `background: ${color >= 0 ? BLOCK_COLORS[color] : '#16181d'}`
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
  <v-dialog :model-value="true" max-width="720" @update:model-value="() => emit('close')">
    <v-card title="まとまりを編集">
      <v-card-text>
        <p class="text-caption text-medium-emphasis mb-2">
          写真をタップすると、その塊の代表に選び直せます（★印）。はさみ/鎖のアイコンで境目を切る・繋ぐ
        </p>
        <div class="d-flex ga-1 mb-4 flex-wrap">
          <template v-for="(id, index) in run" :key="id">
            <div
              :style="tileStyle(index)"
              style="width: 72px; height: 72px; border-radius: 4px; display:flex; align-items:center; justify-content:center; cursor:pointer; position:relative"
              class="text-caption"
              @click="pickLeader(index, id)"
            >
              {{ id.split('/').pop() }}
              <v-icon v-if="isLeader(index, id)" icon="mdi-star" size="14" color="#d6ff73" style="position:absolute; top:2px; right:2px" />
            </div>
            <v-btn
              v-if="index < run.length - 1"
              :icon="cuts[index] ? 'mdi-content-cut' : 'mdi-link'"
              size="small"
              variant="text"
              @click="toggleCut(index)"
            />
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
