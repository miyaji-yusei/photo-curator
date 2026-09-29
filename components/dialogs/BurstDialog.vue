<script setup lang="ts">
const {
  MAX_RATING,
  applyBurstShape,
  boundaryBefore,
  burstBlocks,
  burstBusy,
  burstDialog,
  burstOriginal,
  burstPhotoOf,
  burstPhotos,
  burstPicked,
  columnsFor,
  confirmBurstPhoto,
  desktop,
  dropBurstPhoto,
  isOutsideBurst,
  joinBurstAt,
  makeBurstRepresentative,
  openZoom,
  representativeOf,
  scatterBurst,
  splitBurstSelection,
  toggleBurstPick
} = useCurator()
</script>

<template>
<!-- まとめられた連写の中身。ここで代表を差し替えられる。 -->
<v-dialog v-model="burstDialog" fullscreen transition="dialog-bottom-transition" scrollable>
  <v-card>
    <v-toolbar color="surface" density="comfortable">
      <v-toolbar-title>連写 {{ burstOriginal.length }} 枚 — {{ burstBlocks.length }} つのまとまり</v-toolbar-title>
      <v-spacer />
      <v-btn icon="mdi-close" aria-label="閉じる" @click="burstDialog = false" />
    </v-toolbar>
    <v-card-text class="pt-5">
      <p class="text-body-2 text-medium-emphasis mb-4">
        写真を選んで「切り離す」と、選んだぶんが別のまとまりになります。
        まとまりの境目の「つなぐ」で元に戻せます。薄い写真はまとめの外にある近くの写真で、
        つなぐと取り込めます。
      </p>

      <v-progress-linear v-if="burstBusy" indeterminate color="primary" class="mb-4" />

      <!-- まとまりごとに枠で囲む。境目そのものが操作の対象なので、
           間に「つなぐ」を置いて、切れているのが見えるようにする。 -->
      <template v-for="(block, blockIndex) in burstBlocks" :key="block[0]">
        <div v-if="blockIndex > 0" class="burst-seam">
          <span class="burst-seam__line" />
          <v-btn
            size="small" variant="outlined" prepend-icon="mdi-link-variant"
            @click="joinBurstAt(boundaryBefore(block[0]!))"
          >つなぐ</v-btn>
          <span class="burst-seam__line" />
        </div>

        <div class="burst-block" :class="{ 'is-outside': isOutsideBurst(block) }">
          <div class="text-caption text-medium-emphasis mb-2">
            {{ isOutsideBurst(block) ? 'まとめの外' : `まとまり ${blockIndex + 1}` }}
            ・ {{ block.length }} 枚
          </div>
          <div
            class="tournament-grid burst-review-grid"
            :style="{ '--tournament-columns': columnsFor(block.length), '--tournament-rows': 1 }"
          >
            <v-card
              v-for="photoId in block"
              :key="photoId"
              class="tournament-card"
              :class="{
                'is-selected': burstPicked.includes(photoId),
                'is-representative': block.length > 1 && representativeOf(block) === photoId
              }"
              @click="toggleBurstPick(photoId)"
            >
              <div class="tournament-card__tools">
                <v-btn
                  :icon="burstPhotoOf(photoId)?.rating === MAX_RATING ? 'mdi-star' : 'mdi-star-outline'"
                  size="x-small" variant="flat"
                  :color="burstPhotoOf(photoId)?.rating === MAX_RATING ? 'secondary' : undefined"
                  :aria-label="`${burstPhotoOf(photoId)?.name} を★${MAX_RATING} で確定`"
                  @click.stop="confirmBurstPhoto(photoId)"
                />
                <v-btn
                  icon="mdi-thumb-down-outline" size="x-small" variant="flat"
                  :aria-label="`${burstPhotoOf(photoId)?.name} を脱落させる`"
                  @click.stop="dropBurstPhoto(photoId)"
                />
                <v-btn
                  icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                  :aria-label="`${burstPhotoOf(photoId)?.name} を拡大`"
                  @click.stop="openZoom(burstPhotoOf(photoId), burstPhotos)"
                />
              </div>
              <span v-if="block.length > 1 && representativeOf(block) === photoId" class="tournament-card__confirmed">代表</span>
              <span v-if="burstPicked.includes(photoId)" class="tournament-card__check">
                <v-icon icon="mdi-check-bold" size="20" />
              </span>
              <span class="tournament-card__number">★{{ burstPhotoOf(photoId)?.rating ?? 0 }}</span>
              <img v-if="burstPhotoOf(photoId)" :src="desktop.photoDisplayUrl(burstPhotoOf(photoId)!)" :alt="burstPhotoOf(photoId)?.name">
            </v-card>
          </div>
        </div>
      </template>

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
