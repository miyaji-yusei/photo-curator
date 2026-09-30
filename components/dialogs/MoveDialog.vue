<script setup lang="ts">
const {
  densityOptions,
  desktop,
  gridClass,
  gridStyle,
  isMoveSelected,
  loadMovePage,
  moveBusy,
  moveDensity,
  moveDialog,
  moveError,
  moveFrom,
  movePhotos,
  moveSelectedCount,
  moveTo,
  moveTotal,
  runMove,
  session,
  setMoveSelectAll,
  toggleMoveSelection
} = useCurator()
</script>

<template>
<!-- レートの移動。既定は全選択で、外したい写真だけチェックを解く。 -->
<v-dialog v-model="moveDialog" fullscreen transition="dialog-bottom-transition" scrollable>
  <v-card>
    <v-toolbar color="surface" density="comfortable">
      <v-toolbar-title>★{{ moveFrom }} の写真をどのレートへ移しますか？</v-toolbar-title>
      <v-spacer />
      <v-btn icon="mdi-close" aria-label="閉じる" @click="moveDialog = false" />
    </v-toolbar>
    <v-card-text class="pt-5">
      <v-alert v-if="moveError" type="error" density="compact" class="mb-4">{{ moveError }}</v-alert>
      <!-- 選別中に星を動かすと、進行中のラウンドの前提が変わる。 -->
      <v-alert v-if="session" type="warning" variant="tonal" density="compact" class="mb-4">
        選別が進行中です。星を動かすと、いま選別している対象と食い違うことがあります。
      </v-alert>

      <div class="d-flex flex-wrap align-center ga-4 mb-5">
        <div>
          <div class="text-caption text-medium-emphasis mb-1">移動先のレート</div>
          <v-btn-toggle v-model="moveTo" density="comfortable" variant="outlined" divided mandatory>
            <v-btn v-for="rating in [0, 1, 2, 3, 4, 5]" :key="rating" :value="rating" :disabled="rating === moveFrom">★{{ rating }}</v-btn>
          </v-btn-toggle>
        </div>
        <v-spacer />
        <div class="d-flex align-center ga-2">
          <v-btn size="small" variant="text" :disabled="moveSelectedCount >= moveTotal" @click="setMoveSelectAll(true)">全選択</v-btn>
          <v-btn size="small" variant="text" :disabled="!moveSelectedCount" @click="setMoveSelectAll(false)">全解除</v-btn>
          <v-btn-toggle v-model="moveDensity" density="comfortable" variant="outlined" divided mandatory>
            <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
          </v-btn-toggle>
        </div>
      </div>

      <div v-if="movePhotos.length" class="result-grid" :class="gridClass(moveDensity)" :style="gridStyle(moveDensity)">
        <div
          v-for="photo in movePhotos" :key="photo.id" class="result-tile"
          :class="{ 'is-unpicked': !isMoveSelected(photo.id) }"
          role="button" tabindex="0"
          @click="toggleMoveSelection(photo.id)" @keydown.enter="toggleMoveSelection(photo.id)"
        >
          <v-checkbox-btn class="result-tile__pick" :model-value="isMoveSelected(photo.id)" density="compact" :aria-label="`${photo.name} を移動対象にする`" @click.stop="toggleMoveSelection(photo.id)" />
          <img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy">
          <div class="result-tile__name text-caption">{{ photo.name }}</div>
        </div>
      </div>
      <v-card v-else-if="!moveBusy" class="pa-10 text-center text-medium-emphasis">★{{ moveFrom }} の写真はありません。</v-card>

      <div class="d-flex justify-center mt-5">
        <v-btn v-if="movePhotos.length < moveTotal" variant="outlined" :loading="moveBusy" @click="loadMovePage()">
          さらに読み込む（{{ movePhotos.length.toLocaleString() }} / {{ moveTotal.toLocaleString() }}）
        </v-btn>
        <span v-else-if="movePhotos.length" class="text-caption text-medium-emphasis">{{ moveTotal.toLocaleString() }} 枚すべて表示しました</span>
      </div>
      <!-- まだ読み込んでいない写真も移動の対象に入る。件数は総数から数える。 -->
      <p v-if="movePhotos.length < moveTotal" class="text-caption text-medium-emphasis text-center mt-2">
        表示していない写真も対象に含まれます。外したい写真だけを読み込んでチェックを外してください。
      </p>
    </v-card-text>
    <v-divider />
    <v-card-actions class="pa-4">
      <div class="text-body-2">
        <strong>{{ moveSelectedCount.toLocaleString() }} 枚</strong> を ★{{ moveFrom }} → ★{{ moveTo }} へ移します
      </div>
      <v-spacer />
      <v-btn variant="text" @click="moveDialog = false">やめる</v-btn>
      <v-btn color="primary" :loading="moveBusy" :disabled="!moveSelectedCount || moveFrom === moveTo" @click="runMove">移動する</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
