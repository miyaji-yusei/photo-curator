<script setup lang="ts">
const {
  chooseExportDestination,
  exportBusy,
  exportDestination,
  exportDialog,
  exportError,
  exportMode,
  exportMoveConfirm,
  exportPreviewCount,
  exportRatings,
  exportResult,
  isAmazon,
  ratingCount,
  requestExport,
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

      <!-- 連写の仲間まで広げたあとの枚数（書き出す枚数）。 -->
      <p class="text-body-2 mb-4"><strong>{{ exportPreviewCount.toLocaleString() }} 枚</strong>を書き出します（連写の仲間を含む）。</p>

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
        :loading="exportBusy" :disabled="!exportDestination || !exportRatings.length || !exportPreviewCount"
        @click="requestExport"
      >{{ exportMode === 'move' ? '移動する' : 'コピーする' }}</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>

<!-- 移動の確認。移動は原本フォルダから写真が消えるので、実行の前に必ず確認する。 -->
<v-dialog v-model="exportMoveConfirm" max-width="480">
  <v-card title="原本を移動しますか？">
    <v-card-text class="pt-4">
      <p class="text-error font-weight-bold mb-3">原本を移動します</p>
      <p class="text-body-2">
        <strong>{{ exportPreviewCount.toLocaleString() }} 枚</strong>が、元のフォルダから
        <code>{{ exportDestination }}</code> の <code>star-N</code> フォルダへ移ります。
        元のフォルダには残りません。
      </p>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" @click="exportMoveConfirm = false">やめる</v-btn>
      <v-btn color="error" @click="runExport">原本を移動する</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
