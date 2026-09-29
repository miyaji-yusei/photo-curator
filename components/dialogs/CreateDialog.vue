<script setup lang="ts">
const {
  canImportPhotos,
  chooseFolder,
  createDialog,
  createProject,
  desktop,
  devFolderPath,
  fileName,
  folderPath,
  isDev,
  loading,
  projectName
} = useCurator()
</script>

<template>
<v-dialog v-model="createDialog" max-width="620"><v-card title="プロジェクトを作成"><v-card-text class="pt-5">
  <v-text-field v-model="projectName" label="プロジェクト名" :placeholder="folderPath ? fileName(folderPath) : '任意のプロジェクト名'" :hint="canImportPhotos && !folderPath && !devFolderPath.trim() ? '作成したあと「写真を追加」から選びます。' : '空欄ならフォルダ名を使います。'" persistent-hint class="mb-5" />
  <!-- フォルダを選べる環境（デスクトップ・showDirectoryPicker があるブラウザ）はフォルダを参照する。
       選べないブラウザは作成後にピッカーで選ぶ。 -->
  <v-text-field v-if="desktop.capabilities.browseFolders" v-model="folderPath" label="写真フォルダ" readonly prepend-inner-icon="mdi-folder-image" :hint="canImportPhotos ? '選ばないときは、作成したあと写真ピッカーから選びます。' : undefined" :persistent-hint="canImportPhotos"><template #append-inner><v-btn variant="outlined" size="small" @click="chooseFolder">選択</v-btn></template></v-text-field>
  <v-alert v-else type="info" variant="tonal" density="comfortable">
    この端末の写真から選びます。写真そのものは端末の外に出ません。
  </v-alert>
  <!-- 開発用（pnpm dev のときだけ）。OS のダイアログを使わず、フォルダの絶対パスで読む。 -->
  <v-text-field v-if="isDev" v-model="devFolderPath" label="(開発用) フォルダの絶対パス" placeholder="/tmp/photos" prepend-inner-icon="mdi-folder-wrench" class="mt-5" hide-details />
</v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="createDialog = false">キャンセル</v-btn><v-btn variant="outlined" color="primary" :disabled="!canImportPhotos && !folderPath" :loading="loading" @click="createProject">作成</v-btn></v-card-actions></v-card></v-dialog>
</template>
