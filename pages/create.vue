<script setup lang="ts">
// プロジェクトを作成（設計 02 章）。**画面にする（シートにしない）**。
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useCapabilities } from '~/composables/useCapabilities'
import { useLayout } from '~/composables/useLayout'
import type { FolderEntry } from '~/lib/backend'
import type { Project, ProjectSource } from '~/types/project'

definePageMeta({ layout: 'default' })

const backend = useBackend()
const router = useRouter()
const { isWide } = useLayout()
const { capabilities, environment } = useCapabilities()

const tab = ref<'folder' | 'amazon'>('folder')
const devPath = ref('')
const root = ref<ProjectSource | null>(null)
const subPath = ref('')
const entries = ref<FolderEntry[]>([])
const loadingEntries = ref(false)
const errorText = ref('')
const existingKeys = ref<Set<string>>(new Set())

const selected = ref<{ name: string; key: string } | null>(null)
const projectName = ref('')
const creating = ref(false)
// 見本の絵（02章「見本の絵」節）。選んだフォルダの「枚数・見本12枚」。
const sampleCount = ref<number | null>(null)
const sampleThumbs = ref<string[]>([])
const sampleLoading = ref(false)

// Amazon Photos（PC だけ。capabilities.amazon）。共有リンクを貼って下見してから作成する（08章）。
const amazonUrl = ref('')
const amazonLoading = ref(false)
const amazonSource = ref<ProjectSource | null>(null)

async function loadAmazon() {
  if (!amazonUrl.value.trim() || !backend.amazonPreview) return
  amazonLoading.value = true
  errorText.value = ''
  amazonSource.value = null
  selected.value = null
  sampleCount.value = null
  sampleThumbs.value = []
  try {
    const preview = await backend.amazonPreview(amazonUrl.value.trim())
    amazonSource.value = { kind: 'amazon', key: preview.key, label: preview.name }
    selected.value = { name: preview.name, key: preview.key }
    projectName.value = preview.name
    sampleCount.value = preview.count
    sampleThumbs.value = preview.samples
  } catch (cause) {
    errorText.value = cause instanceof Error ? cause.message : 'リンクを読めませんでした。'
  } finally {
    amazonLoading.value = false
  }
}

// Web（ピッカー）。フォルダが無いので「写真を選ぶ」→ その場で取り込む
// （02章「PC・Webの差分」表: 作成の左側は「写真を選ぶ」ボタン）。
const pickedFiles = ref<File[]>([])
const fileInput = ref<HTMLInputElement | null>(null)
function openFilePicker() {
  fileInput.value?.click()
}
function onFilesPicked(event: Event) {
  const input = event.target as HTMLInputElement
  const files = Array.from(input.files ?? []).filter(f => f.type.startsWith('image/'))
  pickedFiles.value = files
  sampleThumbs.value.forEach(url => URL.revokeObjectURL(url))
  sampleThumbs.value = files.slice(0, 12).map(f => URL.createObjectURL(f))
  sampleCount.value = files.length
  selected.value = files.length > 0 ? { name: '選んだ写真', key: 'picker' } : null
  projectName.value = files.length > 0 ? `写真 ${new Date().toLocaleDateString('ja-JP')}` : ''
  input.value = ''
}

onMounted(async () => {
  const projects = await backend.listProjects()
  existingKeys.value = new Set(projects.map((p: Project) => p.source.key))
})

// タブを切り替えたら選択を捨てる（前のタブの選択を引きずって作成しないため）。
watch(tab, () => {
  selected.value = null
  projectName.value = ''
  sampleCount.value = null
  sampleThumbs.value = []
  amazonSource.value = null
  errorText.value = ''
})

async function pickNative() {
  errorText.value = ''
  const source = await backend.pickFolder()
  if (!source) return
  root.value = source
  subPath.value = ''
  selected.value = { name: source.label, key: '' }
  projectName.value = source.label
  await loadEntries()
  void loadSample()
}

async function pickDev() {
  if (!devPath.value.trim()) return
  errorText.value = ''
  const source = await backend.pickFolder(devPath.value.trim())
  if (!source) return
  root.value = source
  subPath.value = ''
  selected.value = { name: source.label, key: '' }
  projectName.value = source.label
  await loadEntries()
  void loadSample()
}

/** 選んだフォルダの枚数・見本12枚。**下位フォルダも含めて数える**
 * （実際に準備で拾う範囲と揃える）。原本は読まない。断念（出所に繋がらない等）は
 * 静かに空のまま返す（選別を止めない。02章「断念の条件」）。 */
async function loadSample() {
  if (!root.value || !selected.value) {
    sampleCount.value = null
    sampleThumbs.value = []
    return
  }
  sampleLoading.value = true
  sampleCount.value = null
  sampleThumbs.value = []
  try {
    const result = await backend.sampleFolder(root.value, selected.value.key, 12)
    sampleCount.value = result.count
    sampleThumbs.value = result.samples
  } catch {
    sampleCount.value = null
    sampleThumbs.value = []
  } finally {
    sampleLoading.value = false
  }
}

async function loadEntries() {
  if (!root.value) return
  loadingEntries.value = true
  errorText.value = ''
  try {
    entries.value = await backend.listEntries(root.value, subPath.value)
  } catch (cause) {
    errorText.value = cause instanceof Error ? cause.message : '一覧を読めませんでした。'
    entries.value = []
  } finally {
    loadingEntries.value = false
  }
}

function selectRow(entry: FolderEntry) {
  selected.value = { name: entry.name, key: entry.key }
  projectName.value = entry.name
  void loadSample()
}

async function enter(entry: FolderEntry) {
  subPath.value = entry.key
  await loadEntries()
}

async function up() {
  const parts = subPath.value.split('/').filter(Boolean)
  parts.pop()
  subPath.value = parts.join('/')
  await loadEntries()
}

const alreadyCreated = computed(() => {
  if (tab.value === 'amazon') {
    return !!amazonSource.value && existingKeys.value.has(amazonSource.value.key)
  }
  if (!root.value || !selected.value) return false
  // 相対パスまで含めた鍵で確認済みかを見る（dev モードは root+subPath を連結する）。
  const combined = root.value.key.startsWith('dev:')
    ? `dev:${root.value.key.slice(4)}/${selected.value.key}`.replace(/\/$/, '')
    : selected.value.key
  return existingKeys.value.has(combined) || existingKeys.value.has(root.value.key)
})

async function create() {
  if (!selected.value || !projectName.value.trim()) return
  creating.value = true
  errorText.value = ''
  try {
    if (environment === 'webPicker') {
      if (!backend.importPhotos) throw new Error('この環境では写真の取り込みに対応していません。')
      const project = await backend.importPhotos(projectName.value.trim(), pickedFiles.value)
      await router.push(`/project/${project.id}`)
      return
    }
    if (tab.value === 'amazon') {
      if (!amazonSource.value) return
      const project = await backend.createProject({
        name: projectName.value.trim(),
        source: { ...amazonSource.value, label: projectName.value.trim() }
      })
      await router.push(`/project/${project.id}`)
      return
    }
    if (!root.value) return
    // 選んだ行までを 1 つの出所（プロジェクトの根）にする。
    // root は「フォルダを選ぶ」で選んだ絶対パス、selected.key は「中へ」で
    // 潜った分の相対パス（潜っていなければ空文字）。
    const finalKey = root.value.key.startsWith('dev:')
      ? `dev:${root.value.key.slice(4)}${selected.value.key ? '/' + selected.value.key : ''}`
      : `${root.value.key}${selected.value.key ? '/' + selected.value.key : ''}`
    const source: ProjectSource = { kind: 'folder', key: finalKey, label: selected.value.name }
    const project = await backend.createProject({ name: projectName.value.trim(), source })
    await router.push(`/project/${project.id}`)
  } catch (cause) {
    errorText.value = cause instanceof Error ? cause.message : '作成できませんでした。'
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="d-flex flex-column" style="height: 100vh; overflow: hidden">
    <div v-if="!isWide" class="d-flex align-center pa-3" style="flex-shrink: 0">
      <v-btn icon="mdi-arrow-left" variant="text" @click="router.back()" />
      <span class="text-h6 ml-2">プロジェクトを作成</span>
    </div>
    <span v-else class="text-h5 pa-4 pb-0" style="flex-shrink: 0">プロジェクトを作成</span>

    <v-tabs v-model="tab" class="px-4" style="flex-shrink: 0">
      <v-tab value="folder">フォルダ</v-tab>
      <v-tab value="amazon" :disabled="!capabilities.amazon">
        Amazon Photos
        <span v-if="!capabilities.amazon" class="text-caption ml-1 text-medium-emphasis">（PC 版で使えます）</span>
      </v-tab>
    </v-tabs>

    <!-- フォームが長くなっても（NASタブの一覧が多い等）、この中だけがスクロールし、
         下のキャンセル・作成ボタンは常に画面内に収まる（flex-grow-1 + min-height:0 + overflow-y:auto）。 -->
    <div v-if="tab === 'amazon'" class="pa-6 flex-grow-1" style="max-width: 520px; min-height: 0; overflow-y: auto">
      <v-text-field
        v-model="amazonUrl"
        label="Amazon Photos の共有リンク"
        placeholder="https://www.amazon.co.jp/photos/share/..."
        :disabled="amazonLoading"
        clearable
        @keyup.enter="loadAmazon"
      />
      <v-btn color="primary" variant="tonal" :loading="amazonLoading" :disabled="!amazonUrl.trim()" @click="loadAmazon">
        リンクを読み込む
      </v-btn>
      <v-alert v-if="errorText" type="error" density="compact" class="mt-3">{{ errorText }}</v-alert>
      <p class="text-caption text-medium-emphasis mt-3">
        まず見本だけ読みます（原本はまだ読みません）。選別を始めると、
        写真1枚ごとに必要なぶんだけ Amazon から取ってきます。端末には置きません。
      </p>

      <template v-if="selected">
        <v-text-field v-model="projectName" label="プロジェクト名" clearable class="mt-4" />
        <p v-if="sampleCount !== null" class="text-body-2 mb-1">{{ sampleCount }} 枚</p>
        <div v-if="sampleThumbs.length" class="d-flex ga-1 flex-wrap mb-2">
          <img
            v-for="(url, i) in sampleThumbs"
            :key="i"
            :src="url"
            style="width: 64px; height: 64px; object-fit: cover; border-radius: 4px; flex-shrink: 0"
          >
        </div>
        <v-alert v-if="alreadyCreated" type="warning" density="compact">
          同じ出所のプロジェクトが既にあります。写真は使い回されます。
        </v-alert>
      </template>
    </div>

    <div v-else class="d-flex flex-grow-1" :class="isWide ? 'flex-row' : 'flex-column'" style="min-height: 0">
      <div class="flex-grow-1 pa-4" style="overflow-y: auto">
        <div class="d-flex ga-2 mb-3 flex-wrap">
          <v-btn v-if="capabilities.browseFolders" variant="tonal" prepend-icon="mdi-folder-open" @click="pickNative">
            フォルダを選ぶ
          </v-btn>
          <template v-if="environment === 'webPicker'">
            <input
              ref="fileInput"
              type="file"
              multiple
              accept="image/*"
              style="display: none"
              @change="onFilesPicked"
            >
            <v-btn variant="tonal" prepend-icon="mdi-image-multiple" @click="openFilePicker">
              写真を選ぶ
            </v-btn>
          </template>
          <template v-if="environment === 'webFolder'">
            <v-text-field
              v-model="devPath"
              label="（開発用）フォルダの絶対パス"
              density="compact"
              hide-details
              style="max-width: 360px"
              @keyup.enter="pickDev"
            />
            <v-btn variant="text" @click="pickDev">開く</v-btn>
          </template>
        </div>

        <v-alert v-if="errorText" type="error" density="compact" class="mb-3">{{ errorText }}</v-alert>

        <div v-if="root" class="mb-2 d-flex align-center ga-2">
          <v-btn v-if="subPath" size="small" variant="text" prepend-icon="mdi-arrow-up" @click="up">上へ</v-btn>
          <span class="text-body-2 text-medium-emphasis">{{ root.label }}{{ subPath ? '/' + subPath : '' }}</span>
        </div>

        <div v-if="loadingEntries" class="text-center pa-6">
          <v-progress-circular indeterminate color="primary" />
        </div>

        <v-list v-else-if="root">
          <v-list-item
            v-for="entry in entries"
            :key="entry.key"
            :active="selected?.key === entry.key"
            @click="selectRow(entry)"
          >
            <template #prepend>
              <v-icon icon="mdi-folder" />
            </template>
            <v-list-item-title>{{ entry.name }}</v-list-item-title>
            <v-list-item-subtitle>
              {{ entry.hasPhotos ? '写真あり' : '写真なし' }}
              <span v-if="existingKeys.has(entry.key)"> · 作成済み</span>
            </v-list-item-subtitle>
            <template #append>
              <v-btn v-if="entry.hasChildren" size="small" variant="text" @click.stop="enter(entry)">中へ</v-btn>
            </template>
          </v-list-item>
          <v-list-item v-if="entries.length === 0">
            <v-list-item-title class="text-medium-emphasis">下に階層はありません（このフォルダで作成できます）</v-list-item-title>
          </v-list-item>
        </v-list>

        <div v-else-if="environment === 'webPicker'" class="text-medium-emphasis pa-6 text-center">
          {{ pickedFiles.length > 0 ? `${pickedFiles.length} 枚を選びました。右側で確かめてください。` : '写真を選んでください' }}
        </div>
        <div v-else class="text-medium-emphasis pa-6 text-center">
          フォルダを選んでください
        </div>
      </div>

      <v-divider :vertical="isWide" />

      <div class="pa-4" :style="isWide ? 'width: 280px' : ''">
        <v-text-field v-model="projectName" label="プロジェクト名" clearable :disabled="!selected" />
        <div v-if="selected && environment !== 'webPicker'" class="text-body-2 text-medium-emphasis mb-2">
          出所: {{ root?.label }}{{ subPath ? '/' + subPath : '' }}
        </div>

        <!-- 見本の絵（02章）。枚数・見本12枚。下位フォルダも含めて数える。 -->
        <div v-if="selected" class="mb-3">
          <p v-if="sampleLoading" class="text-caption text-medium-emphasis mb-1">見本を読み込んでいます…</p>
          <p v-else-if="sampleCount !== null" class="text-body-2 mb-1">{{ sampleCount }} 枚</p>
          <div v-if="sampleThumbs.length" class="d-flex ga-1" style="overflow-x: auto">
            <img
              v-for="(url, i) in sampleThumbs"
              :key="i"
              :src="url"
              style="width: 64px; height: 64px; object-fit: cover; border-radius: 4px; flex-shrink: 0"
            >
          </div>
        </div>

        <v-alert v-if="alreadyCreated" type="warning" density="compact" class="mb-2">
          同じ出所のプロジェクトが既にあります。写真は使い回されます。
        </v-alert>
        <p class="text-caption text-medium-emphasis">原本には触れません。準備ができ次第、選別を始められます。</p>
      </div>
    </div>

    <v-divider />
    <div class="d-flex justify-end ga-2 pa-3">
      <v-btn variant="text" @click="router.back()">キャンセル</v-btn>
      <v-btn
        color="primary"
        :disabled="!selected || !projectName.trim()"
        :loading="creating"
        @click="create"
      >
        作成して準備を始める
      </v-btn>
    </div>
  </div>
</template>
