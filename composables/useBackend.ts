// いまの環境に合う Backend を 1 つ返す。**画面はここ経由でしか出入力しない。**
import type { Backend } from '~/lib/backend'
import { WebFolderBackend } from '~/lib/backends/webFolder'
import { WebPickerBackend } from '~/lib/backends/webPicker'
import { TauriBackend } from '~/lib/backends/tauri'
import { withAmazonWeb } from '~/lib/backends/amazonWeb'
import { detectEnvironment } from '~/composables/useCapabilities'

let instance: Backend | null = null

export function useBackend(): Backend {
  if (instance) return instance
  const environment = detectEnvironment()
  if (environment === 'pc') instance = new TauriBackend()
  // Web は Amazon 出所のプロジェクトだけ表示専用の経路へ回す（lib/backends/amazonWeb.ts）。
  else if (environment === 'webPicker') instance = withAmazonWeb(new WebPickerBackend())
  else instance = withAmazonWeb(new WebFolderBackend())
  return instance
}
