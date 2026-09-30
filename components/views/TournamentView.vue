<script setup lang="ts">
import type { Ref } from 'vue'
import type { SavedSelection } from '~/utils/selectionFlow'
import { gridFor, PHOTO_ASPECT, type GridShape } from '~/utils/gridFor'

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
  tournamentPhotos,
  undoChoice,
  view
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const session = nullableSession as Ref<SavedSelection>

// 並べ方（行×列）は、写真を置く枠の大きさから決める。枠（.tournament-stage）は高さを
// CSS で固定し、格子は中で絶対配置にしてある。中身が枠を押し広げないので、測った値が
// 並べ替えで変わり続けることはない。
const GRID_GAP = 10
const MEASURE_DELAY_MS = 100
const stage = ref<HTMLElement | null>(null)
const frame = ref({ width: 0, height: 0 })
const shape = ref<GridShape>({ rows: 1, cols: 1 })
// 写真の縦横比（横 / 縦）。組の写真が全部読み込めたら、その中央値に替える。読み込むまでは
// 直前の組の値（最初は 1.5）を使う。アルバムは向きがそろいやすく、組が替わっても並びが跳ねない。
const groupAspect = ref(PHOTO_ASPECT)
const measured = new Map<string, number>()
const failedIds = new Set<string>()
let aspectSettled = false
function recompute() {
  shape.value = gridFor(tournamentPhotos.value.length, frame.value.width, frame.value.height, GRID_GAP, shape.value, groupAspect.value)
}
function median(values: number[]) {
  const sorted = [...values].sort((a, b) => a - b)
  const mid = sorted.length >> 1
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2
}
// 全部（読み込みに失敗したものは除く）が測れたら、1 回だけ縦横比を確定して並べ直す。
function settleAspect() {
  if (aspectSettled) return
  const expected = tournamentPhotos.value.length - failedIds.size
  if (expected <= 0 || measured.size < expected) return
  aspectSettled = true
  groupAspect.value = median([...measured.values()])
  recompute()
}
function onPhotoLoad(id: string, img: HTMLImageElement | null) {
  if (!img || !img.naturalWidth || !img.naturalHeight) return
  measured.set(id, img.naturalWidth / img.naturalHeight)
  settleAspect()
}
function onPhotoError(id: string) {
  failedIds.add(id)
  settleAspect()
}
let measureTimer: ReturnType<typeof setTimeout> | null = null
let observer: ResizeObserver | null = null
function measure() {
  const el = stage.value
  if (!el) return
  frame.value = { width: el.clientWidth, height: el.clientHeight }
  recompute()
}
onMounted(() => {
  measure()
  if (typeof ResizeObserver === 'undefined' || !stage.value) return
  observer = new ResizeObserver(() => {
    if (measureTimer) clearTimeout(measureTimer)
    measureTimer = setTimeout(measure, MEASURE_DELAY_MS)
  })
  observer.observe(stage.value)
})
onBeforeUnmount(() => {
  observer?.disconnect()
  if (measureTimer) clearTimeout(measureTimer)
})
// 枚数が変わったら（表示枚数の変更・組の切り替わり）すぐに計算し直す。
watch(() => tournamentPhotos.value.length, recompute, { immediate: true })
// 組が替わったら測り直す（測るのは組につき 1 回）。描画済みの <img> が使い回されて @load が
// 来ない写真は、描画後に読み込み済みのものから拾う。
watch(
  () => tournamentPhotos.value.map(photo => photo.id).join('\n'),
  async () => {
    measured.clear()
    failedIds.clear()
    aspectSettled = false
    await nextTick()
    stage.value?.querySelectorAll<HTMLImageElement>('img[data-photo-id]').forEach(img => {
      if (img.complete && img.naturalWidth) onPhotoLoad(img.dataset.photoId ?? '', img)
    })
  },
  { immediate: true }
)

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

    <div ref="stage" class="tournament-stage">
    <div
      class="tournament-grid tournament-grid--fit"
      :style="{ '--tournament-columns': shape.cols, '--tournament-rows': shape.rows, '--tournament-gap': `${GRID_GAP}px` }"
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

        <img
          :src="desktop.photoDisplayUrl(photo)" :alt="photo.name" :data-photo-id="photo.id"
          @load="onPhotoLoad(photo.id, $event.target as HTMLImageElement)" @error="onPhotoError(photo.id)"
        >
      </v-card>
    </div>
    </div>
</template>
