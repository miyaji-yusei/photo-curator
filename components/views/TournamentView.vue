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
  selectedCount,
  session: nullableSession,
  skipGroup,
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
</script>

<template>
    <div class="d-flex align-center justify-space-between flex-wrap ga-3 mb-3">
      <div>
        <div class="text-overline text-primary">★{{ targetStar }} を選別中 &middot; Round {{ roundNumber }}</div>
        <h1 class="text-h6 text-md-h5">残り {{ remainingPhotos.toLocaleString() }} 枚 / {{ remainingGroups.toLocaleString() }} グループ</h1>
      </div>
      <div class="d-flex flex-wrap ga-2">
        <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!canUndo" @click="undoChoice">1つ戻す<span class="ms-1 text-caption">⌫</span></v-btn>
        <v-btn variant="text" prepend-icon="mdi-view-grid-outline" @click="openGroupSizeDialog">表示枚数</v-btn>
        <v-btn variant="text" prepend-icon="mdi-pause-circle-outline" @click="view = 'project'">中断して戻る</v-btn>
      </div>
    </div>
    <v-progress-linear :model-value="roundProgress" color="primary" height="6" rounded class="mb-2" />
    <div class="d-flex justify-space-between text-caption text-medium-emphasis mb-4"><span>選択済み {{ selectedCount }} 枚</span><span>このグループ {{ tournamentPhotos.length }} 枚</span></div>

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

    <v-sheet class="d-flex align-center justify-space-between flex-wrap ga-3 mt-4 pa-3" color="surface-variant" rounded>
      <v-checkbox v-model="session.multiSelect" density="compact" hide-details label="複数枚選択（M）" @update:model-value="session.selectedInGroup = []; saveSession()" />
      <!-- キーボードが無い環境では案内しない。操作はカード上のボタンで完結する。 -->
      <div v-if="!isTouchOnly" class="text-caption text-medium-emphasis">
        1〜0 選ぶ ・ Ctrl+数字 拡大 ・ Shift+数字 ★{{ MAX_RATING }}で確定 ・ Alt+数字 まとめを開く ・ Enter 決定 ・ ⌫ 戻す
      </div>
      <div class="d-flex flex-wrap ga-2">
        <v-btn variant="outlined" @click="skipGroup">どれも選ばない</v-btn>
        <v-btn color="primary" @click="confirmChoices">
          {{ session.selectedInGroup.length ? `${session.selectedInGroup.length} 枚を選択` : '選択なしで次へ' }}（Enter）
        </v-btn>
      </div>
    </v-sheet>
</template>
