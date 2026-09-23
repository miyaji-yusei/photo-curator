<script setup lang="ts">
// 選別結果（設計 02 章）。星チップで絞る／並べ替え。格子（<1100は4列/≥1100は6列）。
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useCapabilities } from '~/composables/useCapabilities'
import { useLayout } from '~/composables/useLayout'
import type { Project, ProjectPhoto } from '~/types/project'
import type { Session } from '~/lib/core'
import { comparePhotos, filterPhotos, summarizeRatings } from '~/utils/photoQuery'
import type { PhotoSort } from '~/types/photo'

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

const filterStar = ref<number | null>(null)
const sort = ref<PhotoSort>('rating')
const selected = ref<Set<string>>(new Set())
const multiMode = ref(false)
const zoomPath = ref<string | null>(null)
const zoomUrl = ref<string | null>(null)
const menu = ref(false)
const exportBusy = ref(false)
const exportMessage = ref('')

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

async function load() {
  project.value = await backend.getProject(projectId.value)
  if (!project.value) return
  photos.value = await backend.listPhotos(projectId.value)
  session.value = await backend.loadSession(projectId.value)
  const paths = filteredSorted.value.slice(0, 300).map(r => r.relativePath)
  const entries = await Promise.all(paths.map(async p => [p, await backend.displayUrl(projectId.value, p)] as const))
  thumbUrls.value = Object.fromEntries(entries.filter(([, u]) => u) as [string, string][])
}
onMounted(load)

function tapTile(row: Row) {
  if (multiMode.value) {
    const set = new Set(selected.value)
    if (set.has(row.relativePath)) set.delete(row.relativePath); else set.add(row.relativePath)
    selected.value = set
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

async function openZoom(path: string) {
  zoomPath.value = path
  zoomUrl.value = thumbUrls.value[path] ?? await backend.displayUrl(projectId.value, path)
}

async function bumpRating(path: string, delta: number) {
  if (!session.value) return
  const next: Session = { ...session.value, ratings: { ...session.value.ratings } }
  const current = next.ratings[path] ?? 0
  next.ratings[path] = Math.max(0, Math.min(5, current + delta))
  session.value = next
  await backend.saveSession(projectId.value, next)
}

async function doExport(kind: 'folders' | 'xmp' | 'csv') {
  if (!project.value) return
  exportBusy.value = true
  menu.value = false
  try {
    const targets = (selected.value.size > 0 ? filteredSorted.value.filter(r => selected.value.has(r.relativePath)) : filteredSorted.value)
      .map(r => ({ relativePath: r.relativePath, rating: r.rating, capturedAt: r.capturedAt }))
    if (kind === 'folders') {
      const report = await backend.exportFolders(projectId.value, targets)
      exportMessage.value = report.errors[0] ?? `${report.processed} 枚を書き出しました。`
    } else if (kind === 'xmp') {
      const report = await backend.exportXmp(projectId.value, targets)
      exportMessage.value = report.errors[0] ?? `${report.processed} 枚に星を書き込みました。`
    } else {
      const blob = await backend.exportCsv(projectId.value, targets)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `${project.value.name}.csv`
      a.click()
      URL.revokeObjectURL(url)
      exportMessage.value = 'CSV を書き出しました。'
    }
  } finally {
    exportBusy.value = false
  }
}

const hasBursts = computed(() => rows.value.some(r => r.burstSize > 1))
const starChips = computed(() => {
  const chips: { label: string; value: number | null }[] = [{ label: 'すべて', value: null }]
  for (let s = 5; s >= 1; s -= 1) {
    if ((summary.value.counts[s] ?? 0) > 0) chips.push({ label: `★${s} · ${summary.value.counts[s]}`, value: s })
  }
  return chips
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
          <v-list-item title="最初からやり直す" @click="router.push(`/project/${projectId}`); menu = false" />
          <v-list-item title="技術情報" @click="menu = false" />
        </v-list>
      </v-menu>
      <v-menu>
        <template #activator="{ props: outProps }">
          <v-btn color="primary" class="ml-2" :loading="exportBusy" v-bind="outProps">
            {{ selected.size > 0 ? selected.size + ' 枚' : 'すべて' }} を…
          </v-btn>
        </template>
        <v-list>
          <v-list-item
            :disabled="!capabilities.exportFolders"
            :title="capabilities.exportFolders ? 'フォルダ分けしてコピー' : 'フォルダ分け（PC 版で使えます）'"
            @click="doExport('folders')"
          />
          <v-list-item
            :disabled="!capabilities.writeMetadata"
            :title="capabilities.writeMetadata ? '原本の XMP に星を書く' : 'XMP に星（PC 版で使えます）'"
            @click="doExport('xmp')"
          />
          <v-list-item title="CSV を書き出す" @click="doExport('csv')" />
        </v-list>
      </v-menu>
    </div>

    <v-alert v-if="exportMessage" type="info" density="compact" class="mb-3" closable @click:close="exportMessage = ''">
      {{ exportMessage }}
    </v-alert>

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

    <v-dialog :model-value="!!zoomPath" max-width="900" @update:model-value="(v) => { if (!v) zoomPath = null }">
      <v-card v-if="zoomPath">
        <img :src="zoomUrl ?? ''" style="width: 100%; max-height: 80vh; object-fit: contain">
        <v-card-actions>
          <v-btn icon="mdi-minus" @click="bumpRating(zoomPath, -1)" />
          <v-btn icon="mdi-plus" @click="bumpRating(zoomPath, 1)" />
          <v-spacer />
          <v-btn @click="zoomPath = null">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
  <div v-else class="text-center pa-8">
    <v-progress-circular indeterminate color="primary" />
  </div>
</template>
