<script setup lang="ts">
// 一時的な失敗の入れ物。app.vue に1つだけ置く。
import { useSnackbar } from '~/composables/useSnackbar'

const { state, close } = useSnackbar()
</script>

<template>
  <v-snackbar v-model="state.open" :timeout="4000" location="bottom">
    {{ state.text }}
    <template #actions>
      <v-btn v-if="state.retry" variant="text" color="primary" @click="state.retry?.(); close()">再試行</v-btn>
      <v-btn variant="text" @click="close">閉じる</v-btn>
    </template>
  </v-snackbar>
</template>
