<script setup lang="ts">
const {
  activeProject,
  applyDisplayEdge,
  beginTournament,
  displayBusy,
  displayEdge,
  displaySettings,
  groupLimits,
  relearnThreshold,
  settings,
  view
} = useCurator()
</script>

<template>
    <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'method'">方法の選択へ戻る</v-btn><div class="text-overline text-primary">Tournament setup</div><h1 class="text-h4 mb-6">トーナメントの設定</h1>
    <v-card max-width="720" class="pa-6 settings-card"><div class="text-subtitle-1 font-weight-medium mb-5">何枚から選びますか？</div><v-slider v-model="settings.groupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="何枚から選ぶか"><template #append><v-text-field v-model.number="settings.groupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template></v-slider><p class="text-caption text-medium-emphasis mt-2">少ないほど比較は丁寧に、多いほどテンポよく進みます。</p><v-divider class="my-7" /><v-switch v-model="settings.groupBursts" color="primary" label="事前にバースト写真（連写）をまとめる" hint="最大 8 問だけ答えると、残りは同じ基準で自動的にまとまります。" persistent-hint />
      <v-alert v-if="settings.groupBursts && activeProject?.burstThreshold !== null && activeProject?.burstThreshold !== undefined" type="info" variant="tonal" density="comfortable" class="mt-4">
        <div class="d-flex align-center justify-space-between flex-wrap ga-3">
          <span>このプロジェクトは学習済みです（基準 {{ activeProject.burstThreshold }}）。質問は出ません。</span>
          <v-btn size="small" variant="outlined" @click="relearnThreshold">学習し直す</v-btn>
        </div>
      </v-alert>
      <template v-if="displaySettings && displaySettings.choices.length > 1">
        <v-divider class="my-7" />
        <div class="text-subtitle-1 font-weight-medium mb-2">選別に出す画像の大きさ</div>
        <p class="text-caption text-medium-emphasis mb-4">
          ここで選ぶとこのプロジェクトに適用されます。長辺の画素数です。
        </p>
        <v-btn-toggle
          :model-value="displayEdge" density="comfortable" variant="outlined" divided mandatory
          @update:model-value="applyDisplayEdge($event as number)"
        >
          <v-btn v-for="choice in displaySettings.choices" :key="choice" :value="choice" :disabled="displayBusy">
            {{ choice }}
          </v-btn>
        </v-btn-toggle>
        <p class="text-caption text-medium-emphasis mt-3 mb-0">
          既定は {{ displaySettings.defaultEdge }}px。2 枚を並べて見比べるときは {{ displaySettings.largeEdge }}px 以上あると
          引き伸ばされません（実機計測）。
        </p>
      </template>
      <div class="d-flex justify-end mt-8"><v-btn color="primary" size="large" prepend-icon="mdi-play" @click="beginTournament">選別を開始</v-btn></div></v-card>
</template>
