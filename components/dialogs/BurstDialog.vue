<script setup lang="ts">
const {
  MAX_RATING,
  applyBurstShape,
  burstBlocks,
  burstBusy,
  burstCuts,
  burstDialog,
  burstOriginal,
  burstPhotoOf,
  burstPhotos,
  burstPicked,
  confirmBurstPhoto,
  desktop,
  dropBurstPhoto,
  makeBurstRepresentative,
  moveBurstCut,
  openZoom,
  representativeOf,
  scatterBurst,
  splitBurstSelection,
  toggleBurstCut,
  toggleBurstPick
} = useCurator()

// まとまりの色は「まとまりの並び順」で決める（保存しない）。1 枚だけの塊は色を持たず、
// 巡回も進めない。Android の BurstEditSheet と同じ。
const PALETTE = ['#d6ff73', '#a7c8ff', '#ffb3e6']
const NEUTRAL = '#5f6572'

const layout = computed(() => {
  const rows: { id: string; block: string[]; tint: string | null }[] = []
  let turn = 0
  for (const block of burstBlocks.value) {
    const tint = block.length > 1 ? PALETTE[turn++ % PALETTE.length]! : null
    for (const id of block) rows.push({ id, block, tint })
  }
  return rows
})
const groupedCount = computed(() => burstBlocks.value.filter(block => block.length > 1).length)
const singleCount = computed(() => burstBlocks.value.filter(block => block.length === 1).length)

// ---- バーの操作 ----------------------------------------------------------
// タップ（ほぼ動かさずに放す）＝切る／つなぐ。ドラッグ＝切れているバーを隣の境目へ。
// 放したとき、いちばん近い境目に収まる。つながっているバーは動かさない。
const strip = ref<HTMLElement | null>(null)
const drag = reactive({ index: -1, startX: 0, dx: 0, moved: false, stride: 0 })
const TAP_SLOP = 5

function barStride(): number {
  const bars = strip.value?.querySelectorAll<HTMLElement>('.burst-bar')
  if (!bars || bars.length < 2) return 0
  return bars[1]!.offsetLeft - bars[0]!.offsetLeft
}

function onBarDown(index: number, event: PointerEvent) {
  if (event.button !== 0) return
  drag.index = index
  drag.startX = event.clientX
  drag.dx = 0
  drag.moved = false
  drag.stride = barStride()
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
}

function onBarMove(index: number, event: PointerEvent) {
  if (drag.index !== index) return
  const dx = event.clientX - drag.startX
  if (Math.abs(dx) > TAP_SLOP) drag.moved = true
  // 動かせるのは切れているバーだけ。つながっているバーは付いてこない。
  drag.dx = drag.moved && burstCuts.value[index] ? dx : 0
}

function onBarUp(index: number, event: PointerEvent) {
  if (drag.index !== index) return
  const target = event.currentTarget as HTMLElement
  if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId)
  const moved = drag.moved
  const steps = drag.stride > 0 ? Math.round(drag.dx / drag.stride) : 0
  drag.index = -1
  drag.dx = 0
  if (!moved) toggleBurstCut(index)
  else if (steps !== 0) moveBurstCut(index, index + steps)
}

function onBarCancel() {
  drag.index = -1
  drag.dx = 0
}

function onBarKey(index: number, event: KeyboardEvent) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault()
    toggleBurstCut(index)
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
    event.preventDefault()
    moveBurstCut(index, index + (event.key === 'ArrowRight' ? 1 : -1))
  }
}

const barLabel = (index: number) =>
  `${index + 1} 枚目と ${index + 2} 枚目の境目（${burstCuts.value[index] ? '切れている' : 'つながっている'}）`
const barOffset = (index: number) =>
  drag.index === index && drag.dx ? { transform: `translateX(${drag.dx}px)` } : undefined
</script>

<template>
<!-- まとまりの編集。写真を撮影順に横 1 列に並べ、写真のあいだの縦のバー（境目）で切る・つなぐ・ずらす。
     Android の BurstEditSheet と同じ操作。代表・星・拡大はここで決められる。 -->
<v-dialog v-model="burstDialog" fullscreen transition="dialog-bottom-transition" scrollable>
  <v-card>
    <v-toolbar color="surface" density="comfortable">
      <v-toolbar-title>まとまりを編集（元の連写 {{ burstOriginal.length }} 枚）</v-toolbar-title>
      <v-spacer />
      <v-btn icon="mdi-close" aria-label="閉じる" @click="burstDialog = false" />
    </v-toolbar>
    <v-card-text class="pt-5">
      <p class="text-body-2 text-medium-emphasis mb-4">
        写真のあいだの縦のバーをタップすると、そこで切る・つなぐができます。切れているバーは
        左右にドラッグして隣の境目へずらせます。薄い写真はまとまりに入っていない近くの写真で、
        つなぐと取り込めます。
      </p>

      <v-progress-linear v-if="burstBusy" indeterminate color="primary" class="mb-4" />

      <div ref="strip" class="burst-strip">
        <template v-for="(row, index) in layout" :key="row.id">
          <!-- 写真のあいだのバー。切れている＝太い色の線と丸いつまみ、つながっている＝細い灰色の線。 -->
          <div
            v-if="index > 0"
            class="burst-bar"
            :class="{ 'is-cut': burstCuts[index - 1], 'is-dragging': drag.index === index - 1 && drag.moved }"
            role="button"
            tabindex="0"
            :aria-label="barLabel(index - 1)"
            @pointerdown="onBarDown(index - 1, $event)"
            @pointermove="onBarMove(index - 1, $event)"
            @pointerup="onBarUp(index - 1, $event)"
            @pointercancel="onBarCancel"
            @keydown="onBarKey(index - 1, $event)"
          >
            <span
              class="burst-bar__line"
              :style="{ background: burstCuts[index - 1] ? (row.tint ?? NEUTRAL) : NEUTRAL, ...barOffset(index - 1) }"
            />
            <span
              v-if="burstCuts[index - 1]"
              class="burst-bar__knob"
              :style="{ background: row.tint ?? NEUTRAL, ...barOffset(index - 1) }"
            >
              <v-icon icon="mdi-drag-vertical" size="22" color="black" />
            </span>
          </div>

          <v-card
            class="burst-tile"
            :class="{
              'is-selected': burstPicked.includes(row.id),
              'is-single': !row.tint
            }"
            :style="row.tint ? { '--burst-tint': row.tint } : undefined"
            @click="toggleBurstPick(row.id)"
          >
            <div class="tournament-card__tools">
              <v-btn
                :icon="burstPhotoOf(row.id)?.rating === MAX_RATING ? 'mdi-star' : 'mdi-star-outline'"
                size="x-small" variant="flat"
                :color="burstPhotoOf(row.id)?.rating === MAX_RATING ? 'secondary' : undefined"
                :aria-label="`${burstPhotoOf(row.id)?.name} を★${MAX_RATING} で確定`"
                @click.stop="confirmBurstPhoto(row.id)"
              />
              <v-btn
                icon="mdi-thumb-down-outline" size="x-small" variant="flat"
                :aria-label="`${burstPhotoOf(row.id)?.name} を脱落させる`"
                @click.stop="dropBurstPhoto(row.id)"
              />
              <v-btn
                icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                :aria-label="`${burstPhotoOf(row.id)?.name} を拡大`"
                @click.stop="openZoom(burstPhotoOf(row.id), burstPhotos)"
              />
            </div>
            <span v-if="row.tint && representativeOf(row.block) === row.id" class="tournament-card__confirmed">代表</span>
            <span v-if="burstPicked.includes(row.id)" class="tournament-card__check">
              <v-icon icon="mdi-check-bold" size="20" />
            </span>
            <span class="tournament-card__number">★{{ burstPhotoOf(row.id)?.rating ?? 0 }}</span>
            <img
              v-if="burstPhotoOf(row.id)"
              :src="desktop.photoThumbnailUrl(burstPhotoOf(row.id)!)"
              :alt="burstPhotoOf(row.id)?.name"
              draggable="false"
            >
          </v-card>
        </template>
      </div>

      <p v-if="layout.length" class="text-body-2 text-medium-emphasis mt-3">
        {{ layout.length }} 枚 → まとまり {{ groupedCount }} 組 ＋ 単独 {{ singleCount }} 枚
      </p>

      <v-card v-if="!burstBusy && !burstPhotos.length" class="pa-10 text-center text-medium-emphasis">
        この連写を読み込めませんでした。
      </v-card>
    </v-card-text>

    <v-card-actions class="pa-5 flex-wrap ga-2">
      <span class="text-caption text-medium-emphasis">{{ burstPicked.length }} 枚を選択中</span>
      <v-spacer />
      <v-btn
        variant="text" :disabled="burstPicked.length !== 1"
        @click="makeBurstRepresentative(burstPicked[0]!)"
      >代表にする</v-btn>
      <v-btn variant="text" :disabled="burstBusy" @click="scatterBurst">全部バラバラに</v-btn>
      <v-btn
        variant="outlined" prepend-icon="mdi-arrow-split-vertical"
        :disabled="!burstPicked.length" @click="splitBurstSelection"
      >切り離す</v-btn>
      <v-btn color="primary" :loading="burstBusy" @click="applyBurstShape">この形で戻る</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
