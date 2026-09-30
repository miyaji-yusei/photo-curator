<script setup lang="ts">
import { displayEdgeLabel } from '~/utils/displayEdge'

const { appDisplayChoices, appDisplayEdge, saveAppDisplayEdge, view } = useCurator()
</script>

<template>
    <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'home'">ホームへ戻る</v-btn><div class="text-overline text-primary">App settings</div><h1 class="text-h4 mb-6">設定</h1>
    <v-card max-width="720" class="pa-6 settings-card">
      <div class="text-subtitle-1 font-weight-medium mb-2">表示用画像の既定の大きさ</div>
      <p class="text-caption text-medium-emphasis mb-4">
        これから作るプロジェクトの既定です。すでにあるプロジェクトは変わりません（プロジェクトの画面で変えられます）。長辺の画素数で、大きいほど細部まで見えますが、容量と準備の時間が増えます。
      </p>
      <v-radio-group v-if="appDisplayChoices.length" :model-value="appDisplayEdge" hide-details aria-label="表示用画像の既定の大きさ" @update:model-value="saveAppDisplayEdge($event as number)">
        <v-radio v-for="choice in appDisplayChoices" :key="choice" :value="choice" :label="displayEdgeLabel(choice)" color="primary" />
      </v-radio-group>
      <p v-else class="text-body-2 text-medium-emphasis">設定を読み込めませんでした。</p>
    </v-card>
</template>
