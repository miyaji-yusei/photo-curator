<script setup lang="ts">
const {
  MAX_RATING,
  canImportPhotos,
  densityOptions,
  desktop,
  exportDialog,
  gridClass,
  gridStyle,
  isAmazon,
  loadResultsPage,
  metadataDialog,
  openBurstReview,
  openMoveDialog,
  openNextRoundDialog,
  openShareDialog,
  openZoom,
  ratingCount,
  restartDialog,
  resultsBusy,
  resultsDensity,
  resultsPhotos,
  resultsRating,
  resultsSort,
  resultsTotal,
  selectResultsRating,
  selectionSummary,
  sidecarAccess,
  view
} = useCurator()
</script>

<template>
    <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-3" @click="view = 'project'">プロジェクトへ戻る</v-btn>
    <h1 class="text-h5 text-md-h4">レーティング</h1>
    <p class="text-body-2 text-medium-emphasis mt-2">
      星を選ぶと、その星の写真だけを選別できます。選ばれた写真は星が1つ上がり、選ばれなかった写真はそのままです。
    </p>

    <v-chip v-if="sidecarAccess !== 'readwrite'" class="mt-3" size="small" variant="tonal" prepend-icon="mdi-note-off-outline">この端末だけの結果</v-chip>

    <div class="rating-board mt-6">
      <div
        v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
        class="rating-row" :class="{ 'is-active': resultsRating === rating }"
        role="button" tabindex="0"
        @click="selectResultsRating(resultsRating === rating ? null : rating)"
        @keydown.enter="selectResultsRating(resultsRating === rating ? null : rating)"
      >
        <span class="rating-row__stars">
          <v-icon v-for="star in MAX_RATING" :key="star" size="17" :icon="star <= rating ? 'mdi-star' : 'mdi-star-outline'" :class="star <= rating ? 'text-primary' : 'text-medium-emphasis'" />
        </span>
        <span class="rating-row__count">{{ ratingCount(rating).toLocaleString() }} 枚</span>
        <v-progress-linear
          class="rating-row__bar"
          :model-value="selectionSummary?.total ? (ratingCount(rating) / selectionSummary.total) * 100 : 0"
          :color="rating >= 4 ? 'primary' : 'secondary'" height="6" rounded
        />
        <span class="rating-row__actions">
          <v-btn
            size="small" variant="outlined" prepend-icon="mdi-tournament"
            :disabled="ratingCount(rating) < 2"
            @click.stop="openNextRoundDialog(rating)"
          >この {{ ratingCount(rating).toLocaleString() }} 枚を選別</v-btn>
          <v-btn
            size="small" variant="text" prepend-icon="mdi-swap-horizontal"
            :disabled="!ratingCount(rating)"
            @click.stop="openMoveDialog(rating)"
          >レートを移動</v-btn>
        </span>
      </div>
    </div>

    <div class="d-flex flex-wrap align-center ga-3 mt-6 mb-4">
      <v-btn-toggle v-model="resultsSort" density="comfortable" variant="outlined" divided mandatory @update:model-value="loadResultsPage(true)">
        <v-btn value="rating">星の高い順</v-btn>
        <v-btn value="name">ファイル名順</v-btn>
      </v-btn-toggle>
      <v-btn-toggle v-model="resultsDensity" density="comfortable" variant="outlined" divided mandatory>
        <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
      </v-btn-toggle>
      <v-chip v-if="resultsRating !== null" closable color="primary" variant="flat" @click:close="selectResultsRating(null)">
        ★{{ resultsRating }} だけ表示中
      </v-chip>
      <v-spacer />
      <!-- 選別中は「まとめの中から1枚」を決めていない。その1手をここで引き受ける。 -->
      <v-btn variant="outlined" prepend-icon="mdi-layers-triple-outline" @click="openBurstReview">連写を見直す</v-btn>
      <!-- デスクトップは原本のフォルダを直接操作できる。ブラウザはできないので、
           共有シートか星ごとの ZIP を通して渡す。 -->
      <template v-if="canImportPhotos">
        <v-btn variant="outlined" prepend-icon="mdi-export-variant" @click="openShareDialog">書き出す</v-btn>
      </template>
      <template v-else>
        <v-btn variant="outlined" prepend-icon="mdi-folder-move-outline" @click="exportDialog = true">フォルダ分け</v-btn>
        <!-- Amazon の写真の原本は書き換えられない。 -->
        <v-btn variant="outlined" prepend-icon="mdi-tag-text-outline" :disabled="isAmazon" @click="metadataDialog = true">メタデータに反映</v-btn>
        <span v-if="isAmazon" class="text-caption text-medium-emphasis">Amazon の写真には使えません</span>
      </template>
      <v-btn variant="text" prepend-icon="mdi-restart" @click="restartDialog = true">最初からやり直す</v-btn>
    </div>

    <div v-if="resultsPhotos.length" class="result-grid" :class="gridClass(resultsDensity)" :style="gridStyle(resultsDensity)">
      <div
        v-for="photo in resultsPhotos" :key="photo.id" class="result-tile"
        :class="{ 'is-confirmed': photo.rating >= MAX_RATING, 'is-eliminated': photo.rating === 0 }"
        role="button" tabindex="0"
        @click="openZoom(photo, resultsPhotos)" @keydown.enter="openZoom(photo, resultsPhotos)"
      >
        <img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy">
        <div class="result-tile__meta">
          <span class="result-tile__stars">
            <v-icon v-for="star in MAX_RATING" :key="star" size="13" :icon="star <= photo.rating ? 'mdi-star' : 'mdi-star-outline'" :class="star <= photo.rating ? 'text-primary' : 'text-medium-emphasis'" />
          </span>
        </div>
        <div class="result-tile__name text-caption">{{ photo.name }}</div>
      </div>
    </div>
    <v-card v-else-if="!resultsBusy" class="pa-10 text-center text-medium-emphasis">
      この条件に当てはまる写真はありません。
    </v-card>

    <div class="d-flex justify-center mt-6">
      <v-btn v-if="resultsPhotos.length < resultsTotal" variant="outlined" :loading="resultsBusy" @click="loadResultsPage()">
        さらに読み込む（{{ resultsPhotos.length.toLocaleString() }} / {{ resultsTotal.toLocaleString() }}）
      </v-btn>
      <span v-else-if="resultsPhotos.length" class="text-caption text-medium-emphasis">{{ resultsTotal.toLocaleString() }} 枚すべて表示しました</span>
    </div>
</template>
