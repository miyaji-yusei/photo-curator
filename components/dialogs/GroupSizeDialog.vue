<script setup lang="ts">
import { SLIDESHOW_GROUP_SIZE } from '~/utils/groupSize'

const {
  applyGroupBursts,
  applyGroupSize,
  groupLimits,
  groupSizeDialog,
  pendingGroupSize,
  pendingSlideshow,
  session
} = useCurator()

/** 表示枚数を ±1 する（最小・最大で止める）。 */
function stepGroupSize(delta: number) {
  const next = Number(pendingGroupSize.value) + delta
  pendingGroupSize.value = Math.min(groupLimits.max, Math.max(groupLimits.min, next))
}

/** ←→↑↓ で ±1。スライダーと入力欄は自分の矢印キーを持つので、そこでは二重にしない。 */
function onArrowKey(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  if (target?.closest('[role="slider"], input, textarea')) return
  if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') stepGroupSize(-1)
  else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') stepGroupSize(1)
  else return
  event.preventDefault()
}
</script>

<template>
<!-- 選別の途中で1グループの表示枚数を変える。済んだぶんはそのまま残る。 -->
<v-dialog v-model="groupSizeDialog" max-width="520">
  <v-card title="選別中の設定">
    <v-card-text class="pt-5" @keydown="onArrowKey">
      <!-- 方式。スライドショーは 1 グループ 1 枚（セッションの枚数設定と同じ仕組み）。中断して再開したあとでも切り替えられる。 -->
      <div class="text-subtitle-2 mb-1">選別の方式</div>
      <v-btn-toggle
        :model-value="pendingSlideshow ? 'slideshow' : 'tournament'" mandatory divided density="comfortable" color="primary"
        class="mb-4" aria-label="選別の方式"
        @update:model-value="value => (pendingSlideshow = value === 'slideshow')"
      >
        <v-btn value="tournament" prepend-icon="mdi-view-grid-outline">トーナメント</v-btn>
        <v-btn value="slideshow" prepend-icon="mdi-image-outline">スライドショー</v-btn>
      </v-btn-toggle>
      <p v-if="pendingSlideshow" class="text-caption text-medium-emphasis mb-0">
        1 枚ずつ出して、残す（右）か落とす（左）かを決めます。上へのスワイプや、写真の上のほうのクリック・タップで★5 にして確定します。キーは 1・←＝落とす、3・→＝残す、5・↑＝★5 で確定です。写真のほぼ中心のクリック・タップは何もせず、二度押しで拡大します。
      </p>
      <template v-else>
      <div class="text-subtitle-2 mb-1">1グループの表示枚数</div>
      <v-slider v-model="pendingGroupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="1グループの表示枚数">
        <template #prepend>
          <v-btn icon="mdi-minus" size="small" variant="tonal" aria-label="1 枚減らす" :disabled="pendingGroupSize <= groupLimits.min" @click="stepGroupSize(-1)" />
        </template>
        <template #append>
          <v-btn icon="mdi-plus" size="small" variant="tonal" class="mr-2" aria-label="1 枚増やす" :disabled="pendingGroupSize >= groupLimits.max" @click="stepGroupSize(1)" /><v-text-field v-model.number="pendingGroupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template>
      </v-slider>
      <p class="text-caption text-medium-emphasis mt-3 mb-0">
        まだ見ていない写真だけを詰め直します。ここまでの選択と「1つ戻す」の履歴はそのまま残ります。
      </p>
      </template>
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
      <v-btn color="primary" @click="applyGroupSize(pendingSlideshow ? SLIDESHOW_GROUP_SIZE : pendingGroupSize)">{{ pendingSlideshow ? 'この方式にする' : 'この枚数にする' }}</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
