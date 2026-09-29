<script setup lang="ts">
import type { Ref } from 'vue'
import type { SelectionSession } from '~/types/photo'

const {
  MAX_RATING,
  openNextRoundDialog,
  openResults,
  ratingCount,
  session: nullableSession,
  undoChoice,
  view
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const session = nullableSession as Ref<SelectionSession>
</script>

<template>
    <div class="text-overline text-primary">★{{ session.targetRating }} の選別が終わりました</div>
    <h1 class="text-h5 text-md-h4">{{ session.survivors.length.toLocaleString() }} 枚が ★{{ Math.min(MAX_RATING, session.targetRating + 1) }} に上がりました</h1>
    <p class="text-medium-emphasis mt-2">
      選ばれなかった写真は ★{{ session.targetRating }} のまま残っています。次はどの星を選別するか、レーティング画面から選べます。
    </p>
    <v-alert v-if="!session.survivors.length" type="warning" variant="tonal" class="mt-5" max-width="680">
      1枚も選ばれませんでした。厳しく見すぎた場合は「1つ戻す」で直前のグループからやり直せます。
    </v-alert>
    <v-card max-width="720" class="mt-6 pa-6">
      <div class="text-body-1 mb-5">
        ★{{ Math.min(MAX_RATING, session.targetRating + 1) }}: <strong>{{ ratingCount(Math.min(MAX_RATING, session.targetRating + 1)).toLocaleString() }} 枚</strong>
        ／ ★{{ session.targetRating }}: <strong>{{ ratingCount(session.targetRating).toLocaleString() }} 枚</strong>
      </div>
      <div class="d-flex flex-wrap ga-3">
        <v-btn
          color="primary" size="large" prepend-icon="mdi-tournament"
          :disabled="ratingCount(Math.min(MAX_RATING, session.targetRating + 1)) < 2"
          @click="openNextRoundDialog(Math.min(MAX_RATING, session.targetRating + 1))"
        >★{{ Math.min(MAX_RATING, session.targetRating + 1) }} をさらに選別</v-btn>
        <v-btn
          variant="outlined" size="large" prepend-icon="mdi-refresh"
          :disabled="ratingCount(session.targetRating) < 2"
          @click="openNextRoundDialog(session.targetRating)"
        >★{{ session.targetRating }} をもう一度見直す</v-btn>
      </div>
      <div class="d-flex flex-wrap ga-3 mt-4">
        <v-btn variant="text" prepend-icon="mdi-star-outline" @click="openResults">レーティングを見る</v-btn>
        <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!session.history.length" @click="undoChoice">1つ戻す</v-btn>
        <v-btn variant="text" prepend-icon="mdi-check" @click="view = 'project'">選別を終了する</v-btn>
      </div>
    </v-card>
</template>
