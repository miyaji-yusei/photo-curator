// 設定・環境・capabilities をまとめて持つ、小さな Pinia store。
// プロジェクト一覧・選別の状態は各ページがその場で Backend から読む
// （大きなセッションをストアに複製すると二重管理になるため）。
import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { AppSettings } from '~/types/project'
import { DEFAULT_SETTINGS } from '~/types/project'
import { useBackend } from '~/composables/useBackend'

export const useAppStore = defineStore('app', () => {
  const settings = ref<AppSettings>({ ...DEFAULT_SETTINGS })
  const loaded = ref(false)

  async function load() {
    if (loaded.value) return
    settings.value = await useBackend().loadSettings()
    loaded.value = true
  }

  async function save(next: AppSettings) {
    settings.value = next
    await useBackend().saveSettings(next)
  }

  return { settings, loaded, load, save }
})
