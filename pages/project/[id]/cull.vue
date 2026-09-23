<script setup lang="ts">
// 選別（本体）＋開始シート／基準学習／まとまり確認／ラウンド完了（設計 02 章）。
// **帯は上の1本だけ。アプリバーは出さない**（layouts/focus.vue）。
import { computed, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useCapabilities } from '~/composables/useCapabilities'
import { useAppStore } from '~/stores/app'
import * as core from '~/lib/core'
import type { PhotoRef, Session, BurstAnswer, BurstThreshold, PairOverride } from '~/lib/core'
import type { ProjectPhoto, Project } from '~/types/project'
import { buildBurstQuestions } from '~/utils/burstQuestions'
import { groupSizeLimits, clampGroupSize } from '~/utils/groupSize'
import { registerAutoPush } from '~/composables/useSidecarSync'
import BurstEditSheet from '~/components/BurstEditSheet.vue'
import ZoomView from '~/components/ZoomView.vue'

definePageMeta({ layout: 'focus' })

const backend = useBackend()
const router = useRouter()
const route = useRoute()
const app = useAppStore()
const { capabilities } = useCapabilities()

const projectId = computed(() => String(route.params.id))
const project = ref<Project | null>(null)
const photos = ref<ProjectPhoto[]>([])
const photoRefs = ref<PhotoRef[]>([])
const overrides = ref<PairOverride[]>([])
const burstDistance = ref<number | null>(null)
// パス→写真。拡大の「n MB」表示に使う（設計 02 章 zoom シート）。
const photoByPath = computed(() => Object.fromEntries(photos.value.map(p => [p.relativePath, p])))

type Phase = 'loading' | 'start' | 'learn' | 'preview' | 'tournament' | 'roundComplete'
const phase = ref<Phase>('loading')

const session = ref<Session | null>(null)
const displayUrls = ref<Record<string, string>>({})

// start
const groupSize = ref(4)
const groupBursts = ref(true)

// learn
const questions = ref<ReturnType<typeof buildBurstQuestions>>([])
const questionIndex = ref(0)
const answers = ref<BurstAnswer[]>([])

// preview
const previewDistance = ref(9)
const previewGroups = ref<core.BurstGroup[]>([])

// tournament
const selected = ref<Set<string>>(new Set())
const multiMode = ref(false)
const roundStartedAt = ref(0)

// 枠の行×列（02章「選別」の表。GRID = {2:[1,2],3:[1,3],4:[2,2],...}）。
// landscape/portrait で行列を入れ替える（app-android の Cull.kt gridFor と同じ）。
const GRID: Record<number, [number, number]> = {
  1: [1, 1], 2: [1, 2], 3: [1, 3], 4: [2, 2], 5: [2, 3],
  6: [2, 3], 7: [2, 4], 8: [2, 4], 9: [3, 3], 10: [2, 5]
}
function gridFor(count: number, landscape: boolean): [number, number] {
  const found = GRID[count]
  const [rows, cols] = found ?? [Math.ceil(count / 5), Math.min(5, Math.max(1, count))]
  return landscape ? [rows, cols] : [cols, rows]
}

// 枠の実寸から landscape/portrait を測る。**画面の残り全部を使う**ため、
// vw/vh の固定サイズではなく実測したコンテナの縦横比で行×列を決める。
const stageEl = ref<HTMLElement | null>(null)
const stageSize = ref({ width: 0, height: 0 })
let stageObserver: ResizeObserver | null = null
watch(stageEl, (el) => {
  stageObserver?.disconnect()
  stageObserver = null
  if (el) {
    stageObserver = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      if (rect) stageSize.value = { width: rect.width, height: rect.height }
    })
    stageObserver.observe(el)
  }
})
onBeforeUnmount(() => stageObserver?.disconnect())

const gridDims = computed<[number, number]>(() => {
  const count = session.value?.current.length ?? 0
  const landscape = stageSize.value.width >= stageSize.value.height
  return gridFor(count, landscape)
})

// 選別中の「…」（optionsSheet。02章。FR-3.7）。**手を止めずに設定を変えられる場所。**
const optionsOpen = ref(false)
const groupSizeChoices = computed(() => {
  const limits = groupSizeLimits(capabilities.largeGroups)
  const out: number[] = []
  for (let size = limits.min; size <= limits.max; size += 1) out.push(size)
  return out
})
// 枚数はいまの組にすぐ効く（core.resize）。**選んだ印はそのまま残す**
// （画面から外れた写真の分だけ落とす）。
async function applyGroupSize(size: number) {
  if (!session.value) return
  groupSize.value = size
  const next = core.resize(session.value, size)
  session.value = next
  selected.value = new Set([...selected.value].filter(p => next.current.includes(p)))
  await backend.saveSession(projectId.value, next)
  await app.save({ ...app.settings, groupSize: size })
  await loadDisplayUrls(next.current)
}
// 連写まとめの on/off。core.regroup は**まだ判断していない写真だけ**組み直すので、
// 確定済みの組（history）はリセットされない（07章 2026-09-15 の教訓）。
async function applyGroupBursts(on: boolean) {
  if (!session.value) return
  groupBursts.value = on
  const next = core.regroup(session.value, photoRefs.value, on, threshold(), overrides.value)
  session.value = next
  selected.value = new Set()
  await backend.saveSession(projectId.value, next)
  await app.save({ ...app.settings, groupBursts: on })
  await loadDisplayUrls(next.current)
}

// 拡大（zoom）。いまの組の中を左右で見比べられる（02章 zoom シート）。
const zoomIndex = ref<number | null>(null)
function openZoom(path: string) {
  if (!session.value) return
  const at = session.value.current.indexOf(path)
  if (at >= 0) zoomIndex.value = at
}
async function zoomKeep(path: string) {
  zoomIndex.value = null
  if (!session.value) return
  await persist(core.advance(session.value, [path]))
  selected.value = new Set()
}

function threshold(): BurstThreshold {
  return { window_ms: 4000, distance: burstDistance.value ?? previewDistance.value ?? 9, d_hash_version: 2 }
}

async function loadDisplayUrls(paths: string[]) {
  const entries = await Promise.all(
    paths.map(async p => [p, await backend.displayUrl(projectId.value, p)] as const)
  )
  for (const [p, url] of entries) if (url) displayUrls.value[p] = url
}

onMounted(async () => {
  await core.init()
  await app.load()
  project.value = await backend.getProject(projectId.value)
  if (!project.value) return
  photos.value = await backend.listPhotos(projectId.value)
  photoRefs.value = photos.value
    .slice()
    .sort((a, b) => (a.capturedAt ?? 0) - (b.capturedAt ?? 0))
    .map(p => ({
      relative_path: p.relativePath,
      captured_at: p.capturedAt,
      d_hash: p.dHash,
      d_hash_version: p.dHashVersion
    }))
  overrides.value = await backend.loadOverrides(projectId.value)
  burstDistance.value = await backend.loadBurstDistance(projectId.value)
  groupSize.value = clampGroupSize(app.settings.groupSize, groupSizeLimits(capabilities.largeGroups))
  groupBursts.value = app.settings.groupBursts

  const existing = await backend.loadSession(projectId.value)
  if (existing && !existing.finished) {
    session.value = existing
    phase.value = 'tournament'
    roundStartedAt.value = Date.now()
    await loadDisplayUrls(existing.current)
  } else if (existing && existing.finished) {
    // 前のラウンドが完了状態で保存されている。roundComplete から再開。
    session.value = existing
    phase.value = 'roundComplete'
  } else {
    phase.value = 'start'
  }
})

function beginFromStart() {
  if (groupBursts.value && burstDistance.value === null) {
    questions.value = buildBurstQuestions(photoRefs.value)
    questionIndex.value = 0
    answers.value = []
    phase.value = questions.value.length > 0 ? 'learn' : 'preview'
    if (phase.value === 'preview') {
      void preparePreview()
    } else {
      const paths = questions.value.flatMap(q => [q.left.relative_path, q.right.relative_path])
      void loadDisplayUrls([...new Set(paths)])
    }
  } else {
    void preparePreview()
  }
}

function answerQuestion(same: boolean) {
  const q = questions.value[questionIndex.value]
  if (q) answers.value.push({ distance: q.distance, same })
  questionIndex.value += 1
  if (questionIndex.value >= questions.value.length) finishLearning()
}
function skipLearning() {
  finishLearning()
}
function finishLearning() {
  const learned = core.learnDistance(answers.value, 9)
  previewDistance.value = learned
  void preparePreview()
}

async function preparePreview() {
  await core.init()
  previewGroups.value = groupBursts.value
    ? core.groupBursts(photoRefs.value, threshold(), overrides.value)
    : photoRefs.value.map(p => ({ members: [p.relative_path], representative: p.relative_path }))
  phase.value = 'preview'
  void loadDisplayUrls(previewGroups.value.slice(0, 60).map(g => g.representative))
}

function onPreviewSliderChange() {
  previewGroups.value = core.groupBursts(photoRefs.value, threshold(), overrides.value)
}

async function confirmPreviewAndStart() {
  if (groupBursts.value) {
    burstDistance.value = previewDistance.value
    await backend.saveBurstDistance(projectId.value, previewDistance.value)
  }
  const s = core.startRound(photoRefs.value, groupSize.value, 0, groupBursts.value, threshold(), overrides.value)
  session.value = s
  await backend.saveSession(projectId.value, s)
  phase.value = 'tournament'
  roundStartedAt.value = Date.now()
  selected.value = new Set()
  await loadDisplayUrls(s.current)
}

async function persist(next: Session) {
  session.value = next
  await backend.saveSession(projectId.value, next)
  await loadDisplayUrls(next.current)
  if (next.finished) {
    phase.value = 'roundComplete'
    // 書き時「ラウンド完了」（設計02章）。最後のラウンドだけでなく、毎ラウンド書く。
    if (project.value) await backend.pushSidecarIfChanged(project.value)
  }
}

async function tapTile(path: string) {
  if (!session.value) return
  if (multiMode.value) {
    const set = new Set(selected.value)
    if (set.has(path)) set.delete(path); else set.add(path)
    selected.value = set
    return
  }
  await persist(core.advance(session.value, [path]))
  selected.value = new Set()
}

async function confirmGroup() {
  if (!session.value) return
  await persist(core.advance(session.value, [...selected.value]))
  selected.value = new Set()
  multiMode.value = false
}

function longPress(path: string) {
  multiMode.value = true
  const set = new Set(selected.value)
  set.add(path)
  selected.value = set
}

async function undo() {
  if (!session.value) return
  await persist(core.undo(session.value))
  selected.value = new Set()
}

async function keepTop5(path: string) {
  if (!session.value) return
  await persist(core.keepAndTop(session.value, [...selected.value], path))
  selected.value = new Set()
}

const burstEditTarget = ref<string | null>(null)

async function afterBurstEdit(nextSession: Session) {
  await persist(nextSession)
  burstEditTarget.value = null
}

// roundComplete
const kept = computed(() => session.value ? Object.values(session.value.ratings).filter(r => r > 0).length : 0)
const elapsedText = computed(() => {
  const ms = Date.now() - roundStartedAt.value
  const sec = Math.round(ms / 1000)
  return `所要 ${Math.floor(sec / 60)}分${sec % 60}秒`
})

async function nextRound() {
  if (!session.value) return
  const next = core.nextRound(session.value, photoRefs.value, groupBursts.value, threshold(), overrides.value)
  if (!next) { await finishToResults(); return }
  session.value = next
  await backend.saveSession(projectId.value, next)
  roundStartedAt.value = Date.now()
  phase.value = 'tournament'
  selected.value = new Set()
  await loadDisplayUrls(next.current)
}

async function finishToResults() {
  await backend.markCompleted(projectId.value)
  await backend.pushSidecarIfChanged(project.value!)
  router.push(`/project/${projectId.value}/results`)
}

function onKeydown(event: KeyboardEvent) {
  if (!capabilities.keyboard || phase.value !== 'tournament' || !session.value || zoomIndex.value !== null) return
  if (event.key === 'Enter') { confirmGroup(); return }
  if (event.key === 'Backspace') { undo(); return }
  if (event.key.toLowerCase() === 'm') { multiMode.value = !multiMode.value; return }
  if (event.key === 'Escape') { router.push(`/project/${projectId.value}`); return }
  const n = Number(event.key)
  if (!Number.isNaN(n) && session.value.current[n === 0 ? 9 : n - 1]) {
    tapTile(session.value.current[n === 0 ? 9 : n - 1]!)
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

// 書き時「画面を離れる・背面へ回る・窓を閉じる」（設計02章）。ラウンド完了は persist() で個別に書く。
const stopAutoPush = registerAutoPush(() => project.value)
onBeforeUnmount(() => stopAutoPush())
</script>

<template>
  <div class="d-flex flex-column" style="height: 100vh">
    <div v-if="phase === 'loading'" class="flex-grow-1 d-flex align-center justify-center">
      <v-progress-circular indeterminate color="primary" />
    </div>

    <!-- 開始シート -->
    <div v-else-if="phase === 'start'" class="pa-6" style="max-width: 480px; margin: 0 auto">
      <div class="d-flex align-center mb-4">
        <v-btn icon="mdi-arrow-left" variant="text" :to="`/project/${projectId}`" />
        <span class="text-h6 ml-2">選別を始める</span>
      </div>
      <p class="text-body-2 mb-2">枚数</p>
      <v-slider
        v-model="groupSize"
        :min="2"
        :max="capabilities.largeGroups ? 10 : 4"
        :step="1"
        thumb-label
        class="mb-4"
      />
      <v-switch v-model="groupBursts" label="連写をまとめる" color="primary" />
      <p class="text-body-2 text-medium-emphasis mb-4">準備が終わった {{ photos.length }} 枚から始めます</p>
      <v-btn color="primary" block size="large" @click="beginFromStart">選別を開始</v-btn>
    </div>

    <!-- 基準学習 -->
    <div v-else-if="phase === 'learn'" class="pa-4 d-flex flex-column flex-grow-1" style="min-height: 0">
      <p class="text-h6 mb-2">この2枚は同じ連写ですか？（{{ questionIndex + 1 }} / {{ questions.length }}）</p>
      <div v-if="questions[questionIndex]" class="d-flex flex-grow-1 ga-2" style="min-height: 0">
        <div
          v-for="side in [questions[questionIndex]!.left, questions[questionIndex]!.right]"
          :key="side.relative_path"
          class="flex-grow-1"
          style="min-width: 0; position: relative; background: #16181d; border-radius: 6px; overflow: hidden"
        >
          <img
            v-if="displayUrls[side.relative_path]"
            :src="displayUrls[side.relative_path]"
            style="width: 100%; height: 100%; object-fit: contain"
          >
        </div>
      </div>
      <div class="d-flex ga-2 justify-center mt-4" style="flex-shrink: 0">
        <v-btn color="primary" @click="answerQuestion(true)">同じ</v-btn>
        <v-btn variant="tonal" @click="answerQuestion(false)">別</v-btn>
      </div>
      <v-btn variant="text" class="mt-2" style="flex-shrink: 0" @click="skipLearning">残りをスキップ</v-btn>
    </div>

    <!-- まとまり確認 -->
    <div v-else-if="phase === 'preview'" class="pa-6 flex-grow-1" style="overflow-y: auto">
      <p class="text-h6 mb-2">
        {{ photos.length }} 枚 → {{ previewGroups.length }} グループ／
        連写 {{ previewGroups.filter(g => g.members.length > 1).length }} 組
      </p>
      <v-slider
        v-if="groupBursts"
        v-model="previewDistance"
        :min="2"
        :max="24"
        thumb-label
        label="まとめる強さ"
        @update:model-value="onPreviewSliderChange"
      />
      <div class="d-flex flex-wrap ga-2 mb-4">
        <div v-for="group in previewGroups.slice(0, 60)" :key="group.representative" style="position: relative">
          <img
            v-if="displayUrls[group.representative]"
            :src="displayUrls[group.representative]"
            style="width: 96px; height: 96px; object-fit: cover; border-radius: 4px"
          >
          <div v-else style="width: 96px; height: 96px; background: #16181d; border-radius: 4px" />
          <span v-if="group.members.length > 1" class="text-caption" style="position: absolute; left: 2px; top: 2px; background: rgba(0,0,0,.6); padding: 0 4px">
            ⧉{{ group.members.length }}
          </span>
        </div>
      </div>
      <v-btn color="primary" size="large" @click="confirmPreviewAndStart">この基準で選別を開始</v-btn>
    </div>

    <!-- 選別本体 -->
    <div v-else-if="phase === 'tournament' && session" class="d-flex flex-column flex-grow-1" style="min-height: 0">
      <div class="d-flex align-center px-3" style="height: 56px; flex-shrink: 0">
        <v-btn icon="mdi-arrow-left" variant="text" :to="`/project/${projectId}`" />
        <v-btn icon="mdi-undo" variant="text" :disabled="session.history.length === 0" @click="undo" />
        <v-btn :icon="multiMode ? 'mdi-checkbox-multiple-marked' : 'mdi-checkbox-multiple-blank-outline'" variant="text" @click="multiMode = !multiMode" />
        <span class="flex-grow-1 text-center">★{{ session.target_star }} を選別中 · ROUND {{ session.round }}</span>
        <v-btn icon="mdi-dots-vertical" variant="text" @click="optionsOpen = true" />
        <v-btn
          color="primary"
          variant="flat"
          @click="confirmGroup"
        >
          {{ selected.size === 0 ? (session.current.length + '枚とも落とす') : (selected.size + '枚を残す') }}
        </v-btn>
      </div>
      <v-progress-linear :model-value="((photos.length - session.queue.length - session.current.length) / Math.max(1, photos.length)) * 100" height="2" color="primary" />
      <p class="text-center text-caption text-medium-emphasis my-1" style="flex-shrink: 0">
        残り {{ session.queue.length + session.current.length }} 枚
      </p>
      <div
        ref="stageEl"
        class="flex-grow-1 pa-2"
        style="min-height: 0; overflow: hidden; display: grid; gap: 6px"
        :style="{ gridTemplateColumns: `repeat(${gridDims[1]}, 1fr)`, gridTemplateRows: `repeat(${gridDims[0]}, 1fr)` }"
      >
        <div
          v-for="path in session.current"
          :key="path"
          style="position: relative; min-width: 0; min-height: 0; background: #16181d; border-radius: 6px; overflow: hidden; cursor: pointer"
          :style="selected.has(path) ? 'outline: 3px solid #d6ff73' : ''"
          @click="tapTile(path)"
          @contextmenu.prevent="longPress(path)"
        >
          <img v-if="displayUrls[path]" :src="displayUrls[path]" style="width: 100%; height: 100%; object-fit: contain">
          <span v-if="session.members[path]" class="text-caption" style="position: absolute; left: 4px; top: 4px; background: rgba(0,0,0,.6); padding: 0 4px; cursor: pointer" @click.stop="burstEditTarget = path">
            ⧉{{ session.members[path]!.length }}
          </span>
          <div style="position: absolute; right: 4px; top: 4px; display: flex; flex-direction: column; gap: 4px">
            <v-btn icon="mdi-magnify" size="x-small" variant="text" style="background: rgba(0,0,0,.4)" @click.stop="openZoom(path)" />
            <v-btn icon="mdi-star" size="x-small" variant="text" style="background: rgba(0,0,0,.4)" @click.stop="keepTop5(path)" />
          </div>
        </div>
      </div>
    </div>

    <!-- ラウンド完了 -->
    <div v-else-if="phase === 'roundComplete' && session" class="pa-6 flex-grow-1 d-flex flex-column align-center justify-center">
      <p class="text-h5 mb-2">★{{ session.target_star }} が {{ kept }} 枚残りました</p>
      <p class="text-body-2 text-medium-emphasis mb-4">{{ photos.length }} 枚から {{ kept }} 枚に。{{ elapsedText }}</p>
      <div class="d-flex ga-2">
        <v-btn variant="tonal" @click="finishToResults">結果を見る</v-btn>
        <v-btn color="primary" @click="nextRound">次のラウンドへ</v-btn>
      </div>
    </div>

    <BurstEditSheet
      v-if="burstEditTarget && session"
      :session="session"
      :representative="burstEditTarget"
      :photos="photoRefs"
      :threshold="threshold()"
      :group-bursts="groupBursts"
      @close="burstEditTarget = null"
      @apply="afterBurstEdit"
    />

    <!-- 選別中の「…」（optionsSheet。02章）。枚数・連写まとめは、いまの組にすぐ効く。 -->
    <v-dialog v-model="optionsOpen" max-width="420">
      <v-card title="選別の設定">
        <v-card-text>
          <p class="text-body-2 mb-1">一度に見比べる枚数</p>
          <p class="text-caption text-medium-emphasis mb-2">いまのグループにすぐ効きます。選んだ印は残ります</p>
          <div class="d-flex flex-wrap ga-2 mb-4">
            <v-btn
              v-for="size in groupSizeChoices"
              :key="size"
              :color="session && size === session.group_size ? 'primary' : undefined"
              :variant="session && size === session.group_size ? 'flat' : 'outlined'"
              size="small"
              style="min-width: 40px"
              @click="applyGroupSize(size)"
            >{{ size }}</v-btn>
          </div>
          <v-switch
            :model-value="groupBursts"
            label="似た連写をまとめて1枚として見る"
            color="primary"
            hide-details
            @update:model-value="(v) => applyGroupBursts(!!v)"
          />
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="optionsOpen = false">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <ZoomView
      v-if="zoomIndex !== null && session"
      :paths="session.current"
      :index="zoomIndex"
      :display-urls="displayUrls"
      :resolve-original="(p) => backend.originalUrl(projectId, p)"
      :file-size="(p) => photoByPath[p]?.size ?? null"
      :show-keep="true"
      @close="zoomIndex = null"
      @move="(i) => (zoomIndex = i)"
      @keep="zoomKeep"
    />
  </div>
</template>
