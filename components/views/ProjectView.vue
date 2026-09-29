<script setup lang="ts">
import type { Ref } from 'vue'
import type { Project } from '~/types/photo'

const {
  activeProject: nullableActiveProject,
  askDeleteProject,
  canImportPhotos,
  densityOptions,
  desktop,
  displayBacklog,
  displayBusy,
  displayEdge,
  displaySettings,
  enterMethod,
  gridClass,
  gridStyle,
  hasSelectionData,
  largeDisplay,
  openPhotoPicker,
  openResults,
  openZoom,
  previewDensity,
  previewPhotos,
  previewTotal,
  regenerateDisplayImages,
  resumeSession,
  scanRunning,
  session,
  startScan,
  view
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const activeProject = nullableActiveProject as Ref<Project>
</script>

<template>
    <div class="d-flex align-center justify-space-between flex-wrap ga-4 mb-7"><div><v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="view = 'home'">ホーム</v-btn><h1 class="text-h4 font-weight-bold">{{ activeProject.name }}</h1><p class="text-body-2 text-medium-emphasis mt-1">{{ activeProject.folderPath }}</p></div><div class="d-flex flex-wrap ga-2"><v-btn variant="text" prepend-icon="mdi-delete-outline" @click="askDeleteProject(activeProject)">削除</v-btn><v-btn v-if="hasSelectionData" variant="text" prepend-icon="mdi-star-outline" @click="openResults">選別結果を見る</v-btn><v-btn v-if="canImportPhotos" variant="outlined" prepend-icon="mdi-image-plus" :loading="scanRunning" @click="openPhotoPicker">写真を追加</v-btn><v-btn v-else variant="outlined" prepend-icon="mdi-refresh" :loading="scanRunning" @click="startScan">写真を再読み込み</v-btn><v-btn color="primary" prepend-icon="mdi-play" :disabled="!activeProject.photoCount" @click="session ? resumeSession() : enterMethod()">{{ session ? '選別を再開' : '選別を開始' }}</v-btn></div></div>
    <v-card class="mb-6"><v-card-text class="d-flex align-center ga-5"><v-avatar color="primary" size="50"><v-icon color="black" icon="mdi-image-multiple" /></v-avatar><div><div class="text-h6">{{ activeProject.photoCount.toLocaleString() }} 枚の写真</div><div class="text-body-2 text-medium-emphasis">{{ canImportPhotos ? '星とサムネイルはこの端末に保存されます。写真ライブラリは変更しません。' : 'サブフォルダも含めて参照します。写真ファイルは変更しません。' }}</div></div></v-card-text></v-card>
    <!-- 選別画面に出す画像の大きさ。**解析を起こす場所の隣に置く**ので
         対応が分かりやすい。設定画面では全体の既定を決められる。 -->
    <v-card v-if=displaySettings class="mb-6">
      <v-card-text>
        <div class="d-flex align-center flex-wrap ga-4">
          <v-switch
            v-model="largeDisplay" color="primary" hide-details density="comfortable"
            :disabled="displayBusy || displaySettings.choices.length < 2"
            label="大きな画像で選別する"
          />
          <span class="text-caption text-medium-emphasis">
            いま長辺 <strong>{{ displayEdge }}px</strong>
            <template v-if="displayBacklog > 0">・残り {{ displayBacklog.toLocaleString() }} 枚を作成中</template>
          </span>
          <v-spacer />
          <v-btn
            size="small" variant="text" prepend-icon="mdi-refresh"
            :loading="displayBusy" :disabled="displaySettings.choices.length < 2"
            @click="regenerateDisplayImages"
          >作り直す</v-btn>
        </div>
        <p class="text-caption text-medium-emphasis mt-2 mb-0">
          2 枚並べて見比べるときだけ大きさが要ります。3〜4 枚なら既定で十分です。
          <strong>大きくするときは写真を読み直す</strong>ので時間がかかります（小さくするときは一瞬です）。
        </p>
        <v-progress-linear v-if="displayBacklog > 0" indeterminate color="primary" class="mt-3" />
      </v-card-text>
    </v-card>

    <div v-if="previewPhotos.length" class="d-flex align-center justify-end ga-3 mb-4">
      <v-btn-toggle v-model="previewDensity" density="comfortable" variant="outlined" divided mandatory>
        <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
      </v-btn-toggle>
    </div>
    <div v-if="previewPhotos.length" class="photo-grid" :class="gridClass(previewDensity)" :style="gridStyle(previewDensity)"><div v-for="photo in previewPhotos" :key="photo.id" class="photo-tile" role="button" tabindex="0" @click="openZoom(photo, previewPhotos)" @keydown.enter="openZoom(photo, previewPhotos)"><img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy"><div class="photo-tile__caption">{{ photo.relativePath }}</div></div></div>
    <v-alert v-if="previewTotal > previewPhotos.length" type="info" variant="tonal" class="mt-5">表示負荷を抑えるため、最初の {{ previewPhotos.length }} 枚だけを表示しています。選別にはすべての写真が含まれます。</v-alert>
    <!-- まだ 1 枚も無いプロジェクト。次にやることを 1 つだけ置く。 -->
    <v-card v-if="canImportPhotos && !previewPhotos.length && !scanRunning" class="pa-10 text-center">
      <v-icon icon="mdi-image-plus" size="44" class="text-medium-emphasis" />
      <div class="text-h6 mt-4">まだ写真がありません</div>
      <p class="text-body-2 text-medium-emphasis mt-2 mb-6">
        この端末の写真から選びます。写真そのものは端末の外に出ません。
      </p>
      <v-btn color="primary" size="large" prepend-icon="mdi-image-plus" @click="openPhotoPicker">写真を追加</v-btn>
    </v-card>
</template>
