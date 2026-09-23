<script setup lang="ts">
// 骨組み（設計 07 章 段2-2）。判断・状態は pages/ と components/ に分ける。
// ここは Nuxt のレイアウトを出すだけ。
</script>

<template>
  <v-app>
    <NuxtLayout>
      <NuxtPage />
    </NuxtLayout>
    <SnackbarHost />
  </v-app>
</template>
