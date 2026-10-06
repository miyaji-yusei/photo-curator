<script setup lang="ts">
import type { Ref } from 'vue'
import type { Project } from '~/types/photo'
import { canChangeDisplayEdge, displayEdgeLabel } from '~/utils/displayEdge'
import { canStartSelection } from '~/utils/selectionGate'

const {
  activeProject: nullableActiveProject,
  askDeleteProject,
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
  importsByPicker,
  largeDisplay,
  loadMorePreview,
  MAX_RATING,
  openPhotoPicker,
  setPairRawJpeg,
  openResults,
  prepareLines,
  requestDisplayEdge,
  ratingCount,
  openZoom,
  previewDensity,
  previewPhotos,
  regenerateDisplayImages,
  restartDialog,
  resumeSession,
  saveSidecarNow,
  scanRunning,
  selectionSummary,
  sidecarAccess,
  sidecarBusy,
  sidecarChecking,
  sidecarClash,
  sidecarMessage,
  sidecarNotice,
  sidecarDetached,
  sidecarSavedAt,
  writeSidecarToNas,
  session,
  startScan,
  view
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const activeProject = nullableActiveProject as Ref<Project>
</script>

<template>
    <div class="d-flex align-center justify-space-between flex-wrap ga-4 mb-7"><div><v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="view = 'home'">ホーム</v-btn><h1 class="text-h4 font-weight-bold">{{ activeProject.name }}</h1><p class="text-body-2 text-medium-emphasis mt-1"><v-icon v-if="activeProject.sourceKind === 'amazon'" icon="mdi-cloud-outline" size="small" class="mr-1" aria-label="Amazon Photos" />{{ activeProject.folderPath }}</p></div><div class="d-flex flex-wrap align-center ga-2 project-actions"><v-menu><template #activator="{ props: menuProps }"><v-btn v-bind="menuProps" variant="text" icon="mdi-dots-vertical" aria-label="その他の操作" /></template><v-list density="compact"><template v-if="displaySettings && displaySettings.choices.length"><v-list-subheader>表示用画像の px</v-list-subheader><v-list-item v-if="!canChangeDisplayEdge(displaySettings.canRebuild)" disabled prepend-icon="mdi-image-size-select-large" :title="`${displayEdge}px`" subtitle="原本がないので変更できません" /><template v-else><v-list-item v-for="choice in displaySettings.choices" :key="choice" :disabled="displayBusy" :active="choice === displayEdge" :prepend-icon="choice === displayEdge ? 'mdi-check' : undefined" :title="displayEdgeLabel(choice)" @click="requestDisplayEdge(choice)" /></template><v-divider /></template><template v-if="activeProject.sourceKind === 'folder' && !importsByPicker"><v-list-item :prepend-icon="activeProject.pairRawJpeg ? 'mdi-check' : undefined" title="同名の JPEG と RAW を 1 枚の写真として扱う" :subtitle="activeProject.pairRawJpeg ? 'オン。RAW を対象から外します' : 'オフ。全ファイルを対象にします'" @click="setPairRawJpeg(!activeProject.pairRawJpeg)" /><v-divider /></template><v-list-item prepend-icon="mdi-restart" title="選別を最初からやり直す" base-color="error" @click="restartDialog = true" /></v-list></v-menu><v-btn v-if="hasSelectionData" variant="text" prepend-icon="mdi-star-outline" @click="openResults">選別結果を見る</v-btn><v-btn variant="text" prepend-icon="mdi-delete-outline" @click="askDeleteProject(activeProject)">削除</v-btn><v-btn v-if="importsByPicker" variant="outlined" prepend-icon="mdi-image-plus" :loading="scanRunning" @click="openPhotoPicker">写真を追加</v-btn><v-btn v-else-if="activeProject.folderAccess === 'needs-permission'" variant="outlined" color="warning" prepend-icon="mdi-folder-key-outline" :loading="scanRunning" @click="startScan">フォルダへのアクセスを許可</v-btn><v-btn v-else variant="outlined" prepend-icon="mdi-refresh" :loading="scanRunning" @click="startScan">{{ activeProject.sourceKind === 'amazon' && activeProject.status === 'missing' ? '再試行' : '写真を再読み込み' }}</v-btn><v-btn color="primary" prepend-icon="mdi-play" :disabled="!canStartSelection({ photoCount: activeProject.photoCount, sidecarClash: !!sidecarClash, sidecarChecking })" :loading="sidecarChecking" @click="session ? resumeSession() : enterMethod()">{{ session ? '選別を再開' : '選別を開始' }}</v-btn></div></div>
    <v-card class="mb-6"><v-card-text class="d-flex align-center ga-5"><v-avatar color="primary" size="50"><v-icon color="black" icon="mdi-image-multiple" /></v-avatar><div><div class="text-h6">{{ activeProject.photoCount.toLocaleString() }} 枚の写真</div><div class="text-body-2 text-medium-emphasis">{{ activeProject.sourceKind === 'amazon' ? 'Amazon Photos の共有リンクから読んでいます。写真は変更しません。原本は拡大したときだけ取ってきます。' : importsByPicker ? '星とサムネイルはこの端末に保存されます。写真ライブラリは変更しません。' : 'サブフォルダも含めて参照します。写真ファイルは変更しません。' }}</div></div></v-card-text></v-card>
    <!-- 準備の進み。総数が分からない間（走査中）は `n / ?`。 -->
    <v-card class="mb-6">
      <v-card-title class="text-subtitle-1 pt-4 px-5">準備の進み</v-card-title>
      <v-card-text class="pt-2">
        <div v-for="line in prepareLines" :key="line.label" class="prepare-line d-flex align-center ga-4 mb-2">
          <span class="prepare-line__label text-body-2">{{ line.label }}</span>
          <v-progress-linear :model-value="line.total ? (line.done / line.total) * 100 : 0" :indeterminate="line.total === null" color="primary" height="6" rounded class="flex-grow-1" />
          <span class="prepare-line__count text-body-2 text-medium-emphasis">{{ line.done.toLocaleString() }} / {{ line.total === null ? '?' : line.total.toLocaleString() }}</span>
        </div>
      </v-card-text>
    </v-card>
    <!-- 星ごとの行。押すと、その星に絞った結果の画面へ。 -->
    <div v-if="selectionSummary?.total" class="rating-board mb-6">
      <div
        v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
        class="rating-row" role="button" tabindex="0"
        :aria-label="`★${rating} の結果を見る`"
        @click="openResults(rating)"
        @keydown.enter="openResults(rating)"
      >
        <span class="rating-row__stars">
          <v-icon v-for="star in MAX_RATING" :key="star" size="17" :icon="star <= rating ? 'mdi-star' : 'mdi-star-outline'" :class="star <= rating ? 'text-primary' : 'text-medium-emphasis'" />
        </span>
        <span class="rating-row__count">{{ ratingCount(rating).toLocaleString() }} 枚</span>
      </div>
    </div>
    <!-- リンクが消えたとき。開くたびに自動では読みにいかない。 -->
    <v-alert v-if="activeProject.sourceKind === 'amazon' && activeProject.status === 'missing'" type="warning" variant="tonal" class="mb-6">
      このリンクは削除されたか、無効です。
    </v-alert>
    <!-- サイドカー。写真のフォルダの `.photo-curator/catalog.json` に判断だけを記録する。 -->
    <v-card class="mb-6">
      <v-card-text class="d-flex align-center flex-wrap ga-4">
        <v-avatar :color="sidecarAccess === 'readwrite' ? 'primary' : undefined" size="36">
          <v-icon :color="sidecarAccess === 'readwrite' ? 'black' : undefined" :icon="sidecarAccess === 'readwrite' ? 'mdi-note-check-outline' : 'mdi-note-off-outline'" />
        </v-avatar>
        <div>
          <div class="text-body-1">{{ sidecarDetached ? 'この端末だけの結果（NAS とは別）' : sidecarAccess === 'readwrite' ? '写真のフォルダに記録しています' : 'この端末だけの結果' }}</div>
          <div v-if="sidecarDetached" class="text-caption text-medium-emphasis">NAS の記録とは切り離しています。自動では書き込みません。</div>
          <div v-if="sidecarSavedAt" class="text-caption text-medium-emphasis">最後に保存 {{ new Date(sidecarSavedAt).toLocaleTimeString('ja-JP') }}</div>
          <div v-if="sidecarNotice" class="text-caption text-medium-emphasis">{{ sidecarNotice }}</div>
          <div v-if="sidecarMessage" class="text-caption text-error">{{ sidecarMessage }}</div>
        </div>
        <v-spacer />
        <v-btn v-if="sidecarAccess === 'readwrite' && sidecarDetached" variant="outlined" prepend-icon="mdi-upload-outline" :loading="sidecarBusy" :disabled="!!sidecarClash" @click="writeSidecarToNas">NAS に書き込む</v-btn>
        <v-btn v-else-if="sidecarAccess === 'readwrite'" variant="outlined" prepend-icon="mdi-content-save-outline" :loading="sidecarBusy" :disabled="!!sidecarClash" @click="saveSidecarNow">今すぐ保存</v-btn>
      </v-card-text>
    </v-card>

    <!-- 選別画面に出す画像の大きさ。**解析を起こす場所の隣に置く**ので
         対応が分かりやすい。設定画面では全体の既定を決められる。 -->
    <v-card v-if=displaySettings class="mb-6">
      <v-card-text>
        <div class="d-flex align-center flex-wrap ga-4">
          <v-switch
            v-model="largeDisplay" color="primary" hide-details density="comfortable"
            :disabled="displayBusy || displaySettings.choices.length < 2 || displaySettings.canRebuild === false"
            label="大きな画像で選別する"
          />
          <span class="text-caption text-medium-emphasis">
            いま長辺 <strong>{{ displayEdge }}px</strong>
            <template v-if="displayBacklog > 0">・残り {{ displayBacklog.toLocaleString() }} 枚を作成中</template>
          </span>
          <v-spacer />
          <v-btn
            size="small" variant="text" prepend-icon="mdi-refresh"
            :loading="displayBusy" :disabled="displaySettings.choices.length < 2 || displaySettings.canRebuild === false"
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
    <VirtualPhotoGrid
      v-if="previewPhotos.length"
      :items="previewPhotos" :item-key="photo => photo.id" :item-url="photo => desktop.photoThumbnailUrl(photo)"
      :columns="previewDensity" :gap="16" :tile-aspect="1"
      @end="loadMorePreview"
    >
      <template #default="{ item: photo, url }">
        <div class="photo-tile" role="button" tabindex="0" @click="openZoom(photo, previewPhotos)" @keydown.enter="openZoom(photo, previewPhotos)"><img :src="url" :alt="photo.name" loading="lazy" decoding="async"><div class="photo-tile__caption">{{ activeProject.sourceKind === 'amazon' ? photo.name : photo.relativePath }}</div></div>
      </template>
    </VirtualPhotoGrid>
    <!-- まだ 1 枚も無いプロジェクト。次にやることを 1 つだけ置く。 -->
    <v-card v-if="importsByPicker && !previewPhotos.length && !scanRunning" class="pa-10 text-center">
      <v-icon icon="mdi-image-plus" size="44" class="text-medium-emphasis" />
      <div class="text-h6 mt-4">まだ写真がありません</div>
      <p class="text-body-2 text-medium-emphasis mt-2 mb-6">
        この端末の写真から選びます。写真そのものは端末の外に出ません。
      </p>
      <v-btn color="primary" size="large" prepend-icon="mdi-image-plus" @click="openPhotoPicker">写真を追加</v-btn>
    </v-card>
</template>
