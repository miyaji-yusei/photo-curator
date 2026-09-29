<script setup lang="ts">
const {
  confirmDeleteProject,
  deleteBusy,
  deleteDialog,
  deleteTarget
} = useCurator()
</script>

<template>
<v-dialog v-model="deleteDialog" max-width="560">
  <v-card title="プロジェクトを削除しますか？">
    <v-card-text class="pt-5">
      <div class="text-body-1 mb-3">{{ deleteTarget?.name }}</div>
      <v-alert type="info" variant="tonal" density="comfortable">
        <strong>写真の原本は削除されません。</strong>
        消えるのは、このアプリが持っている写真の索引・選別の履歴・サムネイルだけです。
        フォルダ <code>{{ deleteTarget?.folderPath }}</code> はそのまま残ります。
      </v-alert>
      <p class="text-caption text-medium-emphasis mt-4 mb-0">選別の途中経過は元に戻せません。</p>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" :disabled="deleteBusy" @click="deleteDialog = false">キャンセル</v-btn>
      <v-btn color="error" :loading="deleteBusy" @click="confirmDeleteProject">削除する</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
