<script setup lang="ts">
const {
  chooseExportDestination,
  exportBusy,
  exportDestination,
  exportDialog,
  exportError,
  exportMode,
  exportRatings,
  exportResult,
  isAmazon,
  ratingCount,
  runExport
} = useCurator()
</script>

<template>
<!-- フォルダ分け。移動は原本フォルダから写真が消えるので警告を強くする。 -->
<v-dialog v-model="exportDialog" max-width="640">
  <v-card title="レーティングごとにフォルダ分け">
    <v-card-text class="pt-5">
      <v-text-field v-model="exportDestination" label="出力先フォルダ" readonly prepend-inner-icon="mdi-folder-outline" class="mb-4">
        <template #append-inner><v-btn variant="outlined" size="small" @click="chooseExportDestination">選択</v-btn></template>
      </v-text-field>

      <div class="text-subtitle-2 mb-2">書き出す星</div>
      <div class="d-flex flex-wrap ga-2 mb-5">
        <v-chip
          v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
          :color="exportRatings.includes(rating) ? 'primary' : undefined"
          :variant="exportRatings.includes(rating) ? 'flat' : 'outlined'"
          @click="exportRatings.includes(rating) ? exportRatings.splice(exportRatings.indexOf(rating), 1) : exportRatings.push(rating)"
        >★{{ rating }}（{{ ratingCount(rating).toLocaleString() }}）</v-chip>
      </div>

      <v-radio-group v-model="exportMode" hide-details class="mb-2">
        <v-radio value="copy" label="コピーする（原本はそのまま残る）" />
        <v-radio value="move" label="移動する（原本フォルダから写真が無くなる）" :disabled="isAmazon" />
      </v-radio-group>
      <p v-if="isAmazon" class="text-caption text-medium-emphasis mb-2">移動は Amazon の写真には使えません。コピーは原本を Amazon から取ってきて、出力先に置きます。</p>
      <v-alert v-if="exportMode === 'move'" type="warning" variant="tonal" density="comfortable" class="mt-3">
        移動すると<strong>元のフォルダから写真が無くなります</strong>。移動後はプロジェクトの索引が古くなるため、
        「写真を再読み込み」が必要になります。
      </v-alert>

      <v-alert v-if="exportError" type="error" variant="tonal" class="mt-4" closable @click:close="exportError = ''">
        {{ exportError }}
      </v-alert>
      <v-alert v-if="exportResult" :type="exportResult.failed ? 'warning' : 'success'" variant="tonal" class="mt-4">
        {{ exportResult.processed.toLocaleString() }} 枚を{{ exportMode === 'move' ? '移動' : 'コピー' }}しました。
        <template v-if="exportResult.skipped">見つからず飛ばした写真 {{ exportResult.skipped }} 枚。</template>
        <template v-if="exportResult.failed">失敗 {{ exportResult.failed }} 枚。</template>
        <ul v-if="exportResult.errors.length" class="mt-2 text-caption">
          <li v-for="line in exportResult.errors" :key="line">{{ line }}</li>
        </ul>
      </v-alert>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" :disabled="exportBusy" @click="exportDialog = false">閉じる</v-btn>
      <v-btn
        :color="exportMode === 'move' ? 'error' : 'primary'"
        :loading="exportBusy" :disabled="!exportDestination || !exportRatings.length"
        @click="runExport"
      >{{ exportMode === 'move' ? '移動する' : 'コピーする' }}</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
