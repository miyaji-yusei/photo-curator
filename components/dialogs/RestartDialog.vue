<script setup lang="ts">
const {
  restartBusy,
  restartDialog,
  restartFromScratch
} = useCurator()
</script>

<template>
<v-dialog v-model="restartDialog" max-width="520">
  <v-card title="最初からやり直しますか？">
    <v-card-text class="pt-5">
      <v-alert type="warning" variant="tonal" density="comfortable">
        すべての写真の星が <strong>0 に戻ります</strong>。選別のやり直しは元に戻せません。
      </v-alert>
      <p class="text-caption text-medium-emphasis mt-4 mb-0">
        解析結果（連写の判定やサムネイル）は消えないので、次の選別もすぐ始められます。
        すでに書き出したフォルダやメタデータには影響しません。
      </p>
    </v-card-text>
    <v-card-actions class="pa-5 pt-2">
      <v-spacer />
      <v-btn variant="outlined" :disabled="restartBusy" @click="restartDialog = false">キャンセル</v-btn>
      <v-btn color="error" :loading="restartBusy" @click="restartFromScratch">星を全部消してやり直す</v-btn>
    </v-card-actions>
  </v-card>
</v-dialog>
</template>
