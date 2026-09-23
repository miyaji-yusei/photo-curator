<script setup lang="ts">
// wide（≥1100）はアプリバー＋ドロワー。選別まわりの画面は layouts/focus.vue を使う
// （設計 02 章 PC・Web の差分）。
import { ref } from 'vue'
import { useLayout } from '~/composables/useLayout'

const { isWide } = useLayout()
const drawer = ref(false)
</script>

<template>
  <div>
    <v-app-bar v-if="isWide" flat density="comfortable">
      <v-app-bar-nav-icon @click="drawer = !drawer" />
      <v-app-bar-title>Photo Curator</v-app-bar-title>
    </v-app-bar>
    <v-navigation-drawer v-if="isWide" v-model="drawer">
      <v-list nav>
        <v-list-item to="/" prepend-icon="mdi-home" title="ホーム" />
        <v-list-item to="/settings" prepend-icon="mdi-cog" title="設定" />
      </v-list>
    </v-navigation-drawer>
    <v-main>
      <slot />
    </v-main>
  </div>
</template>
