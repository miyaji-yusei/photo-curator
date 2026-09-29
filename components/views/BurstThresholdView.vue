<script setup lang="ts">
const {
  answerPair,
  currentPair,
  desktop,
  learningPosition,
  learningTotal,
  pairPhotos,
  skipCurrentPair,
  view
} = useCurator()
</script>

<template>
    <div class="d-flex align-center justify-space-between flex-wrap ga-3 mb-2">
      <div>
        <div class="text-overline text-primary">連写のまとめ方を学習中 &middot; {{ learningPosition }} / {{ learningTotal }} 問</div>
        <h1 class="text-h5 text-md-h4">この2枚は同じ連写ですか？</h1>
      </div>
      <v-btn variant="text" prepend-icon="mdi-pause-circle-outline" @click="view = 'project'">中断して戻る</v-btn>
    </div>
    <p class="text-body-2 text-medium-emphasis mb-5">
      数問だけ答えると、残りは同じ基準で自動的にまとまります。全部を確認する必要はありません。
    </p>
    <v-progress-linear :model-value="((learningPosition - 1) / Math.max(1, learningTotal)) * 100" color="primary" height="6" rounded class="mb-6" />

    <template v-if="currentPair && pairPhotos.length === 2">
      <div class="compare-pair">
        <figure v-for="photo in pairPhotos" :key="photo.id" class="compare-pair__item">
          <img :src="desktop.photoDisplayUrl(photo)" :alt="photo.name">
          <figcaption class="text-caption text-medium-emphasis mt-2 text-truncate">{{ photo.name }}</figcaption>
        </figure>
      </div>
      <div class="text-caption text-medium-emphasis mt-4 text-center">
        撮影間隔 {{ (currentPair.gapMs / 1000).toFixed(1) }} 秒
      </div>
      <!-- 主操作を右端（縦並びでは最下部）に置く。まとめる／別々は二分探索の
           性質上ほぼ半々で押されるので、隣り合わせにしておく。 -->
      <div class="compare-actions mt-5">
        <v-btn size="large" variant="text" @click="skipCurrentPair">判断できない<span class="ms-2 text-caption">1</span></v-btn>
        <v-btn size="large" variant="outlined" @click="answerPair(false)">別々に扱う<span class="ms-2 text-caption">2</span></v-btn>
        <v-btn size="large" color="primary" prepend-icon="mdi-image-album" @click="answerPair(true)">まとめる<span class="ms-2 text-caption">3</span></v-btn>
      </div>
    </template>

    <v-card v-else class="pa-8 text-center">
      <v-progress-circular indeterminate color="primary" class="mb-4" />
      <div class="text-body-1">連写の候補を準備しています…</div>
      <p class="text-caption text-medium-emphasis mt-2 mb-0">
        解析が進むと候補が増えます。そのまま少しお待ちください。
      </p>
    </v-card>
</template>
