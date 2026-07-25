import { createLocalBackend } from '~/composables/backends/local'
import { createTauriBackend } from '~/composables/backends/tauri'
import type { PhotoBackend } from '~/composables/photoBackend'
import { isTauriRuntime } from '~/composables/photoBackend'

/**
 * 実行環境に合う `PhotoBackend` を返す。
 *
 * 画面側はこれ 1 つだけを見る。デスクトップでは PC の SQLite と原本フォルダ、
 * ブラウザ（iPad など）では端末内の DB とサムネイルへ繋がるが、
 * **呼び出し側から見た形は同じ**。
 */
let cached: PhotoBackend | null = null

export function useDesktop(): PhotoBackend {
  if (cached) return cached
  cached = isTauriRuntime() ? createTauriBackend() : createLocalBackend()
  return cached
}
