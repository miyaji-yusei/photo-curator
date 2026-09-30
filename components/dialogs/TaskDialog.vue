<script setup lang="ts">
const {
  cancelTask,
  progressValue,
  taskDialog,
  taskProgress
} = useCurator()
</script>

<template>
<v-dialog v-model="taskDialog" persistent max-width="520"><v-card><v-card-title>写真を読み込み中</v-card-title><v-card-text class="pt-5"><div class="d-flex justify-space-between text-body-2 mb-3"><span>{{ taskProgress?.message }}</span><span v-if="taskProgress?.total">{{ taskProgress.processed.toLocaleString() }} / {{ taskProgress.total.toLocaleString() }}</span></div><v-progress-linear :model-value="progressValue" :indeterminate="!taskProgress?.total" color="primary" height="10" rounded /><p class="text-caption text-medium-emphasis mt-5 mb-0">読み込みのあと、連写の解析はバックグラウンドで少しずつ進みます。キャンセルしても、完了済みの読み込み結果は保持されます。</p></v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="cancelTask">キャンセル</v-btn></v-card-actions></v-card></v-dialog>
</template>
