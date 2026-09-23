<script setup lang="ts">
// プロジェクト詳細（設計 02 章）。2 カラム（左340px／右1fr。<600 は縦積み）。
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useLayout } from '~/composables/useLayout'
import { useAppStore } from '~/stores/app'
import type { Project, ProjectPhoto, PrepareProgress } from '~/types/project'
import type { Session } from '~/lib/core'
import { projectStatus } from '~/utils/projectStatus'

definePageMeta({ layout: 'default' })

const backend = useBackend()
const router = useRouter()
const route = useRoute()
const { isNarrow } = useLayout()
const app = useAppStore()

const projectId = computed(() => String(route.params.id))
const project = ref<Project | null>(null)
const photos = ref<ProjectPhoto[]>([])
const session = ref<Session | null>(null)
const thumbUrls = ref<Record<string, string>>({})
const preparing = ref(false)
const prepareProgress = ref<PrepareProgress | null>(null)
const menu = ref(false)
const renameOpen = ref(false)
const renameValue = ref('')
const deleteOpen = ref(false)
const deleteBytes = ref(0)
const techOpen = ref(false)
const restartOpen = ref(false)
let abortController: AbortController | null = null

async function load() {
  await app.load()
  project.value = await backend.getProject(projectId.value)
  if (!project.value) return
  photos.value = await backend.listPhotos(projectId.value)
  session.value = await backend.loadSession(projectId.value)
  // 画面に出す分だけ URL を取る（先頭 96 枚。格子は簡易実装）。
  const shown = photos.value.slice(0, 96)
  const entries = await Promise.all(
    shown.map(async p => [p.relativePath, await backend.thumbnailUrl(projectId.value, p.relativePath)] as const)
  )
  thumbUrls.value = Object.fromEntries(entries.filter(([, url]) => url) as [string, string][])
}

async function runPrepare() {
  if (!project.value || preparing.value) return
  preparing.value = true
  abortController = new AbortController()
  try {
    await backend.prepare(projectId.value, (p) => {
      prepareProgress.value = p
      void backend.getProject(projectId.value).then((next) => { if (next) project.value = next })
    }, abortController.signal)
  } finally {
    preparing.value = false
    prepareProgress.value = null
    await load()
  }
}

function cancelPrepare() {
  abortController?.abort()
}

onMounted(load)
onUnmounted(() => abortController?.abort())

const status = computed(() => project.value ? projectStatus(project.value, session.value) : null)

const needsPrepare = computed(() =>
  !!project.value && (project.value.scannedCount < project.value.photoCount || project.value.displayedCount < project.value.scannedCount || project.value.photoCount === 0)
)

function primaryAction() {
  if (!project.value) return
  if (needsPrepare.value) { runPrepare(); return }
  if (project.value.completedAt) { router.push(`/project/${project.value.id}/results`); return }
  router.push(`/project/${project.value.id}/cull`)
}

const primaryLabel = computed(() => {
  if (!project.value) return ''
  if (needsPrepare.value) return preparing.value ? '準備中…' : '準備を始める'
  if (project.value.completedAt) return '結果を見る'
  if (session.value) return '選別を続ける'
  return '選別を開始'
})

async function openRename() {
  renameValue.value = project.value?.name ?? ''
  renameOpen.value = true
  menu.value = false
}
async function confirmRename() {
  if (!project.value) return
  await backend.renameProject(project.value.id, renameValue.value)
  renameOpen.value = false
  await load()
}

async function openDelete() {
  if (!project.value) return
  deleteBytes.value = await backend.estimateDeleteSize(project.value.id)
  deleteOpen.value = true
  menu.value = false
}
async function confirmDelete() {
  if (!project.value) return
  await backend.deleteProject(project.value.id)
  router.push('/')
}

async function confirmRestart() {
  if (!project.value) return
  await backend.restartProject(project.value.id)
  restartOpen.value = false
  await load()
}

function fmtBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

watch(projectId, load)
</script>

<template>
  <div v-if="project" class="pa-4">
    <div class="d-flex align-center mb-4">
      <v-btn icon="mdi-arrow-left" variant="text" to="/" />
      <span class="text-h6 ml-2 flex-grow-1">{{ project.name }}</span>
      <v-menu v-model="menu">
        <template #activator="{ props }">
          <v-btn icon="mdi-dots-vertical" variant="text" v-bind="props" />
        </template>
        <v-list>
          <v-list-item title="再読み込み" @click="runPrepare(); menu = false" />
          <v-list-item title="名前を変更" @click="openRename" />
          <v-list-item title="技術情報" @click="techOpen = true; menu = false" />
          <v-list-item title="やり直す" @click="restartOpen = true; menu = false" />
          <v-list-item title="削除" @click="openDelete" />
        </v-list>
      </v-menu>
    </div>

    <div class="d-flex" :class="isNarrow ? 'flex-column' : 'flex-row'" style="gap: 16px">
      <div :style="isNarrow ? '' : 'width: 340px; flex-shrink: 0'">
        <v-card class="mb-4">
          <v-card-item>
            <template #prepend><v-icon icon="mdi-folder" /></template>
            <template #title>{{ project.source.label }}</template>
            <template #subtitle>{{ project.photoCount }} 枚</template>
          </v-card-item>
          <v-divider />
          <v-card-text class="text-caption text-medium-emphasis">原本には触れません</v-card-text>
        </v-card>

        <v-card class="mb-4">
          <v-card-item title="準備">
            <template #subtitle>
              {{ preparing ? '進行中' : needsPrepare ? '止まっています' : '完了' }}
            </template>
          </v-card-item>
          <v-card-text>
            <div class="d-flex justify-space-between text-body-2 mb-1">
              <span>写真の走査</span>
              <span>{{ project.scannedCount }} / {{ project.photoCount || '?' }}</span>
            </div>
            <div class="d-flex justify-space-between text-body-2 mb-1">
              <span>撮影時刻・サムネイル</span>
              <span>{{ project.metaHashedCount }} / {{ project.photoCount }}</span>
            </div>
            <div class="d-flex justify-space-between text-body-2 mb-1">
              <span>表示用画像（{{ app.settings.displayEdge }}px）</span>
              <span>{{ project.displayedCount }} / {{ project.photoCount }}</span>
            </div>
            <v-progress-linear
              :model-value="project.photoCount ? (project.displayedCount / project.photoCount) * 100 : 0"
              height="3"
              color="secondary"
              class="mb-2"
            />
            <p class="text-caption text-medium-emphasis">できた写真から選別に出ます。ネットワーク越しの場合は Wi-Fi を推奨します。</p>
            <v-alert v-if="project.prepareWarning" type="warning" density="compact" class="mt-2">
              {{ project.prepareWarning }}
              <template #append>
                <v-btn size="small" variant="text" @click="runPrepare">再試行</v-btn>
              </template>
            </v-alert>
            <v-btn v-if="preparing" class="mt-2" block variant="tonal" @click="cancelPrepare">中断する</v-btn>
          </v-card-text>
        </v-card>

        <v-card v-if="session">
          <v-card-item :title="`★${session.target_star} を選別中 · ROUND ${session.round}`">
            <template #subtitle>残り {{ session.queue.length + session.current.length }} 枚</template>
          </v-card-item>
        </v-card>
      </div>

      <div class="flex-grow-1">
        <v-btn
          color="primary"
          size="large"
          block
          :loading="preparing"
          :disabled="needsPrepare && preparing"
          class="mb-4"
          @click="primaryAction"
        >
          {{ primaryLabel }}
        </v-btn>
        <div v-if="preparing && prepareProgress" class="mb-4">
          <div class="text-body-2 mb-1">
            {{ prepareProgress.task }}: {{ prepareProgress.done }} / {{ prepareProgress.total }}
          </div>
          <v-progress-linear
            :model-value="prepareProgress.total ? (prepareProgress.done / prepareProgress.total) * 100 : 0"
            color="secondary"
          />
        </div>

        <div class="d-flex flex-wrap" style="gap: 4px">
          <div
            v-for="photo in photos.slice(0, 96)"
            :key="photo.relativePath"
            style="width: 96px; height: 96px; background: #16181d; overflow: hidden; border-radius: 4px"
          >
            <img
              v-if="thumbUrls[photo.relativePath]"
              :src="thumbUrls[photo.relativePath]"
              :alt="photo.relativePath"
              style="width: 100%; height: 100%; object-fit: cover"
            >
          </div>
        </div>
        <p v-if="photos.length > 96" class="text-caption text-medium-emphasis mt-2">
          ほか {{ photos.length - 96 }} 枚
        </p>
        <p v-if="photos.length === 0 && !needsPrepare" class="text-medium-emphasis pa-4 text-center">
          写真がありません
        </p>
      </div>
    </div>

    <v-dialog :model-value="renameOpen" max-width="420" @update:model-value="(v) => (renameOpen = v)">
      <v-card title="名前を変更">
        <v-card-text><v-text-field v-model="renameValue" autofocus clearable /></v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="renameOpen = false">キャンセル</v-btn>
          <v-btn color="primary" @click="confirmRename">変更する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="deleteOpen" max-width="420" @update:model-value="(v) => (deleteOpen = v)">
      <v-card title="このプロジェクトを削除しますか">
        <v-card-text>
          <p class="text-error">「{{ project.name }}」を削除します。この操作は取り消せません。</p>
          <p class="text-body-2 text-medium-emphasis mt-2">約 {{ fmtBytes(deleteBytes) }} を消します（原本は消えません）。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="deleteOpen = false">キャンセル</v-btn>
          <v-btn color="error" @click="confirmDelete">削除する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="restartOpen" max-width="420" @update:model-value="(v) => (restartOpen = v)">
      <v-card title="選別を最初からやり直しますか">
        <v-card-text>
          <p class="text-error">星・履歴・手直し・学習した基準を消します。顔ぶれ・指紋・絵は残ります。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="restartOpen = false">キャンセル</v-btn>
          <v-btn color="error" @click="confirmRestart">やり直す</v-btn>
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
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="techOpen = false">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
  <div v-else class="text-center pa-8">
    <v-progress-circular indeterminate color="primary" />
  </div>
</template>
