<script setup lang="ts">
const {
  MAX_RATING,
  canUndo,
  openNextRoundDialog,
  openResults,
  ratingCount,
  survivorCount,
  targetStar,
  undoChoice,
  view
} = useCurator()
</script>

<template>
    <div class="text-overline text-primary">★{{ targetStar }} の選別が終わりました</div>
    <h1 class="text-h5 text-md-h4">{{ survivorCount.toLocaleString() }} 枚が ★{{ Math.min(MAX_RATING, targetStar + 1) }} に上がりました</h1>
    <p class="text-medium-emphasis mt-2">
      選ばれなかった写真は ★{{ targetStar }} のまま残っています。次はどの星を選別するか、レーティング画面から選べます。
    </p>
    <v-alert v-if="!survivorCount" type="warning" variant="tonal" class="mt-5" max-width="680">
      1枚も選ばれませんでした。厳しく見すぎた場合は「1つ戻す」で直前のグループからやり直せます。
    </v-alert>
    <v-card max-width="720" class="mt-6 pa-6">
      <div class="text-body-1 mb-5">
        ★{{ Math.min(MAX_RATING, targetStar + 1) }}: <strong>{{ ratingCount(Math.min(MAX_RATING, targetStar + 1)).toLocaleString() }} 枚</strong>
        ／ ★{{ targetStar }}: <strong>{{ ratingCount(targetStar).toLocaleString() }} 枚</strong>
      </div>
      <div class="d-flex flex-wrap ga-3">
        <v-btn
          color="primary" size="large" prepend-icon="mdi-tournament"
          :disabled="ratingCount(Math.min(MAX_RATING, targetStar + 1)) < 2"
          @click="openNextRoundDialog(Math.min(MAX_RATING, targetStar + 1))"
        >★{{ Math.min(MAX_RATING, targetStar + 1) }} をさらに選別</v-btn>
        <v-btn
          variant="outlined" size="large" prepend-icon="mdi-refresh"
          :disabled="ratingCount(targetStar) < 2"
          @click="openNextRoundDialog(targetStar)"
        >★{{ targetStar }} をもう一度見直す</v-btn>
      </div>
      <div class="d-flex flex-wrap ga-3 mt-4">
        <v-btn variant="text" prepend-icon="mdi-star-outline" @click="openResults">レーティングを見る</v-btn>
        <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!canUndo" @click="undoChoice">1つ戻す</v-btn>
        <v-btn variant="text" prepend-icon="mdi-check" @click="view = 'project'">選別を終了する</v-btn>
      </div>
    </v-card>
</template>
