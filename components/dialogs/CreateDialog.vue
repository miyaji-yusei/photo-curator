<script setup lang="ts">
import { displayEdgeLabel } from '~/utils/displayEdge'

const {
  amazonError,
  amazonLoading,
  amazonPreview,
  amazonUrl,
  canImportPhotos,
  chooseFolder,
  createDialog,
  createDisplayChoices,
  createDisplayEdge,
  createProject,
  createTab,
  desktop,
  devFolderPath,
  fileName,
  folderPath,
  isDev,
  loadAmazonPreview,
  loading,
  projectName
} = useCurator()

const edgeItems = computed(() => createDisplayChoices.value.map(edge => ({ title: displayEdgeLabel(edge), value: edge })))
</script>

<template>
<v-dialog v-model="createDialog" max-width="620"><v-card title="プロジェクトを作成"><v-card-text class="pt-5">
  <!-- 出所の選び方。Amazon Photos を選べるのは PC だけ（`capabilities.amazon`）。 -->
  <v-tabs v-if="desktop.capabilities.amazon" v-model="createTab" color="primary" class="mb-5">
    <v-tab value="folder" prepend-icon="mdi-folder-image">フォルダ</v-tab>
    <v-tab value="amazon" prepend-icon="mdi-cloud-outline">Amazon Photos</v-tab>
  </v-tabs>

  <template v-if="!desktop.capabilities.amazon || createTab === 'folder'">
  <v-text-field v-model="projectName" label="プロジェクト名" :placeholder="folderPath ? fileName(folderPath) : '任意のプロジェクト名'" :hint="canImportPhotos && !folderPath && !devFolderPath.trim() ? '作成したあと「写真を追加」から選びます。' : '空欄ならフォルダ名を使います。'" persistent-hint class="mb-5" />
  <!-- フォルダを選べる環境（デスクトップ・showDirectoryPicker があるブラウザ）はフォルダを参照する。
       選べないブラウザは作成後にピッカーで選ぶ。 -->
  <v-text-field v-if="desktop.capabilities.browseFolders" v-model="folderPath" label="写真フォルダ" readonly prepend-inner-icon="mdi-folder-image" :hint="canImportPhotos ? '選ばないときは、作成したあと写真ピッカーから選びます。' : undefined" :persistent-hint="canImportPhotos"><template #append-inner><v-btn variant="outlined" size="small" @click="chooseFolder">選択</v-btn></template></v-text-field>
  <v-alert v-else type="info" variant="tonal" density="comfortable">
    この端末の写真から選びます。写真そのものは端末の外に出ません。
  </v-alert>
  <!-- 開発用（pnpm dev のときだけ）。OS のダイアログを使わず、フォルダの絶対パスで読む。 -->
  <v-text-field v-if="isDev" v-model="devFolderPath" label="(開発用) フォルダの絶対パス" placeholder="/tmp/photos" prepend-inner-icon="mdi-folder-wrench" class="mt-5" hide-details />
  </template>

  <!-- Amazon Photos の共有リンク。ログインしない。公開の一覧を読むだけ。 -->
  <template v-else>
  <v-text-field v-model="amazonUrl" label="共有リンク" placeholder="https://www.amazon.co.jp/photos/share/…" prepend-inner-icon="mdi-cloud-outline" :disabled="amazonLoading" hide-details="auto" :error-messages="amazonError ? [amazonError] : []" @keydown.enter="loadAmazonPreview"><template #append-inner><v-btn variant="outlined" size="small" :loading="amazonLoading" :disabled="!amazonUrl.trim()" @click="loadAmazonPreview">読み込む</v-btn></template></v-text-field>
  <template v-if="amazonPreview">
    <v-text-field v-model="projectName" label="プロジェクト名" :placeholder="amazonPreview.name" hint="空欄なら共有の名前を使います。" persistent-hint class="mt-5" />
    <div class="text-body-2 mt-4 mb-2"><strong>{{ amazonPreview.count.toLocaleString() }} 枚</strong>の写真</div>
    <div v-if="amazonPreview.samples.length" class="amazon-samples" aria-label="見本">
      <img v-for="sample in amazonPreview.samples" :key="sample" :src="desktop.photoUrl(sample)" alt="">
    </div>
  </template>
  <v-alert type="warning" variant="tonal" density="comfortable" class="mt-5">
    リンクを知っている人は誰でも見られます。選別が終わったら Amazon Photos でリンクを削除してください。
  </v-alert>
  <!-- ブラウザは写真を Amazon から直接表示する（端末には置かない）。 -->
  <p v-if="desktop.kind === 'local'" class="text-caption text-medium-emphasis mt-3">選別中も通信が要ります（写真は Amazon から表示します）。</p>
  </template>
  <!-- 表示用画像の大きさ。作成のあと、この大きさで最初から作る。選んだ値はアプリの既定にもなる。 -->
  <v-select v-if="edgeItems.length" v-model="createDisplayEdge" :items="edgeItems" label="表示用画像の大きさ" hint="大きいほど細部まで見えますが、容量と準備の時間が増えます。あとからプロジェクトの画面でも変えられます。" persistent-hint class="mt-5" />
</v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="createDialog = false">キャンセル</v-btn><v-btn variant="outlined" color="primary" :disabled="desktop.capabilities.amazon && createTab === 'amazon' ? !amazonPreview || !amazonPreview.count : !canImportPhotos && !folderPath" :loading="loading" @click="createProject">作成</v-btn></v-card-actions></v-card></v-dialog>
</template>

<style scoped>
.amazon-samples { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 4px; }
.amazon-samples img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 4px; background: rgba(128, 128, 128, 0.15); }
@media (max-width: 480px) { .amazon-samples { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
</style>
