<script setup lang="ts">
// 選別結果（設計 02 章）。星チップで絞る／並べ替え。格子（<1100は4列/≥1100は6列）。
import { computed, onBeforeUnmount, onMounted, ref, toRaw } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useCapabilities } from '~/composables/useCapabilities'
import { useLayout } from '~/composables/useLayout'
import { registerAutoPush } from '~/composables/useSidecarSync'
import type { Project, ProjectPhoto } from '~/types/project'
import type { Session } from '~/lib/core'
import { comparePhotos, filterPhotos, summarizeRatings } from '~/utils/photoQuery'
import type { PhotoSort } from '~/types/photo'
import ZoomView from '~/components/ZoomView.vue'
import BurstReviewView from '~/components/BurstReviewView.vue'

definePageMeta({ layout: 'default' })

const backend = useBackend()
const router = useRouter()
const route = useRoute()
const { isWide } = useLayout()
const { capabilities } = useCapabilities()

const projectId = computed(() => String(route.params.id))
const project = ref<Project | null>(null)
const photos = ref<ProjectPhoto[]>([])
const session = ref<Session | null>(null)
const thumbUrls = ref<Record<string, string>>({})
const photoByPath = computed(() => Object.fromEntries(photos.value.map(p => [p.relativePath, p])))
/** Amazon 出所は端末にフォルダも原本も無いので、フォルダ分け・XMP は選べない（08章）。 */
const isAmazon = computed(() => project.value?.source.kind === 'amazon')

// プロジェクト詳細の「星の行」から来たときは、その星で絞った状態で開く
// （02章「プロジェクト詳細」の遷移表「星の行 → results（その星で絞る）」）。
// クエリで星の指定が無いときは、既定でその結果内の一番高い星に絞る
// （ユーザー要望。Android版 Results.kt は既定「すべて」だが、PC/Web版は
// 「結果を見に来たらまず一番良い写真から」を優先してあえて分ける。07章参照）。
const initialStar = Number(route.query.star)
const hasQueryStar = Number.isFinite(initialStar) && initialStar >= 0 && initialStar <= 5
const filterStar = ref<number | null>(hasQueryStar ? initialStar : null)
const sort = ref<PhotoSort>('rating')
const selected = ref<Set<string>>(new Set())
const multiMode = ref(false)
const zoomIndex = ref<number | null>(null)
const menu = ref(false)
const exportBusy = ref(false)
const exportMessage = ref('')
// 「最初からやり直す」（02章「results」の「…」）。押した場所で即やり直せるように
// ここで確認ダイアログを挟んで実行する（プロジェクト詳細への遷移だけでは終わらない）。
const restartOpen = ref(false)
const restarting = ref(false)
const techOpen = ref(false)
// 連写の中身選別（02章 burst-review。app-android BurstReview.kt が見本）。
// **選別画面と同じ「見るだけの拡大」ではなく、専用の画面。**
const reviewTarget = ref<string | null>(null)

interface Row { relativePath: string; rating: number; capturedAt: number | null; burstSize: number }

const rows = computed<Row[]>(() => {
  if (!session.value) return []
  // 連写は代表 1 タイル。出すのはその組で星が一番高い 1 枚（07章の決定）。
  const seen = new Set<string>()
  const out: Row[] = []
  for (const photo of photos.value) {
    if (seen.has(photo.relativePath)) continue
    const mates = session.value.members[photo.relativePath]
    if (mates) {
      let best = mates[0]!
      let bestRating = -1
      for (const m of mates) {
        seen.add(m)
        const r = session.value.ratings[m] ?? 0
        if (r > bestRating) { bestRating = r; best = m }
      }
      const bestPhoto = photos.value.find(p => p.relativePath === best) ?? photo
      out.push({ relativePath: bestPhoto.relativePath, rating: bestRating, capturedAt: bestPhoto.capturedAt, burstSize: mates.length })
    } else {
      seen.add(photo.relativePath)
      out.push({ relativePath: photo.relativePath, rating: session.value.ratings[photo.relativePath] ?? 0, capturedAt: photo.capturedAt, burstSize: 1 })
    }
  }
  return out
})

const summary = computed(() => summarizeRatings(rows.value))
const filteredSorted = computed(() => {
  const filtered = filterPhotos(rows.value.map(r => ({ ...r, isMissing: false })), filterStar.value)
  return filtered.sort(comparePhotos<Row>(filterStar.value === null ? sort.value : 'name'))
})
const zoomPaths = computed(() => filteredSorted.value.map(r => r.relativePath))

async function load() {
  project.value = await backend.getProject(projectId.value)
  if (!project.value) return
  photos.value = await backend.listPhotos(projectId.value)
  session.value = await backend.loadSession(projectId.value)
  // クエリで星を指定されていなければ、実際に付いている一番高い星に絞る。
  // 星が1枚も付いていなければ（全部★0）「すべて」のまま。
  if (!hasQueryStar) {
    const highest = [5, 4, 3, 2, 1].find(s => (summary.value.counts[s] ?? 0) > 0)
    if (highest !== undefined) filterStar.value = highest
  }
  const paths = filteredSorted.value.slice(0, 300).map(r => r.relativePath)
  const entries = await Promise.all(paths.map(async p => [p, await backend.displayUrl(projectId.value, p)] as const))
  thumbUrls.value = Object.fromEntries(entries.filter(([, u]) => u) as [string, string][])
}
onMounted(load)

// 書き時「画面を離れる・背面へ回る・窓を閉じる」（設計02章）。結果画面での★上げ下げ・
// 連写中身選別・やり直しも、ここを離れるときに書く。
const stopAutoPush = registerAutoPush(() => project.value)
onBeforeUnmount(() => stopAutoPush())

function tapTile(row: Row) {
  if (multiMode.value) {
    const set = new Set(selected.value)
    if (set.has(row.relativePath)) set.delete(row.relativePath); else set.add(row.relativePath)
    selected.value = set
    return
  }
  // 連写の組（⧉）は中身選別、単独は拡大（02章「タイル タップ」）。
  if (row.burstSize > 1) {
    const mates = session.value?.members[row.relativePath] ?? [row.relativePath]
    reviewTarget.value = row.relativePath
    void loadReviewUrls(mates)
    return
  }
  openZoom(row.relativePath)
}
function longPress(row: Row) {
  multiMode.value = true
  const set = new Set(selected.value)
  set.add(row.relativePath)
  selected.value = set
}

function openZoom(path: string) {
  const at = zoomPaths.value.indexOf(path)
  if (at >= 0) zoomIndex.value = at
}

// 連写の中身選別（BurstReviewView）を開いている間の拡大は、結果一覧の並び
// （zoomPaths）ではなく、そのまとまりの中身（reviewMembers）を対象にする。
const reviewZooming = ref(false)
function openReviewZoom(index: number) {
  reviewZooming.value = true
  zoomIndex.value = index
}
function closeZoom() {
  zoomIndex.value = null
  reviewZooming.value = false
}

// ---- 連写の中身選別（burst-review） ----
const reviewMembers = computed<string[]>(() => {
  if (!reviewTarget.value || !session.value) return []
  const mates = session.value.members[reviewTarget.value] ?? [reviewTarget.value]
  const order = new Map(photos.value.map((p, i) => [p.relativePath, i]))
  return [...mates].sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0))
})
const reviewBaseStar = computed(() => {
  if (!reviewTarget.value || !session.value) return 0
  return session.value.ratings[reviewTarget.value] ?? 0
})
async function loadReviewUrls(paths: string[]) {
  const missing = paths.filter(p => !thumbUrls.value[p])
  if (missing.length === 0) return
  const entries = await Promise.all(missing.map(async p => [p, await backend.displayUrl(projectId.value, p)] as const))
  for (const [p, url] of entries) if (url) thumbUrls.value[p] = url
}
// 決めるまでデータは変えない。選んだ写真だけ ratings を更新（連写の仲間は道連れにしない）。
//
// **必ず toRaw(session.value) から組み立てる。** session は ref なので、代入した
// オブジェクトは Vue が中身まで reactive（Proxy）にする。session.value を直接
// スプレッドすると members・ratings 等の中身が Proxy のまま新しいオブジェクトに
// 乗り移り、webFolder 版（IndexedDB の structured clone）で
// 「DataCloneError: could not be cloned」となって保存に失敗する
// （tauri 版は JSON 経由の invoke なので気づかれなかった）。
async function applyReview(changes: Record<string, number>) {
  if (!session.value) return
  const raw = toRaw(session.value)
  const next: Session = { ...raw, ratings: { ...raw.ratings, ...changes } }
  session.value = next
  await backend.saveSession(projectId.value, next)
  reviewTarget.value = null
}

async function bumpRating(path: string | null, delta: number) {
  if (!path || !session.value) return
  const raw = toRaw(session.value)
  const next: Session = { ...raw, ratings: { ...raw.ratings } }
  const current = next.ratings[path] ?? 0
  next.ratings[path] = Math.max(0, Math.min(5, current + delta))
  session.value = next
  await backend.saveSession(projectId.value, next)
}

// 取り出し先の対象（選択があればその分だけ、無ければ絞り込み後の全部）。
const exportTargets = computed(() =>
  (selected.value.size > 0 ? filteredSorted.value.filter(r => selected.value.has(r.relativePath)) : filteredSorted.value)
    .map(r => ({ relativePath: r.relativePath, rating: r.rating, capturedAt: r.capturedAt }))
)

// フォルダ分け・XMP は確認を挟む（06 章 A-2/A-3）。CSV・ZIP・共有は原本に触れないので即実行。
const confirmKind = ref<'folders' | 'xmp' | null>(null)

function requestExport(kind: 'folders' | 'xmp' | 'csv' | 'zip' | 'share') {
  menu.value = false
  if (kind === 'folders' || kind === 'xmp') { confirmKind.value = kind; return }
  void doExport(kind)
}

async function confirmExport() {
  const kind = confirmKind.value
  confirmKind.value = null
  if (kind) await doExport(kind)
}

function downloadBlobAs(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = fileName
  a.click()
  URL.revokeObjectURL(url)
}

async function doExport(kind: 'folders' | 'xmp' | 'csv' | 'zip' | 'share') {
  if (!project.value) return
  exportBusy.value = true
  try {
    const targets = exportTargets.value
    if (kind === 'folders') {
      const report = await backend.exportFolders(projectId.value, targets)
      exportMessage.value = report.errors[0] ?? `${report.processed} 枚を書き出しました。`
    } else if (kind === 'xmp') {
      const report = await backend.exportXmp(projectId.value, targets)
      exportMessage.value = report.errors[0] ?? `${report.processed} 枚に星を書き込みました。`
    } else if (kind === 'csv') {
      const blob = await backend.exportCsv(projectId.value, targets)
      downloadBlobAs(blob, `${project.value.name}.csv`)
      exportMessage.value = 'CSV を書き出しました。'
    } else if (kind === 'zip') {
      const blob = await backend.exportZip(projectId.value, targets)
      downloadBlobAs(blob, `${project.value.name}.zip`)
      exportMessage.value = 'ZIP を書き出しました。'
    } else {
      const { shareFiles, canShareFiles } = await import('~/utils/shareExport')
      const files = await backend.shareTargets(projectId.value, targets)
      if (!canShareFiles(files)) {
        exportMessage.value = 'この端末では共有できません。ZIP をお使いください。'
      } else {
        const outcome = await shareFiles(files, project.value.name)
        exportMessage.value = outcome === 'shared' ? '共有しました。' : outcome === 'cancelled' ? '' : 'この端末では共有できません。ZIP をお使いください。'
      }
    }
  } finally {
    exportBusy.value = false
  }
}

// 「最初からやり直す」。app-android の Album.kt の確認文と同じ考え方
// （何が消えるかを具体的に言う）。実行後は詳細画面へ（選別はまだ開始していない状態）。
function openRestart() {
  menu.value = false
  restartOpen.value = true
}
async function confirmRestart() {
  restarting.value = true
  try {
    await backend.restartProject(projectId.value)
    restartOpen.value = false
    router.push(`/project/${projectId.value}`)
  } finally {
    restarting.value = false
  }
}

const hasBursts = computed(() => rows.value.some(r => r.burstSize > 1))
// Android版 Results.kt の StarChip と同じく、件数付きの文言で揃える
// （「すべて」にも合計枚数を出す）。
const starChips = computed(() => {
  const chips: { label: string; value: number | null }[] = [{ label: `すべて ${summary.value.total}`, value: null }]
  for (let s = 5; s >= 1; s -= 1) {
    if ((summary.value.counts[s] ?? 0) > 0) chips.push({ label: `★${s} · ${summary.value.counts[s]}`, value: s })
  }
  return chips
})
// 取り出しボタンの文言。Android版 Results.kt の targetLabel と同じ考え方
// （「[対象] を…」。選んでいなければいまの絞り込みの対象を言う）。
const targetLabel = computed(() => {
  if (selected.value.size > 0) return `選んだ ${selected.value.size} 枚`
  if (filterStar.value === null) return `すべて ${filteredSorted.value.length} 枚`
  return `★${filterStar.value} ${filteredSorted.value.length} 枚`
})
</script>

<template>
  <div v-if="project" class="pa-4">
    <div class="d-flex align-center mb-3">
      <v-btn icon="mdi-arrow-left" variant="text" :to="`/project/${projectId}`" />
      <span class="text-h6 ml-2 flex-grow-1">結果 · {{ project.name }}</span>
      <v-menu v-model="menu">
        <template #activator="{ props: menuProps }">
          <v-btn icon="mdi-dots-vertical" variant="text" v-bind="menuProps" />
        </template>
        <v-list>
          <v-list-item title="最初からやり直す" @click="openRestart" />
          <v-list-item title="技術情報" @click="menu = false; techOpen = true" />
        </v-list>
      </v-menu>
      <v-menu>
        <template #activator="{ props: outProps }">
          <v-btn color="primary" class="ml-2" prepend-icon="mdi-export-variant" :loading="exportBusy" v-bind="outProps">
            {{ targetLabel }} を…
          </v-btn>
        </template>
        <v-list>
          <v-list-item
            v-if="!isAmazon"
            :disabled="!capabilities.exportFolders"
            :title="capabilities.exportFolders ? 'フォルダ分けしてコピー' : 'フォルダ分け（PC 版で使えます）'"
            @click="requestExport('folders')"
          />
          <v-list-item
            v-if="!isAmazon"
            :disabled="!capabilities.writeMetadata"
            :title="capabilities.writeMetadata ? '原本の XMP に星を書く' : 'XMP に星（PC 版で使えます）'"
            @click="requestExport('xmp')"
          />
          <v-list-item v-if="capabilities.share" title="共有" @click="requestExport('share')" />
          <!-- PC 通常は exportFolders が出口。Amazon 出所だけは端末にフォルダが無いので ZIP を出口にする（08章）。 -->
          <v-list-item v-if="capabilities.exportZip || isAmazon" title="ZIP を書き出す" @click="requestExport('zip')" />
          <v-list-item title="CSV を書き出す" @click="requestExport('csv')" />
        </v-list>
      </v-menu>
    </div>

    <v-alert v-if="exportMessage" type="info" density="compact" class="mb-3" closable @click:close="exportMessage = ''">
      {{ exportMessage }}
    </v-alert>

    <v-dialog :model-value="confirmKind !== null" max-width="440" @update:model-value="(v) => { if (!v) confirmKind = null }">
      <v-card v-if="confirmKind === 'folders'" title="フォルダ分けしてコピーしますか">
        <v-card-text>
          <p>{{ exportTargets.length }} 枚を星ごとのフォルダ（<code>star-0</code>〜<code>star-5</code>）へコピーします。元の場所の写真はそのまま残ります。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="confirmKind = null">やめる</v-btn>
          <v-btn color="primary" @click="confirmExport">コピーする</v-btn>
        </v-card-actions>
      </v-card>
      <v-card v-else-if="confirmKind === 'xmp'" title="原本に星を書き込みますか">
        <v-card-text>
          <p class="text-error">原本を書き換えます。{{ exportTargets.length }} 枚の写真に、星を XMP（xmp:Rating）として直接書き込みます。取り消せません。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="confirmKind = null">やめる</v-btn>
          <v-btn color="error" @click="confirmExport">書き込む</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="restartOpen" max-width="420" @update:model-value="(v) => { if (!v) restartOpen = v }">
      <v-card title="選別を最初からやり直しますか">
        <v-card-text>
          <p class="text-error">星と進捗が消えます。連写のまとめ方もリセットされ、次回また質問します。写真そのものは変更しません。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" :disabled="restarting" @click="restartOpen = false">キャンセル</v-btn>
          <v-btn color="error" :loading="restarting" @click="confirmRestart">やり直す</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="techOpen" max-width="420" @update:model-value="(v) => (techOpen = v)">
      <v-card title="技術情報">
        <v-card-text>
          <div class="text-body-2">id: {{ project.id }}</div>
          <div class="text-body-2">出所: {{ project.source.kind }} / {{ project.source.key }}</div>
          <div class="text-body-2">枚数: {{ project.photoCount }}</div>
          <div class="text-body-2">連写の境目: {{ project.burstDistance ?? '未学習' }}</div>
          <div v-if="session" class="text-body-2">ROUND: {{ session.round }}（★{{ session.target_star }} を選別中）</div>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="techOpen = false">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <div class="d-flex flex-wrap align-center mb-2" style="gap: 6px">
      <v-chip
        v-for="chip in starChips"
        :key="String(chip.value)"
        :color="filterStar === chip.value ? 'lime' : undefined"
        variant="flat"
        size="small"
        @click="filterStar = chip.value"
      >
        {{ chip.label }}
      </v-chip>
      <v-divider vertical class="mx-1" />
      <template v-if="filterStar === null">
        <span class="text-caption text-medium-emphasis">並べ替え</span>
        <v-chip :color="sort === 'rating' ? 'secondary' : undefined" size="small" @click="sort = 'rating'">星が高い順</v-chip>
        <v-chip :color="sort === 'name' ? 'secondary' : undefined" size="small" @click="sort = 'name'">撮影順</v-chip>
      </template>
    </div>

    <div class="d-flex mb-4" style="height: 6px; border-radius: 3px; overflow: hidden">
      <div
        v-for="s in [5, 4, 3, 2, 1]"
        :key="s"
        :style="`flex: ${summary.counts[s] || 0}; background: hsl(${s * 24}, 70%, 55%)`"
      />
      <div :style="`flex: ${summary.counts[0] || 1}; background: #333`" />
    </div>

    <p v-if="hasBursts" class="text-caption text-medium-emphasis mb-2">⧉ の付いた組は押すと中身を選別できます</p>

    <div class="d-flex flex-wrap" style="gap: 4px">
      <div
        v-for="row in filteredSorted"
        :key="row.relativePath"
        style="position: relative; width: 140px; height: 140px; background: #16181d; border-radius: 4px; overflow: hidden; cursor: pointer"
        :style="selected.has(row.relativePath) ? 'outline: 3px solid #d6ff73' : (row.burstSize > 1 ? 'outline: 2px solid white' : '')"
        @click="tapTile(row)"
        @contextmenu.prevent="longPress(row)"
      >
        <img v-if="thumbUrls[row.relativePath]" :src="thumbUrls[row.relativePath]" style="width: 100%; height: 100%; object-fit: cover">
        <span class="text-caption" style="position: absolute; left: 4px; bottom: 4px; background: rgba(0,0,0,.6); padding: 0 4px">★{{ row.rating }}</span>
        <span v-if="row.burstSize > 1" class="text-caption" style="position: absolute; left: 4px; top: 4px; background: rgba(0,0,0,.6); padding: 0 4px">⧉{{ row.burstSize }}</span>
      </div>
    </div>

    <p class="text-caption text-medium-emphasis text-center mt-6">
      {{ capabilities.sidecar ? '星は写真のフォルダにも記録します（.photo-curator）' : 'この端末だけの結果' }}
    </p>

    <ZoomView
      v-if="zoomIndex !== null"
      :paths="reviewZooming ? reviewMembers : zoomPaths"
      :index="zoomIndex"
      :display-urls="thumbUrls"
      :resolve-original="(p) => backend.originalUrl(projectId, p)"
      :file-size="(p) => photoByPath[p]?.size ?? null"
      :show-keep="false"
      @close="closeZoom"
      @move="(i) => (zoomIndex = i)"
    >
      <template #extra="{ path }">
        <v-btn icon="mdi-minus" variant="tonal" @click="bumpRating(path, -1)" />
        <v-btn icon="mdi-plus" variant="tonal" @click="bumpRating(path, 1)" />
      </template>
    </ZoomView>

    <BurstReviewView
      v-if="reviewTarget"
      :members="reviewMembers"
      @zoom="openReviewZoom"
      :base-star="reviewBaseStar"
      :display-urls="thumbUrls"
      @close="reviewTarget = null"
      @apply="applyReview"
    />
  </div>
  <div v-else class="text-center pa-8">
    <v-progress-circular indeterminate color="primary" />
  </div>
</template>
