<script setup lang="ts">
// ホーム（設計 02 章）。出所へ問い合わせない。端末に置いたものだけを見る。
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useBackend } from '~/composables/useBackend'
import { useLayout } from '~/composables/useLayout'
import type { Project } from '~/types/project'
import type { Session } from '~/lib/core'
import { projectStatus, STATE_COLOR } from '~/utils/projectStatus'

definePageMeta({ layout: 'default' })

const backend = useBackend()
const router = useRouter()
const { isWide, isNarrow } = useLayout()

const projects = ref<Project[]>([])
const sessions = ref<Record<string, Session | null>>({})
const loading = ref(true)
const renameTarget = ref<Project | null>(null)
const renameValue = ref('')
const deleteTarget = ref<Project | null>(null)
const deleteBytes = ref(0)
const techTarget = ref<Project | null>(null)

async function load() {
  loading.value = true
  projects.value = await backend.listProjects()
  const entries = await Promise.all(
    projects.value.map(async p => [p.id, await backend.loadSession(p.id)] as const)
  )
  sessions.value = Object.fromEntries(entries)
  loading.value = false
}

onMounted(load)

function open(project: Project) {
  const status = projectStatus(project, sessions.value[project.id] ?? null)
  if (status.state === 'done') router.push(`/project/${project.id}/results`)
  else router.push(`/project/${project.id}`)
}

function openRename(project: Project) {
  renameTarget.value = project
  renameValue.value = project.name
}
async function confirmRename() {
  if (!renameTarget.value) return
  await backend.renameProject(renameTarget.value.id, renameValue.value)
  renameTarget.value = null
  await load()
}

async function openDelete(project: Project) {
  deleteTarget.value = project
  deleteBytes.value = await backend.estimateDeleteSize(project.id)
}
async function confirmDelete() {
  if (!deleteTarget.value) return
  await backend.deleteProject(deleteTarget.value.id)
  deleteTarget.value = null
  await load()
}

function fmtBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

const columns = computed(() => (isNarrow.value ? 1 : isWide.value ? 3 : 2))
</script>

<template>
  <div class="pa-4">
    <div v-if="!isWide" class="d-flex align-center justify-space-between mb-4">
      <span class="text-h6">Photo Curator</span>
      <div>
        <v-btn icon="mdi-cog" variant="text" to="/settings" />
      </div>
    </div>

    <div class="d-flex align-center justify-space-between flex-wrap mb-2">
      <span class="text-body-2 text-medium-emphasis">
        プロジェクト {{ projects.length }} 件 · 更新順
      </span>
      <span class="text-caption text-medium-emphasis">原本には触れません</span>
    </div>

    <v-btn color="primary" prepend-icon="mdi-plus" class="mb-4" to="/create">
      プロジェクトを作成
    </v-btn>

    <div v-if="loading" class="text-center pa-8">
      <v-progress-circular indeterminate color="primary" />
    </div>

    <div v-else-if="projects.length === 0" class="text-center pa-8">
      <p class="text-h6 mb-2">まだプロジェクトがありません</p>
      <p class="text-body-2 text-medium-emphasis mb-4">フォルダを選んで、選別を始めましょう。</p>
      <v-btn color="primary" prepend-icon="mdi-plus" to="/create">プロジェクトを作成</v-btn>
    </div>

    <v-row v-else>
      <v-col v-for="project in projects" :key="project.id" :cols="12 / columns">
        <v-card :data-testid="`project-card-${project.id}`" @click="open(project)">
          <v-card-item>
            <template #title>{{ project.name }}</template>
            <template #append>
              <v-menu>
                <template #activator="{ props }">
                  <v-btn icon="mdi-dots-vertical" variant="text" size="small" v-bind="props" @click.stop />
                </template>
                <v-list>
                  <v-list-item title="名前を変更" @click="openRename(project)" />
                  <v-list-item title="技術情報" @click="techTarget = project" />
                  <v-list-item title="削除" @click="openDelete(project)" />
                </v-list>
              </v-menu>
            </template>
          </v-card-item>
          <v-card-text>
            <div class="d-flex align-center mb-1">
              <v-icon
                :color="STATE_COLOR[projectStatus(project, sessions[project.id] ?? null).state]"
                icon="mdi-circle"
                size="10"
                class="mr-2"
              />
              <span>{{ projectStatus(project, sessions[project.id] ?? null).statusText }}</span>
            </div>
            <div class="text-body-2 text-medium-emphasis">
              {{ projectStatus(project, sessions[project.id] ?? null).countText }}
            </div>
          </v-card-text>
          <v-card-actions>
            <v-spacer />
            <v-btn
              variant="tonal"
              :disabled="!projectStatus(project, sessions[project.id] ?? null).actionEnabled"
              @click.stop="open(project)"
            >
              {{ projectStatus(project, sessions[project.id] ?? null).actionLabel }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-col>
    </v-row>

    <v-dialog :model-value="!!renameTarget" max-width="420" @update:model-value="(v) => { if (!v) renameTarget = null }">
      <v-card v-if="renameTarget" title="名前を変更">
        <v-card-text>
          <v-text-field v-model="renameValue" label="プロジェクト名" autofocus clearable />
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="renameTarget = null">キャンセル</v-btn>
          <v-btn color="primary" @click="confirmRename">変更する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="!!deleteTarget" max-width="420" @update:model-value="(v) => { if (!v) deleteTarget = null }">
      <v-card v-if="deleteTarget" title="このプロジェクトを削除しますか">
        <v-card-text>
          <p class="text-error">「{{ deleteTarget.name }}」を削除します。この操作は取り消せません。</p>
          <p class="text-body-2 text-medium-emphasis mt-2">約 {{ fmtBytes(deleteBytes) }} を消します（原本は消えません）。</p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="deleteTarget = null">キャンセル</v-btn>
          <v-btn color="error" @click="confirmDelete">削除する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog :model-value="!!techTarget" max-width="420" @update:model-value="(v) => { if (!v) techTarget = null }">
      <v-card v-if="techTarget" title="技術情報">
        <v-card-text>
          <div class="text-body-2">id: {{ techTarget.id }}</div>
          <div class="text-body-2">出所: {{ techTarget.source.kind }} / {{ techTarget.source.label }}</div>
          <div class="text-body-2">作成: {{ new Date(techTarget.createdAt).toLocaleString('ja-JP') }}</div>
          <div class="text-body-2">更新: {{ new Date(techTarget.updatedAt).toLocaleString('ja-JP') }}</div>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn @click="techTarget = null">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>
