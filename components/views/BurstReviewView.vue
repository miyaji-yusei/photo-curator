<script setup lang="ts">
const {
  applyBurstReview,
  burstReviewBusy,
  burstReviewColumns,
  burstReviewGroups,
  burstReviewIndex,
  burstReviewKept,
  burstReviewLoaded,
  burstReviewPhotos,
  burstReviewRows,
  desktop,
  openResults,
  openZoom,
  skipBurstReview,
  toggleBurstReviewKeep
} = useCurator()
</script>

<template>
    <div class="d-flex flex-wrap align-center justify-space-between ga-3 mb-3">
      <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="openResults">レーティングへ戻る</v-btn>
      <span v-if="burstReviewGroups.length" class="text-caption text-medium-emphasis">
        {{ (burstReviewIndex + 1).toLocaleString() }} / {{ burstReviewGroups.length.toLocaleString() }} グループ
      </span>
    </div>

    <template v-if="burstReviewGroups.length && burstReviewPhotos.length">
      <h1 class="text-h6 text-md-h5">まとめられた {{ burstReviewPhotos.length }} 枚から残す写真を選ぶ</h1>
      <p class="text-body-2 text-medium-emphasis mt-1 mb-3">
        残した写真は星が1つ上がり、外した写真は1つ下がります。
      </p>
      <v-progress-linear
        :model-value="burstReviewGroups.length ? (burstReviewIndex / burstReviewGroups.length) * 100 : 100"
        color="primary" height="6" rounded class="mb-4"
      />

      <div
        class="tournament-grid"
        :style="{ '--tournament-columns': burstReviewColumns, '--tournament-rows': burstReviewRows }"
      >
        <v-card
          v-for="(photo, index) in burstReviewPhotos"
          :key="photo.id"
          class="tournament-card"
          :class="{ 'is-selected': burstReviewKept.includes(photo.id) }"
          @click="toggleBurstReviewKeep(photo.id)"
        >
          <span class="tournament-card__number">{{ index + 1 === 10 ? 0 : index + 1 }}</span>
          <div class="tournament-card__tools">
            <v-btn
              icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
              :aria-label="`${photo.name} を拡大`"
              @click.stop="openZoom(photo, burstReviewPhotos)"
            />
          </div>
          <span v-if="burstReviewKept.includes(photo.id)" class="tournament-card__check">
            <v-icon icon="mdi-check-bold" size="20" />
          </span>
          <span class="tournament-card__confirmed">★{{ photo.rating }}</span>
          <img :src="desktop.photoDisplayUrl(photo)" :alt="photo.name">
        </v-card>
      </div>

      <v-sheet class="d-flex align-center justify-space-between flex-wrap ga-3 mt-4 pa-3" color="surface-variant" rounded>
        <span class="text-caption text-medium-emphasis">
          残す {{ burstReviewKept.length }} 枚 ・ 下げる {{ burstReviewPhotos.length - burstReviewKept.length }} 枚
        </span>
        <div class="d-flex flex-wrap ga-2">
          <v-btn variant="outlined" :disabled="burstReviewBusy" @click="skipBurstReview">変更しない</v-btn>
          <v-btn
            color="primary" :loading="burstReviewBusy" :disabled="!burstReviewKept.length"
            @click="applyBurstReview"
          >この {{ burstReviewKept.length }} 枚を残す</v-btn>
        </div>
      </v-sheet>
    </template>

    <v-card v-else-if="burstReviewLoaded" class="pa-10 text-center text-medium-emphasis">
      まとめられた連写はありません。
    </v-card>
</template>
