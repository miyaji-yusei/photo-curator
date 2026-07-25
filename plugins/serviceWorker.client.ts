/**
 * サービスワーカーの登録。**製品ビルドだけ**で行う。
 * 開発中に登録すると、資産が古いまま返って変更が反映されなくなる。
 *
 * Tauri（デスクトップ）では不要なので登録しない。ローカルの資産を
 * そのまま読んでいて、オフラインの概念が無い。
 */
export default defineNuxtPlugin(() => {
  if (import.meta.dev) return
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) return
  // Tauri は tauri://localhost で動く。ここでは登録しない。
  if ('__TAURI_INTERNALS__' in window) return

  const base = useRuntimeConfig().app.baseURL
  window.addEventListener('load', () => {
    navigator.serviceWorker.register(`${base}sw.js`, { scope: base }).catch(() => {
      // 登録できなくてもアプリは動く。オフラインにならないだけ。
    })
  })
})
