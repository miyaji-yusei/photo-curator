<script setup lang="ts">
const {
  sidecarBusy,
  sidecarClash,
  sidecarMessage,
  resolveSidecarClash
} = useCurator()

// 端末の時計はずれるので、日時をそのまま見せる（新旧の判定には使わない）。
const formatDate = (value: number) =>
  new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium', timeStyle: 'short' }).format(value)
</script>

<template>
<v-dialog :model-value="!!sidecarClash" max-width="560" persistent>
  <v-card v-if="sidecarClash" title="別の端末の記録があります">
    <v-card-text class="pt-5">
      <p class="text-body-2 mb-4">
        この端末とは別に、<strong>{{ sidecarClash.theirsSummary.deviceName }}</strong> が進めた選別の記録が
        写真のフォルダにあります。写真ごとではなく、どちらか一方を選びます。選ぶまで選別は始められません。
      </p>
      <div class="d-flex flex-wrap ga-3">
        <v-card variant="outlined" class="flex-1-1" style="min-width: 220px">
          <v-card-text>
            <div class="text-caption text-medium-emphasis">この端末の結果</div>
            <div class="text-subtitle-1 font-weight-bold">{{ sidecarClash.mine.deviceName }}</div>
            <div class="text-body-2 mt-2">★1 以上 {{ sidecarClash.mine.starred.toLocaleString() }} 枚</div>
            <div class="text-body-2">ROUND {{ sidecarClash.mine.round }}</div>
            <div class="text-caption text-medium-emphasis">最終更新 {{ formatDate(sidecarClash.mine.updatedAt) }}</div>
          </v-card-text>
        </v-card>
        <v-card variant="outlined" class="flex-1-1" style="min-width: 220px">
          <v-card-text>
            <div class="text-caption text-medium-emphasis">NAS の記録</div>
            <div class="text-subtitle-1 font-weight-bold">{{ sidecarClash.theirsSummary.deviceName }}</div>
            <div class="text-body-2 mt-2">★1 以上 {{ sidecarClash.theirsSummary.starred.toLocaleString() }} 枚</div>
            <div class="text-body-2">ROUND {{ sidecarClash.theirsSummary.round }}</div>
            <div class="text-caption text-medium-emphasis">最終更新 {{ formatDate(sidecarClash.theirsSummary.updatedAt) }}</div>
          </v-card-text>
        </v-card>
      </div>
      <p class="text-caption text-medium-emphasis mt-4 mb-0">
        選ばなかった方は消さず、写真のフォルダの <code>.photo-curator/</code> に <code>catalog.＜端末の id の先頭 12 文字＞.json</code> として残します。
      </p>
      <v-alert v-if="sidecarMessage" type="error" variant="tonal" density="comfortable" class="mt-4">{{ sidecarMessage }}</v-alert>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('theirs')">NAS の記録を使う</v-btn>
      <v-btn color="primary" :loading="sidecarBusy" @click="resolveSidecarClash('mine')">この端末の結果を使う</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
