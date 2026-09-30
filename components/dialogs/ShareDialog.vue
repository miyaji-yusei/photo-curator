<script setup lang="ts">
const {
  exportCsvByRating,
  exportPreviewCount,
  exportZipByRating,
  isAmazon,
  shareBusy,
  shareDialog,
  shareError,
  shareMessage,
  shareRatings,
  shareSelectedPhotos
} = useCurator()
</script>

<template>
<!-- 選別結果をライブラリ側へ渡す。ブラウザからは写真ライブラリを
     直接書き換えられないので、どれも利用者の操作を経由する。 -->
<v-dialog v-model="shareDialog" max-width="640" scrollable>
  <v-card title="選別結果を書き出す">
    <v-card-text class="pt-5">
      <v-alert v-if="shareError" type="error" density="compact" class="mb-4">{{ shareError }}</v-alert>
      <v-alert v-if="shareMessage" type="success" density="compact" class="mb-4">{{ shareMessage }}</v-alert>

      <div class="text-caption text-medium-emphasis mb-1">どの星を書き出しますか？</div>
      <v-btn-toggle v-model="shareRatings" multiple density="comfortable" variant="outlined" divided class="mb-5">
        <v-btn v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating" :value="rating">★{{ rating }}</v-btn>
      </v-btn-toggle>

      <v-alert v-if="isAmazon" type="info" variant="tonal" density="comfortable" class="mb-5">
        Amazon の写真です。星はこの端末だけの結果で、Amazon には書き戻しません。
        ZIP は原本を Amazon から取ってきて作ります（このアプリのサーバーを経由できないときは取れないので、CSV だけ書き出せます）。
      </v-alert>
      <v-alert v-else type="info" variant="tonal" density="comfortable" class="mb-5">
        写真アプリに星はなく「お気に入り(♡)」だけです。星そのものはこのアプリが持ち続けます。
        <strong>原本はこの端末で取り込んだ回のあいだだけ手元にあります。</strong>
        読み込み直したあとは、写真を選び直すと書き出せます。
      </v-alert>

      <p class="text-body-2 mb-4"><strong>{{ exportPreviewCount.toLocaleString() }} 枚</strong>が対象です（連写の仲間を含む）。</p>

      <v-list class="bg-transparent">
        <v-list-item v-if="!isAmazon" class="px-0">
          <v-list-item-title>共有シートで渡す</v-list-item-title>
          <v-list-item-subtitle class="text-wrap">
            「画像を保存」で写真アプリへ、「ファイルに保存」でファイルアプリへ。
            写真アプリには<strong>重複として</strong>入り、星は付きません（{{ SHARE_FILE_LIMIT }} 枚まで）。
          </v-list-item-subtitle>
          <template #append>
            <v-btn variant="outlined" :loading="shareBusy" :disabled="!shareRatings.length" @click="shareSelectedPhotos">共有</v-btn>
          </template>
        </v-list-item>
        <v-divider v-if="!isAmazon" />
        <v-list-item class="px-0">
          <v-list-item-title>星ごとに ZIP で書き出す</v-list-item-title>
          <v-list-item-subtitle class="text-wrap">
            <code>star-5/</code> のように星ごとのフォルダに分けます。枚数が多いときや PC に渡すときはこちら。
          </v-list-item-subtitle>
          <template #append>
            <v-btn variant="outlined" :loading="shareBusy" :disabled="!shareRatings.length" @click="exportZipByRating">ZIP</v-btn>
          </template>
        </v-list-item>
        <v-divider />
        <v-list-item class="px-0">
          <v-list-item-title>CSV で書き出す</v-list-item-title>
          <v-list-item-subtitle class="text-wrap">
            写真の名前と星の一覧です（<code>{{ isAmazon ? 'relative_path,rating,captured_at,name' : 'relative_path,rating,captured_at' }}</code>）。原本は取りません。
          </v-list-item-subtitle>
          <template #append>
            <v-btn variant="outlined" :loading="shareBusy" :disabled="!shareRatings.length" @click="exportCsvByRating">CSV</v-btn>
          </template>
        </v-list-item>
      </v-list>
    </v-card-text>
    <v-divider />
    <v-card-actions class="pa-4">
      <v-spacer />
      <v-btn variant="text" @click="shareDialog = false">閉じる</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
