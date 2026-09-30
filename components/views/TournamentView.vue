<script setup lang="ts">
import type { Ref } from 'vue'
import type { SavedSelection } from '~/utils/selectionFlow'

const {
  MAX_RATING,
  burstSizeOf,
  canUndo,
  confirmChoices,
  confirmPhoto,
  desktop,
  groupSelectedAsBurst,
  isConfirmed,
  isTouchOnly,
  openBurst,
  openGroupSizeDialog,
  openZoom,
  remainingGroups,
  remainingPhotos,
  roundNumber,
  roundProgress,
  saveSession,
  session: nullableSession,
  targetStar,
  toggleChoice,
  tournamentColumns,
  tournamentPhotos,
  tournamentRows,
  undoChoice,
  view
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const session = nullableSession as Ref<SavedSelection>

function toggleMultiSelect() {
  session.value.multiSelect = !session.value.multiSelect
  session.value.selectedInGroup = []
  saveSession()
}
function clearSelection() {
  session.value.selectedInGroup = []
  saveSession()
}
</script>

<template>
    <!-- 上のバー。Android の Cull.kt の並び（左: 中断・1 つ戻す・複数選択 / 真ん中: 状況 /
         右: 設定・主ボタン）に合わせる。以前は下にあったボタンもここへ集め、写真を下端まで使う。 -->
    <div class="tournament-bar d-flex align-center ga-2">
      <div class="d-flex align-center ga-1 tournament-bar__side">
        <v-btn variant="text" prepend-icon="mdi-pause" @click="view = 'project'">中断して戻る</v-btn>
        <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!canUndo" @click="undoChoice">1つ戻す</v-btn>
        <v-btn
          :variant="session.multiSelect ? 'tonal' : 'text'"
          :color="session.multiSelect ? 'primary' : undefined"
          :prepend-icon="session.multiSelect ? 'mdi-check' : 'mdi-checkbox-multiple-outline'"
          :aria-pressed="session.multiSelect"
          aria-label="複数枚選択（M）"
          @click="toggleMultiSelect"
        >複数選択</v-btn>
        <v-btn
          v-if="session.multiSelect && session.selectedInGroup.length"
          variant="text" size="small" @click="clearSelection"
        >解除</v-btn>
      </div>

      <div class="tournament-bar__center text-center">
        <div class="text-overline text-primary">★{{ targetStar }} を選別中 &middot; ROUND {{ roundNumber }}</div>
        <div class="text-body-2">残り {{ remainingPhotos.toLocaleString() }} 枚 / {{ remainingGroups.toLocaleString() }} グループ</div>
      </div>

      <div class="d-flex align-center justify-end ga-2 tournament-bar__side">
        <!-- 複数選択で 2 枚以上選んだときだけ。「連写をまとめる」がオフだと手直しが効かないので押せない。 -->
        <v-btn
          v-if="session.multiSelect && session.selectedInGroup.length >= 2"
          variant="outlined" prepend-icon="mdi-layers-triple-outline"
          :disabled="!session.settings.groupBursts"
          :title="session.settings.groupBursts ? undefined : '「表示枚数」の設定で「連写をまとめる」をオンにすると使えます'"
          @click="groupSelectedAsBurst"
        >この写真をまとめる</v-btn>
        <v-btn icon="mdi-dots-horizontal" variant="text" aria-label="選別中の設定（表示枚数）" title="選別中の設定（表示枚数）" @click="openGroupSizeDialog" />
        <v-btn color="primary" @click="confirmChoices">
          {{ session.selectedInGroup.length ? `${session.selectedInGroup.length} 枚を選択` : '選択なしで次へ' }}<span v-if="!isTouchOnly" class="ms-1 text-caption">（Enter）</span>
        </v-btn>
      </div>
    </div>
    <v-progress-linear :model-value="roundProgress" color="primary" height="4" rounded class="mb-3" />

    <div
      class="tournament-grid"
      :style="{ '--tournament-columns': tournamentColumns, '--tournament-rows': tournamentRows }"
    >
      <v-card
        v-for="(photo, index) in tournamentPhotos"
        :key="photo.id"
        class="tournament-card"
        :class="{
          'is-selected': session.selectedInGroup.includes(photo.relativePath),
          'is-confirmed': isConfirmed(photo.id),
          'is-burst': burstSizeOf(photo.id) > 1
        }"
        @click="toggleChoice(photo.id)"
      >
        <span class="tournament-card__number">{{ index + 1 === 10 ? 0 : index + 1 }}</span>

        <!-- 選択のクリックと切り分けるため、拡大と確定は右上に置く。 -->
        <div class="tournament-card__tools">
          <!-- 迷う必要のない1枚を、その場で★5にして以降の判定から外す。
               複数枚選択中はトグルなので、確定済みでも押せるように出し続ける
               （もう一度押すと確定を外せる）。単数選択では確定した時点で次へ
               進むため、確定済みの表示は残らない。 -->
          <v-btn
            v-if="!isConfirmed(photo.id) || session.multiSelect"
            :icon="isConfirmed(photo.id) ? 'mdi-star' : 'mdi-star-outline'"
            size="x-small" variant="flat"
            :color="isConfirmed(photo.id) ? 'secondary' : undefined"
            :aria-label="isConfirmed(photo.id) ? `${photo.name} の★${MAX_RATING}確定を外す` : `${photo.name} を★${MAX_RATING} で確定`"
            @click.stop="confirmPhoto(photo.id)"
          />
          <v-btn
            icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
            :aria-label="`${photo.name} を拡大`"
            @click.stop="openZoom(photo, tournamentPhotos)"
          />
        </div>

        <!-- 連写の表示は左下の1か所だけ。ここ自体がまとめを開くボタン。
             以前は右上にも同じ操作があり、左下は押しても開かず選択されていた。 -->
        <button
          v-if="burstSizeOf(photo.id) > 1"
          type="button" class="tournament-card__stackmark"
          :aria-label="`まとめられた ${burstSizeOf(photo.id)} 枚を開く`"
          @click.stop="openBurst(photo)"
        >
          <v-icon icon="mdi-layers-triple-outline" size="16" />
          連写 {{ burstSizeOf(photo.id) }} 枚
          <v-icon icon="mdi-chevron-right" size="16" />
        </button>

        <span v-if="session.selectedInGroup.includes(photo.relativePath)" class="tournament-card__check">
          <v-icon icon="mdi-check-bold" size="20" />
        </span>
        <span v-if="isConfirmed(photo.id)" class="tournament-card__confirmed">確定</span>

        <img :src="desktop.photoDisplayUrl(photo)" :alt="photo.name">
      </v-card>
    </div>
</template>
