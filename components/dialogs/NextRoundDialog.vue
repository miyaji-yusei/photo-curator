<script setup lang="ts">
const {
  MAX_RATING,
  groupLimits,
  nextRoundDialog,
  nextRoundGroupSize,
  nextRoundRating,
  ratingCount,
  startRatingSelection
} = useCurator()
</script>

<template>
<!-- 選別対象は星で決まる。枚数だけここで決める。 -->
<v-dialog v-model="nextRoundDialog" max-width="520">
  <v-card :title="`★${nextRoundRating} を選別`">
    <v-card-text class="pt-5">
      <p class="text-body-2 mb-5">
        ★{{ nextRoundRating }} の <strong>{{ ratingCount(nextRoundRating).toLocaleString() }} 枚</strong>が対象です。
        選ばれた写真は ★{{ Math.min(MAX_RATING, nextRoundRating + 1) }} に上がり、選ばれなかった写真は ★{{ nextRoundRating }} のまま残ります。
      </p>
      <v-slider v-model="nextRoundGroupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="1グループの表示枚数">
        <template #append><v-text-field v-model.number="nextRoundGroupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template>
      </v-slider>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" @click="nextRoundDialog = false">キャンセル</v-btn>
      <v-btn color="primary" prepend-icon="mdi-play" @click="startRatingSelection(nextRoundRating)">始める</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
