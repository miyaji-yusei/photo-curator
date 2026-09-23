<script setup lang="ts">
// 選別（本体）＋開始シート／基準学習／まとまり確認／ラウンド完了（設計 02 章）。
// **帯は上の1本だけ。アプリバーは出さない**（layouts/focus.vue）。
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useCapabilities } from '~/composables/useCapabilities'
import { useAppStore } from '~/stores/app'
import * as core from '~/lib/core'
import type { PhotoRef, Session, BurstAnswer, BurstThreshold, PairOverride } from '~/lib/core'
import type { ProjectPhoto, Project } from '~/types/project'
import { buildBurstQuestions } from '~/utils/burstQuestions'
import { groupSizeLimits, clampGroupSize } from '~/utils/groupSize'
import BurstEditSheet from '~/components/BurstEditSheet.vue'

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
  if (next.finished) phase.value = 'roundComplete'
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
  if (!capabilities.keyboard || phase.value !== 'tournament' || !session.value) return
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
    <div v-else-if="phase === 'learn'" class="pa-6 flex-grow-1 d-flex flex-column" style="max-width: 640px; margin: 0 auto">
      <p class="text-h6 mb-4">この2枚は同じ連写ですか？（{{ questionIndex + 1 }} / {{ questions.length }}）</p>
      <div v-if="questions[questionIndex]" class="d-flex flex-grow-1 ga-4 justify-center align-center">
        <img
          v-for="side in [questions[questionIndex]!.left, questions[questionIndex]!.right]"
          :key="side.relative_path"
          :src="displayUrls[side.relative_path]"
          style="max-width: 45%; max-height: 60vh; object-fit: contain"
        >
      </div>
      <div class="d-flex ga-2 justify-center mt-4">
        <v-btn color="primary" @click="answerQuestion(true)">同じ</v-btn>
        <v-btn variant="tonal" @click="answerQuestion(false)">別</v-btn>
      </div>
      <v-btn variant="text" class="mt-4" @click="skipLearning">残りをスキップ</v-btn>
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
    <div v-else-if="phase === 'tournament' && session" class="d-flex flex-column flex-grow-1">
      <div class="d-flex align-center px-3" style="height: 56px; flex-shrink: 0">
        <v-btn icon="mdi-arrow-left" variant="text" :to="`/project/${projectId}`" />
        <v-btn icon="mdi-undo" variant="text" :disabled="session.history.length === 0" @click="undo" />
        <v-btn :icon="multiMode ? 'mdi-checkbox-multiple-marked' : 'mdi-checkbox-multiple-blank-outline'" variant="text" @click="multiMode = !multiMode" />
        <span class="flex-grow-1 text-center">★{{ session.target_star }} を選別中 · ROUND {{ session.round }}</span>
        <v-btn
          color="primary"
          variant="flat"
          @click="confirmGroup"
        >
          {{ selected.size === 0 ? (session.current.length + '枚とも落とす') : (selected.size + '枚を残す') }}
        </v-btn>
      </div>
      <v-progress-linear :model-value="((photos.length - session.queue.length - session.current.length) / Math.max(1, photos.length)) * 100" height="2" color="primary" />
      <p class="text-center text-caption text-medium-emphasis my-1">
        残り {{ session.queue.length + session.current.length }} 枚
      </p>
      <div class="flex-grow-1 d-flex flex-wrap align-center justify-center ga-2 pa-2" style="overflow: auto">
        <div
          v-for="path in session.current"
          :key="path"
          style="position: relative; width: min(45vw, 45vh); height: min(45vw, 45vh); background: #16181d; border-radius: 6px; overflow: hidden; cursor: pointer"
          :style="selected.has(path) ? 'outline: 3px solid #d6ff73' : ''"
          @click="tapTile(path)"
          @contextmenu.prevent="longPress(path)"
        >
          <img v-if="displayUrls[path]" :src="displayUrls[path]" style="width: 100%; height: 100%; object-fit: contain">
          <span v-if="session.members[path]" class="text-caption" style="position: absolute; left: 4px; top: 4px; background: rgba(0,0,0,.6); padding: 0 4px; cursor: pointer" @click.stop="burstEditTarget = path">
            ⧉{{ session.members[path]!.length }}
          </span>
          <v-btn icon="mdi-star" size="x-small" variant="text" style="position: absolute; right: 4px; top: 4px" @click.stop="keepTop5(path)" />
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
      @close="burstEditTarget = null"
      @apply="afterBurstEdit"
    />
  </div>
</template>
