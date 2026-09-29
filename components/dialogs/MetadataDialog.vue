<script setup lang="ts">
const {
  metadataAcknowledged,
  metadataBusy,
  metadataDialog,
  metadataError,
  metadataRatings,
  metadataResult,
  ratingCount,
  runMetadataWrite
} = useCurator()
</script>

<template>
<!-- メタデータ書き込み。原本を書き換えるので、明示的な同意を必須にする。 -->
<v-dialog v-model="metadataDialog" max-width="640">
  <v-card title="レーティングをメタデータに反映">
    <v-card-text class="pt-5">
      <v-alert type="warning" variant="tonal" density="comfortable" class="mb-4">
        <strong>写真の原本を書き換えます。</strong>
        星は XMP（<code>xmp:Rating</code>）として写真の中に書き込まれ、Lightroom や Bridge などが読み取れます。
        撮影情報（EXIF）と画像そのものには手を加えません。
      </v-alert>
      <p class="text-caption text-medium-emphasis mb-4">
        書き込みは、いったん別ファイルを作って画像として開けるか確かめてから置き換えます。
        途中で失敗しても原本はそのまま残ります。対象は JPEG のみで、PNG と WebP は飛ばします。
      </p>

      <div class="text-subtitle-2 mb-2">書き込む星</div>
      <div class="d-flex flex-wrap ga-2 mb-5">
        <v-chip
          v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
          :color="metadataRatings.includes(rating) ? 'primary' : undefined"
          :variant="metadataRatings.includes(rating) ? 'flat' : 'outlined'"
          @click="metadataRatings.includes(rating) ? metadataRatings.splice(metadataRatings.indexOf(rating), 1) : metadataRatings.push(rating)"
        >★{{ rating }}（{{ ratingCount(rating).toLocaleString() }}）</v-chip>
      </div>

      <v-checkbox
        v-model="metadataAcknowledged" hide-details density="comfortable"
        label="原本が書き換わることを理解しました"
      />

      <v-alert v-if="metadataError" type="error" variant="tonal" class="mt-4" closable @click:close="metadataError = ''">
        {{ metadataError }}
      </v-alert>
      <v-alert v-if="metadataResult" :type="metadataResult.failed ? 'warning' : 'success'" variant="tonal" class="mt-4">
        {{ metadataResult.processed.toLocaleString() }} 枚に書き込みました。
        <template v-if="metadataResult.skipped">対象外で飛ばした写真 {{ metadataResult.skipped }} 枚。</template>
        <template v-if="metadataResult.failed">失敗 {{ metadataResult.failed }} 枚（原本は変更していません）。</template>
        <ul v-if="metadataResult.errors.length" class="mt-2 text-caption">
          <li v-for="line in metadataResult.errors" :key="line">{{ line }}</li>
        </ul>
      </v-alert>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" :disabled="metadataBusy" @click="metadataDialog = false">閉じる</v-btn>
      <v-btn color="error" :loading="metadataBusy" :disabled="!metadataAcknowledged || !metadataRatings.length" @click="runMetadataWrite">
        書き込む
      </v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
