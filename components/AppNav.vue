<script setup lang="ts">
const {
  drawerOpen,
  drawerRail,
  isCompact,
  openProject,
  projectCards,
  projects,
  statusLabel,
  view
} = useCurator()
</script>

<template>
<!-- 狭い画面では常設をやめ、ヘッダーのボタンで開閉する。 -->
<v-app-bar v-if="isCompact" flat color="surface" density="comfortable">
  <v-app-bar-nav-icon aria-label="メニューを開く" @click="drawerOpen = !drawerOpen" />
  <v-app-bar-title class="text-body-1">Photo Curator</v-app-bar-title>
</v-app-bar>

<!-- 選別中は写真に幅を使いたい。rail でアイコンだけの細い状態に畳める。 -->
<v-navigation-drawer
  v-model="drawerOpen"
  :permanent="!isCompact"
  :temporary="isCompact"
  :rail="!isCompact && drawerRail"
  :width="280"
  rail-width="60"
  color="surface"
>
  <v-list-item class="py-4" :title="drawerRail ? undefined : 'Photo Curator'" :subtitle="drawerRail ? undefined : '人の目で、素早く選ぶ'">
    <template #prepend><v-avatar color="primary" size="34"><v-icon color="black" icon="mdi-image-multiple-outline" /></v-avatar></template>
    <template v-if="!drawerRail && !isCompact" #append>
      <v-btn icon="mdi-chevron-left" variant="text" size="small" aria-label="サイドバーをたたむ" @click.stop="drawerRail = true" />
    </template>
  </v-list-item>
  <v-divider />
  <v-list nav class="pt-3">
    <v-list-item prepend-icon="mdi-home-outline" title="ホーム" :active="view === 'home'" @click="view = 'home'" />
    <!-- rail では入れ子のリストが開けないので、畳んだときは1項目にまとめる。 -->
    <v-list-item
      v-if="drawerRail && !isCompact"
      prepend-icon="mdi-folder-multiple-image" title="プロジェクト"
      @click="drawerRail = false"
    />
    <v-list-group v-else value="projects">
      <template #activator="{ props }"><v-list-item v-bind="props" prepend-icon="mdi-folder-multiple-image" title="プロジェクト" /></template>
      <v-list-item v-for="project in projects" :key="project.id" class="project-nav-item" :title="project.name" :subtitle="statusLabel(project)" @click="openProject(project)">
        <template #prepend><v-icon size="18" icon="mdi-folder-outline" /></template>
        <template v-if="projectCards[project.id]" #append><span class="status-dot" :class="`status-dot--${projectCards[project.id]!.status.state}`" role="img" :aria-label="projectCards[project.id]!.status.statusText" /></template>
      </v-list-item>
      <v-list-item v-if="!projects.length" title="まだありません" subtitle="ホームから作成できます" disabled />
    </v-list-group>
  </v-list>
  <template #append>
    <v-list nav class="pb-2">
      <v-list-item
        v-if="!isCompact"
        :prepend-icon="drawerRail ? 'mdi-chevron-right' : 'mdi-chevron-left'"
        :title="drawerRail ? '広げる' : 'サイドバーをたたむ'"
        @click="drawerRail = !drawerRail"
      />
    </v-list>
    <v-card v-if="!drawerRail" flat class="ma-3 pa-3 drop-placeholder" title="作業フォルダへ送る" subtitle="ドラッグ＆ドロップは準備中" prepend-icon="mdi-folder-move-outline" />
  </template>
</v-navigation-drawer>
</template>
