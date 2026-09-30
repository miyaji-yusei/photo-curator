<script setup lang="ts">
import HomeView from '~/components/views/HomeView.vue'
import ProjectView from '~/components/views/ProjectView.vue'
import MethodView from '~/components/views/MethodView.vue'
import SettingsView from '~/components/views/SettingsView.vue'
import AppSettingsView from '~/components/views/AppSettingsView.vue'
import BurstThresholdView from '~/components/views/BurstThresholdView.vue'
import BurstPreviewView from '~/components/views/BurstPreviewView.vue'
import TournamentView from '~/components/views/TournamentView.vue'
import ResultsView from '~/components/views/ResultsView.vue'
import BurstReviewView from '~/components/views/BurstReviewView.vue'
import RoundResultView from '~/components/views/RoundResultView.vue'
import CreateDialog from '~/components/dialogs/CreateDialog.vue'
import TaskDialog from '~/components/dialogs/TaskDialog.vue'
import GroupSizeDialog from '~/components/dialogs/GroupSizeDialog.vue'
import ShareDialog from '~/components/dialogs/ShareDialog.vue'
import MoveDialog from '~/components/dialogs/MoveDialog.vue'
import BurstDialog from '~/components/dialogs/BurstDialog.vue'
import NextRoundDialog from '~/components/dialogs/NextRoundDialog.vue'
import RestartDialog from '~/components/dialogs/RestartDialog.vue'
import DisplayEdgeDialog from '~/components/dialogs/DisplayEdgeDialog.vue'
import ExportDialog from '~/components/dialogs/ExportDialog.vue'
import MetadataDialog from '~/components/dialogs/MetadataDialog.vue'
import DeleteDialog from '~/components/dialogs/DeleteDialog.vue'
import SidecarConflictDialog from '~/components/dialogs/SidecarConflictDialog.vue'
import AppNav from '~/components/AppNav.vue'
import { panBy, wheelFactor, ZOOM_RESET, zoomAt, zoomLabel, type ZoomView } from '~/utils/zoomPan'

const c = useCurator()
onMounted(c.mount)
onBeforeUnmount(c.unmount)
const photoInput = c.photoInput
const { notice, notify, dismiss } = useNotice()
// 成功・情報の通知は下に重ねる（4 秒で消える）。警告はここで通知に流す。
const noticeOpen = computed({
  get: () => notice.value !== null,
  set: (open: boolean) => { if (!open) dismiss() }
})
watch(c.taskWarning, (text) => { if (text) notify(text, 'info') })
const {
  activeProject,
  analysisFailures,
  analysisProgress,
  analysisRunning,
  analysisValue,
  cancelAnalysis,
  desktop,
  error,
  isSelecting,
  loading,
  onPhotoPicked,
  session,
  stepZoom,
  taskWarning,
  view,
  zoomError,
  zoomIndex,
  zoomList,
  zoomLoading,
  zoomPhoto,
  zoomSrc
} = c

// ---- 拡大の倍率・移動（Ctrl+ホイール、ドラッグ、ダブルクリック） ----
const zoomView = ref<ZoomView>({ ...ZOOM_RESET })
const zoomBox = ref<HTMLElement | null>(null)
const zoomImage = ref<HTMLImageElement | null>(null)
const zoomTransform = computed(() => {
  const v = zoomView.value
  return v.scale > 1 ? { transform: `translate(${v.x}px, ${v.y}px) scale(${v.scale})` } : undefined
})
const zoomScaleLabel = computed(() => zoomLabel(zoomView.value.scale))
// 写真を前後に送る・閉じる・開き直すときは 1 倍に戻す。
watch(zoomPhoto, () => { zoomView.value = { ...ZOOM_RESET } })

function zoomSize() {
  return { w: zoomImage.value?.offsetWidth ?? 0, h: zoomImage.value?.offsetHeight ?? 0 }
}

// 拡大中だけ window で受ける。Ctrl+ホイール（ページ全体の拡大）を止めるため passive: false。
function onZoomWheel(event: WheelEvent) {
  if (!event.ctrlKey) return
  event.preventDefault()
  const box = zoomBox.value
  if (!box) return
  const rect = box.getBoundingClientRect()
  const { w, h } = zoomSize()
  zoomView.value = zoomAt(
    zoomView.value,
    wheelFactor(event.deltaY),
    event.clientX - (rect.left + rect.width / 2),
    event.clientY - (rect.top + rect.height / 2),
    w,
    h
  )
}
watch(() => !!zoomPhoto.value, (open) => {
  if (open) window.addEventListener('wheel', onZoomWheel, { passive: false })
  else window.removeEventListener('wheel', onZoomWheel)
})
onBeforeUnmount(() => window.removeEventListener('wheel', onZoomWheel))

let dragFrom: { x: number, y: number, id: number } | null = null
function onZoomPointerDown(event: PointerEvent) {
  if (zoomView.value.scale <= 1 || event.button !== 0) return
  dragFrom = { x: event.clientX, y: event.clientY, id: event.pointerId }
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
}
function onZoomPointerMove(event: PointerEvent) {
  if (!dragFrom || dragFrom.id !== event.pointerId) return
  const { w, h } = zoomSize()
  zoomView.value = panBy(zoomView.value, event.clientX - dragFrom.x, event.clientY - dragFrom.y, w, h)
  dragFrom = { ...dragFrom, x: event.clientX, y: event.clientY }
}
function onZoomPointerUp(event: PointerEvent) {
  if (dragFrom?.id === event.pointerId) dragFrom = null
}
// 倍率が 1 を超えているあいだの写真のクリックは閉じない（ドラッグ直後の click も同じ）。
function onZoomPhotoClick(event: MouseEvent) {
  if (zoomView.value.scale > 1) event.stopPropagation()
}
function onZoomReset() {
  zoomView.value = { ...ZOOM_RESET }
}
</script>

<template>
  <v-app>
    <AppNav />

    <v-main class="app-shell">
      <!-- エラーは消えないまま、本文を下げないよう上に重ねる。 -->
      <v-alert
        v-if="error"
        type="error"
        closable
        elevation="6"
        class="error-overlay"
        @click:close="error = ''"
      >{{ error }}</v-alert>
      <!-- 選別中は写真の面積を優先し、余白と横幅の上限をゆるめる。 -->
      <v-container
        fluid
        :class="isSelecting ? 'pa-3 pa-md-5' : 'pa-7 pa-md-10'"
        :style="{ maxWidth: isSelecting ? '100%' : '1680px' }"
      >
        <v-progress-linear v-if="loading" indeterminate color="primary" class="mb-6" />

        <!-- 連写解析はバックグラウンドで進む。操作を止めずに状況だけ見せる。 -->
        <v-alert v-if="analysisRunning" type="info" variant="tonal" density="compact" class="mb-5">
          <div class="d-flex align-center justify-space-between flex-wrap ga-3">
            <span class="text-body-2">{{ analysisProgress?.message }}<template v-if="analysisProgress?.total"> （{{ analysisProgress.processed.toLocaleString() }} / {{ analysisProgress.total.toLocaleString() }}）</template></span>
            <v-btn variant="text" size="small" @click="cancelAnalysis">解析を止める</v-btn>
          </div>
          <v-progress-linear :model-value="analysisValue" :indeterminate="!analysisProgress?.total" color="primary" height="6" rounded class="mt-2" />
        </v-alert>
        <!-- 1枚も解析できなくても選別は続けられる。件数だけ伝えて先へ進ませる。 -->
        <v-alert v-if="analysisFailures" type="warning" variant="tonal" density="compact" closable class="mb-5" @click:close="analysisFailures = 0">
          {{ analysisFailures.toLocaleString() }} 件を解析できませんでした。該当の写真は連写のまとめ対象から外れますが、選別はこのまま続けられます。
        </v-alert>

        <template v-if="view === 'home'">
          <HomeView />
        </template>

        <template v-else-if="view === 'project' && activeProject">
          <ProjectView />
        </template>

        <template v-else-if="view === 'method'">
          <MethodView />
        </template>

        <template v-else-if="view === 'settings'">
          <SettingsView />
        </template>

        <template v-else-if="view === 'app-settings'">
          <AppSettingsView />
        </template>

        <template v-else-if="view === 'burst-threshold' && session">
          <BurstThresholdView />
        </template>

        <template v-else-if="view === 'burst-preview' && session">
          <BurstPreviewView />
        </template>

        <template v-else-if="view === 'tournament' && session">
          <TournamentView />
        </template>

        <template v-else-if="view === 'results'">
          <ResultsView />
        </template>

        <!-- 連写の見直し。まとめ単位で「残す写真」を決めて星を上げ下げする。 -->
        <template v-else-if="view === 'burst-review'">
          <BurstReviewView />
        </template>

        <template v-else-if="view === 'result' && session">
          <RoundResultView />
        </template>
      </v-container>
    </v-main>

    <!-- 写真ライブラリから受け取る口。ブラウザはフォルダを走査できないので、
         これが取り込みの唯一の入口になる。 -->
    <input
      ref="photoInput" type="file" accept="image/*" multiple
      class="d-none" aria-hidden="true" tabindex="-1"
      @change="onPhotoPicked"
    >

    <!-- 拡大表示。写真だけを見せたいので余計な枠は置かない。
         ← → は前後送り、Esc・Enter・Space は閉じる。Ctrl+スクロールで拡大・縮小。 -->
    <v-overlay
      :model-value="!!zoomPhoto"
      class="zoom-overlay align-center justify-center"
      scrim="#000000"
      opacity="0.94"
      :z-index="3000"
      @click="zoomPhoto = null"
      @update:model-value="value => { if (!value) zoomPhoto = null }"
    >
      <div v-if="zoomPhoto" class="zoom-overlay__inner">
        <!-- 送りボタンは写真の外に置く。写真の上に重ねると、閉じるつもりの
             クリックが送りに化ける。 -->
        <div class="zoom-overlay__stage">
          <v-btn
            class="zoom-overlay__step" icon="mdi-chevron-left" variant="text" size="large"
            aria-label="前の写真" :disabled="zoomIndex <= 0"
            @click.stop="stepZoom(-1)"
          />
          <div
            ref="zoomBox" class="zoom-overlay__photo" :class="{ 'is-zoomed': zoomView.scale > 1 }"
            @click="onZoomPhotoClick" @dblclick="onZoomReset"
            @pointerdown="onZoomPointerDown" @pointermove="onZoomPointerMove"
            @pointerup="onZoomPointerUp" @pointercancel="onZoomPointerUp"
          >
            <img ref="zoomImage" :src="zoomSrc" :alt="zoomPhoto.name" :style="zoomTransform" draggable="false">
            <!-- 原本に替わるまで、表示用画像の上にぐるぐるを重ねる。 -->
            <v-progress-circular v-if="zoomLoading" class="zoom-overlay__spinner" indeterminate color="white" size="48" width="4" aria-label="原本を読み込み中" />
          </div>
          <v-btn
            class="zoom-overlay__step" icon="mdi-chevron-right" variant="text" size="large"
            aria-label="次の写真" :disabled="zoomIndex < 0 || zoomIndex >= zoomList.length - 1"
            @click.stop="stepZoom(1)"
          />
        </div>
        <div class="zoom-overlay__caption text-caption">
          <span v-if="zoomList.length > 1" class="zoom-overlay__position">{{ zoomIndex + 1 }} / {{ zoomList.length }}</span>
          {{ zoomPhoto.name }}
          <template v-if="zoomLoading"> ・ 原本を読み込み中…</template>
          <span v-if="zoomError" class="text-error"> ・ {{ zoomError }}</span>
          <span v-if="zoomScaleLabel" class="zoom-overlay__scale"> ・ {{ zoomScaleLabel }}</span>
          <template v-if="zoomList.length > 1"> ・ ← → で前後</template>
          ・ Esc かクリックで閉じる ・ Ctrl+スクロールで拡大・縮小
        </div>
      </div>
    </v-overlay>

    <CreateDialog />
    <TaskDialog />
    <GroupSizeDialog />
    <ShareDialog />
    <MoveDialog />
    <BurstDialog />
    <NextRoundDialog />
    <RestartDialog />
    <DisplayEdgeDialog />
    <ExportDialog />
    <MetadataDialog />
    <DeleteDialog />
    <SidecarConflictDialog />

    <!-- 成功・情報の通知。画面の下に重ねて 4 秒で消える（× でも消せる）。 -->
    <v-snackbar
      :key="notice?.id ?? 0"
      v-model="noticeOpen"
      location="bottom"
      :timeout="4000"
      :color="notice?.kind === 'warning' ? 'warning' : notice?.kind === 'info' ? 'info' : 'success'"
    >
      {{ notice?.text }}
      <template #actions>
        <v-btn icon="mdi-close" size="small" variant="text" aria-label="閉じる" @click="dismiss" />
      </template>
    </v-snackbar>
  </v-app>
</template>
