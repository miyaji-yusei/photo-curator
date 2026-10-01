<script setup lang="ts">
import { displayRebuildNote } from '~/utils/displayEdge'

const {
  cancelDisplayEdge,
  confirmDisplayEdge,
  displayEdge,
  displayEdgeDialog,
  isAmazon,
  pendingDisplayEdge
} = useCurator()
</script>

<template>
<v-dialog :model-value="displayEdgeDialog" max-width="520" @update:model-value="open => { if (!open) cancelDisplayEdge() }">
  <v-card title="表示用画像を作り直しますか？">
    <v-card-text class="pt-5">
      <v-alert type="warning" variant="tonal" density="comfortable">
        表示用画像を作り直します（{{ displayEdge }}px から {{ pendingDisplayEdge }}px へ）。よろしいですか
      </v-alert>
      <p class="text-caption text-medium-emphasis mt-4 mb-0">
        {{ displayRebuildNote(isAmazon) }}
        星や連写のまとまりは変わりません。
      </p>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" @click="cancelDisplayEdge">キャンセル</v-btn>
      <v-btn color="primary" @click="confirmDisplayEdge">作り直す</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
