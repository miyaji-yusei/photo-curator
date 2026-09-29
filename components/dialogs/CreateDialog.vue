<script setup lang="ts">
const {
  canImportPhotos,
  chooseFolder,
  createDialog,
  createProject,
  fileName,
  folderPath,
  loading,
  projectName
} = useCurator()
</script>

<template>
<v-dialog v-model="createDialog" max-width="620"><v-card title="プロジェクトを作成"><v-card-text class="pt-5">
  <v-text-field v-model="projectName" label="プロジェクト名" :placeholder="folderPath ? fileName(folderPath) : '任意のプロジェクト名'" :hint="canImportPhotos ? '作成したあと「写真を追加」から選びます。' : '空欄ならフォルダ名を使います。'" persistent-hint class="mb-5" />
  <!-- デスクトップはフォルダを参照する。ブラウザは作成後にピッカーで選ぶ。 -->
  <v-text-field v-if="!canImportPhotos" v-model="folderPath" label="写真フォルダ" readonly prepend-inner-icon="mdi-folder-image"><template #append-inner><v-btn variant="outlined" size="small" @click="chooseFolder">選択</v-btn></template></v-text-field>
  <v-alert v-else type="info" variant="tonal" density="comfortable">
    この端末の写真から選びます。写真そのものは端末の外に出ません。
  </v-alert>
</v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="createDialog = false">キャンセル</v-btn><v-btn variant="outlined" color="primary" :disabled="!canImportPhotos && !folderPath" :loading="loading" @click="createProject">作成</v-btn></v-card-actions></v-card></v-dialog>
</template>
