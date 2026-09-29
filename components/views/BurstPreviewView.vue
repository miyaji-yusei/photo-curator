<script setup lang="ts">
const {
  acceptBurstThreshold,
  askMorePairs,
  askedCount,
  burstGroupCount,
  groupedPhotoCount,
  maxPairDistance,
  previewBusy,
  previewThreshold,
  refreshBurstPreview
} = useCurator()
</script>

<template>
    <div class="text-overline text-primary">連写のまとめ方</div>
    <h1 class="text-h5 text-md-h4">{{ burstGroupCount.toLocaleString() }} グループ / {{ groupedPhotoCount.toLocaleString() }} 枚にまとまりました</h1>
    <p class="text-body-2 text-medium-emphasis mt-2">
      {{ askedCount }} 問の回答から、まとめる基準を {{ previewThreshold }} と判断しました。ここで微調整できます。
    </p>

    <v-card max-width="760" class="mt-6 pa-6 settings-card">
      <div class="text-subtitle-2 mb-4">まとめる基準（小さいほど厳しく、似た写真だけをまとめます）</div>
      <v-slider
        class="selection-slider"
        :model-value="previewThreshold"
        :min="0"
        :max="maxPairDistance"
        :step="1"
        thumb-label
        :disabled="previewBusy"
        aria-label="まとめる基準"
        @update:model-value="refreshBurstPreview($event as number)"
      />
      <div class="d-flex justify-space-between text-caption text-medium-emphasis">
        <span>厳しく（まとまりにくい）</span><span>ゆるく（まとまりやすい）</span>
      </div>
      <v-divider class="my-6" />
      <div class="d-flex flex-wrap ga-3 justify-end">
        <v-btn variant="outlined" :disabled="previewBusy" @click="askMorePairs">もう少し質問する</v-btn>
        <v-btn color="primary" size="large" prepend-icon="mdi-play" :loading="previewBusy" @click="acceptBurstThreshold">この設定で選別を始める</v-btn>
      </div>
    </v-card>
</template>
