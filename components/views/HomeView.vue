<script setup lang="ts">
const {
  askDeleteProject,
  createDialog,
  formatDate,
  openProject,
  projects,
  shortPath,
  statusLabel
} = useCurator()
</script>

<template>
    <div class="d-flex align-start justify-space-between flex-wrap ga-4 mb-8">
      <div><div class="text-overline text-primary">Photo selection workspace</div><h1 class="text-h3 font-weight-bold">写真を、選びやすい形へ。</h1><p class="text-medium-emphasis mt-2">プロジェクトを作成して、直感的な選択を始めましょう。</p></div>
      <v-btn color="primary" size="large" prepend-icon="mdi-plus" @click="createDialog = true">プロジェクトを作成</v-btn>
    </div>
    <v-row>
      <v-col cols="12"><v-card><v-card-title class="pt-5 px-5">プロジェクト</v-card-title><v-card-subtitle class="px-5">選別の状況をここから確認できます。</v-card-subtitle><v-list v-if="projects.length" lines="two" class="mt-3"><v-list-item v-for="project in projects" :key="project.id" :title="project.name" :subtitle="shortPath(project.folderPath)" @click="openProject(project)"><template #prepend><v-avatar color="surface-variant"><v-icon icon="mdi-folder-image" /></v-avatar></template><template #append><div class="d-flex align-center ga-4"><div class="text-right"><div class="text-body-2">{{ statusLabel(project) }}</div><div class="text-caption text-medium-emphasis">更新 {{ formatDate(project.updatedAt) }}</div></div><v-btn icon="mdi-delete-outline" variant="text" size="small" :aria-label="`${project.name} を削除`" @click.stop="askDeleteProject(project)" /></div></template></v-list-item></v-list><v-card-text v-else class="py-12 text-center text-medium-emphasis">まだプロジェクトがありません。</v-card-text></v-card></v-col>
    </v-row>
</template>
