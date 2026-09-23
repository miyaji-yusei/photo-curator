<script setup lang="ts">
// まとまり編集シート（設計 02 章）。時間軸の帯。線を動かして分ける/つなげる、
// 写真をタップで代表。確定前に「元のn枚 → まとまりn組 ＋ 単独n枚」。
import { computed, ref } from 'vue'
import { boundaryCount, blocksFromCuts } from '~/utils/burstEdit'
import * as core from '~/lib/core'
import type { Session, PhotoRef, PairOverride } from '~/lib/core'
import { useBackend } from '~/composables/useBackend'
import { useRoute } from 'vue-router'

const props = defineProps<{
  session: Session
  representative: string
  photos: PhotoRef[]
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

  const threshold: core.BurstThreshold = { window_ms: 4000, distance: 9, d_hash_version: 2 }
  const next = core.regroup(props.session, props.photos, true, threshold, merged)
  emit('apply', next)
}
</script>

<template>
  <v-dialog :model-value="true" max-width="720" @update:model-value="() => emit('close')">
    <v-card title="まとまりを編集">
      <v-card-text>
        <div class="d-flex ga-1 mb-4 flex-wrap">
          <template v-for="(id, index) in run" :key="id">
            <div style="width: 72px; height: 72px; background: #16181d; border-radius: 4px; display:flex; align-items:center; justify-content:center" class="text-caption">
              {{ id.split('/').pop() }}
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
