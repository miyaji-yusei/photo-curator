<script setup lang="ts">
const {
  openSettings,
  openSlideshowSettings,
  view
} = useCurator()
</script>

<template>
    <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'project'">プロジェクトへ戻る</v-btn><div class="text-overline text-primary">選別を開始</div><h1 class="text-h4 mb-6">方法を選んでください</h1>
    <v-row><v-col cols="12" md="6"><v-card class="choice-card pa-6 h-100" role="button" tabindex="0" @click="openSettings" @keydown.enter="openSettings"><v-icon color="primary" size="40" icon="mdi-tournament" /><div class="text-h5 mt-5">トーナメントで選別</div><p class="text-medium-emphasis mt-2">複数の写真から、次のラウンドに進める1枚を選びます。</p><v-btn color="primary" class="mt-4">設定へ進む</v-btn></v-card></v-col><v-col cols="12" md="6"><v-card class="choice-card pa-6 h-100" role="button" tabindex="0" @click="openSlideshowSettings" @keydown.enter="openSlideshowSettings"><v-icon color="primary" size="40" icon="mdi-play-box-outline" /><div class="text-h5 mt-5">スライドショーで選別</div><p class="text-medium-emphasis mt-2">1枚ずつ直感的に選べる方法です。</p><v-btn color="primary" class="mt-4">設定へ進む</v-btn></v-card></v-col></v-row>
</template>
