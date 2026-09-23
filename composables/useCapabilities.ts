/**
 * 実行環境ができること。設計 05 章の表そのもの。
 *
 * 「どの実装か」ではなく「何ができるか」で画面を分岐する（CON-5）。
 * Android 列は無い（このアプリは PC・Web だけ。Android は app-android/ の
 * ネイティブ実装）。
 */

export type Environment = 'pc' | 'webFolder' | 'webPicker'

export interface Capabilities {
  /** 物理キーボードを想定してよい。 */
  keyboard: boolean
  /** 1 グループ 10 枚まで選べる。無ければ groupSize.ts の TOUCH 側（2–4）。 */
  largeGroups: boolean
  /** フォルダを選んで走査できる。無ければ写真ピッカー等の取り込みになる。 */
  browseFolders: boolean
  /** 原本のフルサイズを表示できる。 */
  fullResolution: boolean
  /** SMB で NAS に直接繋げる。PC・Web は無し（PC は Windows のフォルダとして開く）。 */
  smb: boolean
  /** Amazon Photos の共有リンクを読める。CORS 実測待ち（段7）なので、いまは全環境 false。 */
  amazon: boolean
  /** OS の共有ライブラリ（お気に入り等）に書ける。 */
  mediaStoreWrite: boolean
  /** 共有シート（Web Share API）を持つ。 */
  share: boolean
  /** 星ごとのフォルダへコピーして書き出せる。 */
  exportFolders: boolean
  /** 原本の XMP に星を書ける。原本を書き換えるので環境を絞る。 */
  writeMetadata: boolean
  /** サイドカー（.photo-curator/catalog.json）を読み書きできる見込みがある。
   *  Web フォルダは実際に書けるかどうかは handle の許可次第（03 章）なので、
   *  ここでは「対応し得るか」だけを表す。 */
  sidecar: boolean
}

const TABLE: Record<Environment, Capabilities> = {
  pc: {
    keyboard: true,
    largeGroups: true,
    browseFolders: true,
    fullResolution: true,
    smb: false,
    amazon: false,
    mediaStoreWrite: false,
    share: false,
    exportFolders: true,
    writeMetadata: true,
    sidecar: true
  },
  webFolder: {
    keyboard: true,
    largeGroups: true,
    browseFolders: true,
    fullResolution: true,
    smb: false,
    amazon: false,
    mediaStoreWrite: false,
    share: false,
    exportFolders: false,
    writeMetadata: false,
    sidecar: true
  },
  webPicker: {
    keyboard: false,
    largeGroups: false,
    browseFolders: false,
    fullResolution: false,
    smb: false,
    amazon: false,
    mediaStoreWrite: false,
    share: true,
    exportFolders: false,
    writeMetadata: false,
    sidecar: false
  }
}

/** できない理由。`outputMenu` 等で灰色にしたボタンに添える（H-1）。 */
export const UNAVAILABLE_REASON: Record<Environment, string> = {
  pc: 'PC 版で使えます。',
  webFolder: 'PC 版で使えます。',
  webPicker: 'PC 版で使えます。'
}

export function capabilitiesFor(environment: Environment): Capabilities {
  return TABLE[environment]
}

/**
 * いまの環境を判定する。Tauri なら PC、そうでなければ
 * `showDirectoryPicker` の有無で Web フォルダ／ピッカーを分ける
 * （ブラウザ名では見ない。05 章「分けるのは機能の有無」）。
 */
export function detectEnvironment(): Environment {
  if (typeof window === 'undefined') return 'webFolder'
  if ('__TAURI_INTERNALS__' in window) return 'pc'
  return 'showDirectoryPicker' in window ? 'webFolder' : 'webPicker'
}

export function useCapabilities() {
  const environment = detectEnvironment()
  const capabilities = capabilitiesFor(environment)
  return { environment, capabilities }
}
