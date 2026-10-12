<script setup lang="ts">
const { analysisFailureList, analysisFailuresDialog } = useCurator()

const unsupported = computed(() => analysisFailureList.value.filter(item => item.kind === 'unsupported'))
const transient = computed(() => analysisFailureList.value.filter(item => item.kind !== 'unsupported'))
</script>

<template>
<!-- 解析できなかった写真の一覧（U58）。名前・理由・種類（対応していない形式／一時的に読めなかった）。 -->
<v-dialog v-model="analysisFailuresDialog" max-width="720" scrollable>
  <v-card title="解析できなかった写真">
    <v-card-text class="pt-3">
      <p v-if="!analysisFailureList.length" class="text-body-2">解析できなかった写真はありません。</p>
      <template v-if="unsupported.length">
        <div class="text-subtitle-2 mb-1">対応していない形式（{{ unsupported.length.toLocaleString() }} 件）</div>
        <p class="text-caption text-medium-emphasis mb-2">
          写真ファイルが変わるまで、もう一度は試しません。選別の対象と枚数には入りません（ファイルは消しません）。
        </p>
        <v-list density="compact" class="mb-4">
          <v-list-item v-for="item in unsupported" :key="item.relativePath" :title="item.relativePath" :subtitle="item.reason" />
        </v-list>
      </template>
      <template v-if="transient.length">
        <div class="text-subtitle-2 mb-1">一時的に読めなかった（{{ transient.length.toLocaleString() }} 件）</div>
        <p class="text-caption text-medium-emphasis mb-2">次に開いたときにもう一度試します。選別の対象には入ります。</p>
        <v-list density="compact">
          <v-list-item v-for="item in transient" :key="item.relativePath" :title="item.relativePath" :subtitle="item.reason" />
        </v-list>
      </template>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" @click="analysisFailuresDialog = false">閉じる</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
