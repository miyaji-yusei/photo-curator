<script setup lang="ts">
// スライドショー選別（U19）。写真を 1 枚ずつ出し、残す（右）／落とす（左）／★5 で確定（上）を決める。
// 方式が違うだけで、セッション・★の上げ方・1 つ戻す・連写のまとめ・拡大はトーナメントと同じ処理を通す。
// 上のバーは TournamentView と同じ並び・文言・クラス。ここには判定を書かない（決まった操作を渡すだけ）。
import type { Ref } from 'vue'
import SelectionHelpButton from '~/components/dialogs/SelectionHelpButton.vue'
import type { SavedSelection } from '~/utils/selectionFlow'
import {
  dragFeedback, fitContain, flyTarget, isDoubleTap, isTap, judgeDrag, slideKeyDecision, tapDecision, type TapRecord,
  type SlideDecision
} from '~/utils/slideshowGesture'

const {
  MAX_RATING,
  burstDialog,
  burstSizeOf,
  canUndo,
  decideSlide,
  desktop,
  groupSizeDialog,
  helpDialog,
  openBurst,
  openGroupSizeDialog,
  openZoom,
  remainingPhotos,
  roundNumber,
  roundProgress,
  session: nullableSession,
  targetStar,
  tournamentPhotos,
  undoChoice,
  view,
  zoomPhoto
} = useCurator()
// 親の `v-if` で null を除いているので、ここでは non-null として扱う。
const session = nullableSession as Ref<SavedSelection>

/** 写真と枠の間の余白（px）。 */
const MARGIN = 6
/** 決めたあと、写真が画面の外へ飛ぶ時間（ms）。 */
const FLY_MS = 220
/** ドラッグを離して元に戻る時間（ms）。 */
const SNAP_MS = 180

const photo = computed(() => tournamentPhotos.value[0] ?? null)

// ---- 枠の大きさ（TournamentView と同じく、固定の高さの枠を測る） ----
const stage = ref<HTMLElement | null>(null)
const frame = ref({ width: 0, height: 0 })
let observer: ResizeObserver | null = null
function measure() {
  const el = stage.value
  if (el) frame.value = { width: el.clientWidth, height: el.clientHeight }
}
onMounted(() => {
  measure()
  window.addEventListener('keydown', onKeydown)
  if (typeof ResizeObserver === 'undefined' || !stage.value) return
  observer = new ResizeObserver(measure)
  observer.observe(stage.value)
})
onBeforeUnmount(() => {
  observer?.disconnect()
  window.removeEventListener('keydown', onKeydown)
})

// ---- 写真を枠いっぱい（縦横比は保つ）に ----
// 読み込むまでは 3:2 とみなす。ボタン・連写の印は、枠ではなく写真の四隅に置く。
const natural = ref<{ id: string, width: number, height: number } | null>(null)
function onLoaded(event: Event) {
  const img = event.target as HTMLImageElement
  if (photo.value) natural.value = { id: photo.value.id, width: img.naturalWidth, height: img.naturalHeight }
}
const box = computed(() => {
  const known = natural.value && natural.value.id === photo.value?.id ? natural.value : null
  return fitContain(
    known?.width ?? 3, known?.height ?? 2,
    Math.max(0, frame.value.width - MARGIN * 2), Math.max(0, frame.value.height - MARGIN * 2)
  )
})

// ---- 動き ----
const busy = ref(false)
// 写真が動いている量。photoId が今の写真と違えば無視する（次の写真は必ず静止した位置から出す）。
const motion = ref({ photoId: '', x: 0, y: 0, rotation: 0, animate: 0 })
const feedback = ref<{ photoId: string, direction: SlideDecision | null, strength: number }>({ photoId: '', direction: null, strength: 0 })

const activeMotion = computed(() => (photo.value && motion.value.photoId === photo.value.id ? motion.value : null))
const activeFeedback = computed(() =>
  photo.value && feedback.value.photoId === photo.value.id ? feedback.value : { photoId: '', direction: null, strength: 0 }
)
const photoStyle = computed(() => {
  const m = activeMotion.value
  return {
    width: `${box.value.width}px`,
    height: `${box.value.height}px`,
    transform: m ? `translate(${m.x}px, ${m.y}px) rotate(${m.rotation}deg)` : undefined,
    transition: m && m.animate ? `transform ${m.animate}ms ease-out` : 'none'
  }
})

function setMotion(x: number, y: number, rotation: number, animate = 0) {
  if (photo.value) motion.value = { photoId: photo.value.id, x, y, rotation, animate }
}
function setFeedback(direction: SlideDecision | null, strength: number) {
  if (photo.value) feedback.value = { photoId: photo.value.id, direction, strength }
}
const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

/** 決めて、写真を飛ばし、core へ渡す。 */
async function decide(kind: SlideDecision) {
  const current = photo.value
  if (!current || busy.value) return
  busy.value = true
  try {
    const fly = flyTarget(kind, frame.value.width, frame.value.height)
    setFeedback(kind, 1)
    setMotion(fly.x, fly.y, fly.rotation, FLY_MS)
    await sleep(FLY_MS)
    await decideSlide(kind, current.id)
  } finally {
    motion.value = { photoId: '', x: 0, y: 0, rotation: 0, animate: 0 }
    feedback.value = { photoId: '', direction: null, strength: 0 }
    busy.value = false
  }
}

// ---- ポインタ（クリック・ドラッグ） ----
let lastCenterTap: TapRecord | null = null
let drag: { id: number, startX: number, startY: number, moved: boolean } | null = null
function onPointerDown(event: PointerEvent) {
  if (busy.value || !photo.value || event.button !== 0) return
  // ボタン（拡大・★5・連写の印）の操作はここで拾わない。
  if ((event.target as HTMLElement).closest('button, .v-btn')) return
  drag = { id: event.pointerId, startX: event.clientX, startY: event.clientY, moved: false }
  stage.value?.setPointerCapture(event.pointerId)
}
function onPointerMove(event: PointerEvent) {
  if (!drag || drag.id !== event.pointerId) return
  const dx = event.clientX - drag.startX
  const dy = event.clientY - drag.startY
  if (!drag.moved && isTap(dx, dy)) return
  drag.moved = true
  const look = dragFeedback(dx, dy, frame.value.width)
  setMotion(dx, dy, look.rotation)
  setFeedback(look.direction, look.strength)
}
function release(event: PointerEvent, cancelled: boolean) {
  if (!drag || drag.id !== event.pointerId) return
  const dx = event.clientX - drag.startX
  const dy = event.clientY - drag.startY
  const moved = drag.moved
  drag = null
  if (stage.value?.hasPointerCapture(event.pointerId)) stage.value.releasePointerCapture(event.pointerId)
  if (cancelled) return snapBack(moved)
  if (!moved) {
    // クリック。上の帯は★5、ほぼ中心は何もしない（二度押しで拡大）、それ以外は左半分が落とす・右半分が残す
    // （長押しの拡大はしない）。
    const rect = stage.value?.getBoundingClientRect()
    if (!rect) return
    const result = tapDecision(event.clientX, event.clientY, rect.left, rect.top, rect.width, rect.height)
    if (result !== 'center') {
      lastCenterTap = null
      void decide(result)
      return
    }
    // 1 回目は何もしない（遅らせない）。300ms・24px 以内の 2 回目で拡大を開く。
    const now = { time: event.timeStamp, x: event.clientX, y: event.clientY }
    if (isDoubleTap(lastCenterTap, now)) {
      lastCenterTap = null
      if (photo.value) openZoom(photo.value, [photo.value])
    } else {
      lastCenterTap = now
    }
    return
  }
  lastCenterTap = null
  const decision = judgeDrag(dx, dy, frame.value.width)
  if (decision) void decide(decision)
  else snapBack(true)
}
function snapBack(moved: boolean) {
  if (!moved) return
  setMotion(0, 0, 0, SNAP_MS)
  setFeedback(null, 0)
}

// ---- キー（1・←＝落とす／3・→＝残す／5・↑＝★5）。Backspace（1 つ戻す）は useCurator が受ける ----
function onKeydown(event: KeyboardEvent) {
  if (view.value !== 'tournament' || event.repeat) return
  if (zoomPhoto.value || burstDialog.value || groupSizeDialog.value || helpDialog.value) return
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return
  const decision = slideKeyDecision(event)
  if (!decision) return
  event.preventDefault()
  void decide(decision)
}

async function undo() {
  if (busy.value) return
  await undoChoice()
}
</script>

<template>
    <!-- 上のバー。TournamentView と同じ並び・文言（左: 中断・1 つ戻す / 真ん中: 状況 / 右: 設定）。
         「複数選択」「この写真をまとめる」「選択なしで次へ」はスライドショーでは出さない。 -->
    <div class="tournament-bar d-flex align-center ga-2">
      <div class="d-flex align-center ga-1 tournament-bar__side">
        <v-btn variant="text" prepend-icon="mdi-pause" @click="view = 'project'">中断して戻る</v-btn>
        <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!canUndo || busy" @click="undo">1つ戻す</v-btn>
      </div>

      <div class="tournament-bar__center text-center">
        <div class="text-overline text-primary">★{{ targetStar }} を選別中 &middot; ROUND {{ roundNumber }}</div>
        <div class="text-body-2">残り {{ remainingPhotos.toLocaleString() }} 枚</div>
      </div>

      <div class="d-flex align-center justify-end ga-2 tournament-bar__side">
        <v-btn icon="mdi-dots-horizontal" variant="text" aria-label="選別中の設定（表示枚数）" title="選別中の設定（表示枚数）" @click="openGroupSizeDialog" />
        <SelectionHelpButton />
      </div>
    </div>
    <v-progress-linear :model-value="roundProgress" color="primary" height="4" rounded class="mb-3" />

    <div
      ref="stage" class="tournament-stage slideshow-stage"
      @pointerdown="onPointerDown" @pointermove="onPointerMove"
      @pointerup="release($event, false)" @pointercancel="release($event, true)"
    >
      <div v-if="photo" :key="photo.id" class="slideshow-photo" :class="{ 'is-burst': burstSizeOf(photo.id) > 1 }" :style="photoStyle">
        <img :src="desktop.photoDisplayUrl(photo)" :alt="photo.name" draggable="false" @load="onLoaded">

        <!-- ドラッグ中だけ、色の膜＋アイコン＋文字がドラッグ量に応じて濃くなる。 -->
        <div
          v-if="activeFeedback.direction"
          class="slideshow-veil" :class="`is-${activeFeedback.direction}`"
          :style="{ '--strength': activeFeedback.strength }"
          aria-hidden="true"
        >
          <div class="slideshow-veil__label">
            <v-icon
              :icon="activeFeedback.direction === 'drop' ? 'mdi-close-thick' : activeFeedback.direction === 'keep' ? 'mdi-heart' : 'mdi-star'"
              size="64"
            />
            <span>{{ activeFeedback.direction === 'drop' ? '落とす' : activeFeedback.direction === 'keep' ? '残す' : `★${MAX_RATING} で確定` }}</span>
          </div>
        </div>

        <!-- トーナメントと同じ「拡大」「★5 で確定」。写真の右上。 -->
        <div class="tournament-card__tools">
          <v-btn
            icon="mdi-star-outline" size="x-small" variant="flat"
            :disabled="busy"
            :aria-label="`${photo.name} を★${MAX_RATING} で確定（キー 5・↑、写真の上のほうのクリックでも確定。ほぼ中心のクリックは何もせず、二度押しで拡大）`"
            :title="`★${MAX_RATING} で確定（5・↑）`"
            @click.stop="decide('top')"
          />
          <v-btn
            icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
            :aria-label="`${photo.name} を拡大`" title="拡大（写真の中心の二度押しでも開く）"
            @click.stop="openZoom(photo, [photo])"
          />
        </div>

        <!-- 連写の印。トーナメントと同じ手直しのダイアログを開く。 -->
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
      </div>
    </div>
</template>

<style scoped>
/* 枠（.tournament-stage）は TournamentView と同じ固定の高さ。写真は中央に、余白 6px の内側いっぱい。 */
.slideshow-stage {
  display: grid; place-items: center;
  touch-action: none; user-select: none; -webkit-user-select: none;
  cursor: grab; overflow: hidden;
}
.slideshow-photo {
  position: relative; border-radius: 6px; will-change: transform;
  background: #16181d;
}
.slideshow-photo img { width: 100%; height: 100%; display: block; object-fit: contain; border-radius: 6px; pointer-events: none; }
/* 連写は白い縁で束ねた紙に見せる（トーナメントと同じ考え方）。 */
.slideshow-photo.is-burst { outline: 3px solid #ffffff; outline-offset: -3px; }
.slideshow-veil {
  position: absolute; inset: 0; border-radius: 6px; pointer-events: none;
  display: grid; place-items: center;
  background: color-mix(in srgb, var(--veil) calc(var(--strength, 0) * 55%), transparent);
}
.slideshow-veil.is-drop { --veil: rgb(239, 83, 80); }
.slideshow-veil.is-keep { --veil: rgb(76, 175, 80); }
.slideshow-veil.is-top { --veil: rgb(var(--v-theme-secondary)); }
.slideshow-veil__label {
  display: flex; flex-direction: column; align-items: center; gap: 4px;
  color: #fff; font-size: 28px; font-weight: 800; text-shadow: 0 2px 10px rgba(0, 0, 0, .55);
  opacity: var(--strength, 0);
}
</style>
