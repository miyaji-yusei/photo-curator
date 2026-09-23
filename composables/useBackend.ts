// いまの環境に合う Backend を 1 つ返す。**画面はここ経由でしか出入力しない。**
import type { Backend } from '~/lib/backend'
import { WebFolderBackend } from '~/lib/backends/webFolder'
import { TauriBackend } from '~/lib/backends/tauri'
import { detectEnvironment } from '~/composables/useCapabilities'

let instance: Backend | null = null

export function useBackend(): Backend {
  if (instance) return instance
  const environment = detectEnvironment()
  instance = environment === 'pc' ? new TauriBackend() : new WebFolderBackend()
  return instance
}
