<script setup lang="ts">
// sidecarConflictDialog（設計02章）。別の端末の記録があるときの2択。
// 写真1枚ずつは選ばせない。選ばなかった方は catalog.<端末名>.json に残る。
import type { Sidecar } from '~/lib/core'

const props = defineProps<{ theirs: Sidecar }>()
const emit = defineEmits<{ keepMine: []; keepTheirs: [] }>()

function fmt(ms: number): string {
  return new Date(ms).toLocaleString('ja-JP')
}
</script>

<template>
  <v-dialog :model-value="true" max-width="480" persistent>
    <v-card title="どちらの記録を使いますか">
      <v-card-text>
        <p class="text-body-2 mb-3">
          この端末とは別に、<strong>{{ theirs.updatedByName }}</strong> が進めた選別の記録があります。
          写真ごとではなく、どちらか一方を選びます。
        </p>
        <div class="d-flex justify-space-between text-body-2 py-1">
          <span>この端末</span>
          <span class="text-medium-emphasis">いま進めている分</span>
        </div>
        <div class="d-flex justify-space-between text-body-2 py-1">
          <span>{{ theirs.updatedByName }}</span>
          <span class="text-medium-emphasis">{{ fmt(theirs.updatedAt) }} 更新</span>
        </div>
        <p class="text-caption text-medium-emphasis mt-3">
          選ばなかった方は消さず、フォルダに残します（catalog.{{ theirs.updatedBy.slice(0, 12) }}.json）。
        </p>
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn @click="emit('keepTheirs')">{{ theirs.updatedByName }} の記録を使う</v-btn>
        <v-btn color="primary" @click="emit('keepMine')">この端末の結果を使う</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
