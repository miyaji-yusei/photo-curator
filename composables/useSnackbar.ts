// 一時的な失敗はスナックバー4秒＋「再試行」。画面上部の赤い帯は使わない
// （設計 02 章「文言の規則」）。
import { ref } from 'vue'

interface SnackbarState {
  open: boolean
  text: string
  retry: (() => void) | null
}

const state = ref<SnackbarState>({ open: false, text: '', retry: null })
let timer: ReturnType<typeof setTimeout> | null = null

export function useSnackbar() {
  function show(text: string, retry?: () => void) {
    if (timer) clearTimeout(timer)
    state.value = { open: true, text, retry: retry ?? null }
    timer = setTimeout(() => { state.value.open = false }, 4000)
  }
  function close() {
    state.value.open = false
    if (timer) clearTimeout(timer)
  }
  return { state, show, close }
}
