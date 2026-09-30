<script setup lang="ts">
const {
  applyGroupBursts,
  applyGroupSize,
  groupLimits,
  groupSizeDialog,
  pendingGroupSize,
  session
} = useCurator()
</script>

<template>
<!-- 選別の途中で1グループの表示枚数を変える。済んだぶんはそのまま残る。 -->
<v-dialog v-model="groupSizeDialog" max-width="520">
  <v-card title="選別中の設定">
    <v-card-text class="pt-5">
      <div class="text-subtitle-2 mb-1">1グループの表示枚数</div>
      <v-slider v-model="pendingGroupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="1グループの表示枚数">
        <template #append><v-text-field v-model.number="pendingGroupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template>
      </v-slider>
      <p class="text-caption text-medium-emphasis mt-3 mb-0">
        まだ見ていない写真だけを詰め直します。ここまでの選択と「1つ戻す」の履歴はそのまま残ります。
      </p>
      <v-divider class="my-4" />
      <!-- 切り替えるとその場で今の組に効く（まだ判断していない写真だけ組み直す）。 -->
      <v-switch
        :model-value="!!session?.settings.groupBursts" color="primary" hide-details density="comfortable"
        label="連写をまとめる"
        @update:model-value="value => applyGroupBursts(!!value)"
      />
      <p class="text-caption text-medium-emphasis mt-1 mb-0">
        切り替えるとすぐ今の組に効きます。済んだ組はそのまま残ります。
      </p>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" @click="groupSizeDialog = false">キャンセル</v-btn>
      <v-btn color="primary" @click="applyGroupSize(pendingGroupSize)">この枚数にする</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
