<script setup lang="ts">
// 設定（設計 02 章）。PC・Web は NAS カードを出さない（smb capability が無い）。
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAppStore } from '~/stores/app'
import { useCapabilities } from '~/composables/useCapabilities'
import { useLayout } from '~/composables/useLayout'

definePageMeta({ layout: 'default' })

const app = useAppStore()
const router = useRouter()
const { isWide } = useLayout()
const { capabilities } = useCapabilities()

const displayEdgeChoice = ref<'1024' | '1536' | 'custom'>('1024')
const customEdge = ref(1024)

onMounted(async () => {
  await app.load()
  const edge = app.settings.displayEdge
  displayEdgeChoice.value = edge === 1024 ? '1024' : edge === 1536 ? '1536' : 'custom'
  customEdge.value = edge
})

async function persist() {
  const edge = displayEdgeChoice.value === '1024' ? 1024
    : displayEdgeChoice.value === '1536' ? 1536
    : Math.min(1920, Math.max(768, customEdge.value))
  await app.save({ ...app.settings, displayEdge: edge })
}
</script>

<template>
  <div class="pa-4" style="max-width: 640px">
    <div v-if="!isWide" class="d-flex align-center mb-4">
      <v-btn icon="mdi-arrow-left" variant="text" @click="router.back()" />
      <span class="text-h6 ml-2">設定</span>
    </div>
    <span v-else class="text-h5 d-block mb-4">設定</span>

    <v-card class="mb-4">
      <v-card-item title="表示用画像の既定">
        <v-card-text>
          <v-radio-group v-model="displayEdgeChoice" @update:model-value="persist">
            <v-radio value="1024" label="標準 1024px（2,000 枚で約 156MB）" />
            <v-radio value="1536" label="大きく 1536px（約 351MB）" />
            <v-radio value="custom" label="詳細…（768–1920）" />
          </v-radio-group>
          <v-slider
            v-if="displayEdgeChoice === 'custom'"
            v-model="customEdge"
            :min="768"
            :max="1920"
            :step="16"
            thumb-label
            @update:model-value="persist"
          />
        </v-card-text>
      </v-card-item>
    </v-card>

    <v-card class="mb-4">
      <v-card-item title="選別の既定">
        <v-card-text>
          <v-slider
            v-model="app.settings.groupSize"
            :min="2"
            :max="capabilities.largeGroups ? 10 : 4"
            :step="1"
            thumb-label
            label="一度に見比べる枚数"
            @update:model-value="persist"
          />
          <v-switch
            v-model="app.settings.groupBursts"
            label="連写を自動でまとめる"
            color="primary"
            @update:model-value="persist"
          />
          <v-switch
            v-model="app.settings.confirmBeforeStart"
            label="開始前に毎回この設定を確認する"
            color="primary"
            @update:model-value="persist"
          />
          <v-switch
            v-model="app.settings.holdZooms"
            label="選別中の長押しで拡大する（既定は選ぶ）"
            color="primary"
            @update:model-value="persist"
          />
        </v-card-text>
      </v-card-item>
    </v-card>

    <p class="text-caption text-medium-emphasis text-center mt-6">
      Photo Curator は写真の原本を移動・削除・書き換えしません。星の書き込みと
      フォルダ分けは、実行前に確認します。<br>
      Photo Curator · v0.1.0
    </p>
  </div>
</template>
