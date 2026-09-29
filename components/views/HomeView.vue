<script setup lang="ts">
const {
  askDeleteProject,
  createDialog,
  formatDate,
  openProject,
  openProjectAction,
  projectCards,
  projects,
  shortPath,
  statusLabel
} = useCurator()
const cloudOrFolder = (project: { sourceKind: string }) => project.sourceKind === 'amazon' ? 'mdi-cloud-outline' : 'mdi-folder-image'
</script>

<template>
    <div class="d-flex align-start justify-space-between flex-wrap ga-4 mb-8">
      <div><div class="text-overline text-primary">Photo selection workspace</div><h1 class="text-h3 font-weight-bold">写真を、選びやすい形へ。</h1><p class="text-medium-emphasis mt-2">プロジェクトを作成して、直感的な選択を始めましょう。</p></div>
      <v-btn color="primary" size="large" prepend-icon="mdi-plus" @click="createDialog = true">プロジェクトを作成</v-btn>
    </div>
    <v-row>
      <v-col cols="12"><v-card><v-card-title class="pt-5 px-5">プロジェクト</v-card-title><v-card-subtitle class="px-5">選別の状況をここから確認できます。</v-card-subtitle><v-list v-if="projects.length" lines="three" class="mt-3"><v-list-item v-for="project in projects" :key="project.id" :title="project.name" @click="openProject(project)"><template #prepend><v-avatar color="surface-variant" rounded="lg" size="56" class="mr-2"><v-img v-if="projectCards[project.id]?.thumbnailUrl" :src="projectCards[project.id]!.thumbnailUrl!" cover :alt="`${project.name} の見本`" /><v-icon v-else :icon="cloudOrFolder(project)" /></v-avatar></template><template #subtitle><div class="text-truncate">{{ project.sourceKind === 'amazon' ? 'Amazon Photos の共有リンク' : shortPath(project.folderPath) }}</div><div v-if="projectCards[project.id]" class="d-flex align-center ga-2 project-status"><span class="status-dot" :class="`status-dot--${projectCards[project.id]!.status.state}`" /><span class="text-body-2">{{ projectCards[project.id]!.status.statusText }}</span></div></template><template #append><div class="d-flex align-center ga-4"><div class="text-right d-none d-sm-block"><div class="text-body-2">{{ statusLabel(project) }}</div><div class="text-caption text-medium-emphasis">更新 {{ formatDate(project.updatedAt) }}</div></div><v-btn v-if="projectCards[project.id]" size="small" :variant="projectCards[project.id]!.status.state === 'culling' || projectCards[project.id]!.status.state === 'new' ? 'flat' : 'outlined'" :color="projectCards[project.id]!.status.state === 'error' ? 'error' : 'primary'" :disabled="!projectCards[project.id]!.status.actionEnabled" @click.stop="openProjectAction(project, projectCards[project.id]!.status.state)">{{ projectCards[project.id]!.status.actionLabel }}</v-btn><v-btn icon="mdi-delete-outline" variant="text" size="small" :aria-label="`${project.name} を削除`" @click.stop="askDeleteProject(project)" /></div></template></v-list-item></v-list><v-card-text v-else class="py-12 text-center text-medium-emphasis">まだプロジェクトがありません。</v-card-text></v-card></v-col>
    </v-row>
</template>
