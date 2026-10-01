<script setup lang="ts">
// 選別中の上のバーの「？」ボタンと、操作の一覧のモーダル（U24）。
// トーナメントとスライドショーの両方が同じ位置・同じ部品で置く。文章は utils/selectionHelp.ts の 1 か所。
import { helpGuides } from '~/utils/selectionHelp'

const { helpDialog, isSlideshow } = useCurator()
const guides = computed(() => helpGuides(isSlideshow.value ? 'slideshow' : 'tournament'))
</script>

<template>
  <v-btn
    icon="mdi-help-circle-outline" variant="outlined" size="small" class="selection-help-btn"
    aria-label="操作のヘルプ" title="操作のヘルプ" @click="helpDialog = true"
  />
  <v-dialog v-model="helpDialog" max-width="560" scrollable>
    <v-card :title="guides.current.title">
      <v-card-text class="pt-2">
        <div v-for="section in guides.current.sections" :key="section.title" class="mb-4">
          <div class="text-subtitle-2 mb-1">{{ section.title }}</div>
          <ul class="selection-help-list text-body-2">
            <li v-for="item in section.items" :key="item">{{ item }}</li>
          </ul>
        </div>
        <v-expansion-panels variant="accordion" class="mt-2">
          <v-expansion-panel :title="`${guides.other.title}（もう一方の方式）`">
            <v-expansion-panel-text>
              <div v-for="section in guides.other.sections" :key="section.title" class="mb-3">
                <div class="text-subtitle-2 mb-1">{{ section.title }}</div>
                <ul class="selection-help-list text-body-2">
                  <li v-for="item in section.items" :key="item">{{ item }}</li>
                </ul>
              </div>
            </v-expansion-panel-text>
          </v-expansion-panel>
        </v-expansion-panels>
      </v-card-text>
      <v-card-actions class="pa-5 pt-2">
        <v-spacer />
        <v-btn variant="outlined" @click="helpDialog = false">閉じる</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.selection-help-list { padding-left: 1.2em; }
.selection-help-list li { margin-bottom: 2px; }
</style>
