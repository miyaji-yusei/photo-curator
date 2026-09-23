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
  /** Amazon Photos の共有リンクを読める。段7でブラウザから実測: JSON API は
   *  CORS 開放だが画像 CDN は不可（fetch・crossOrigin="anonymous" とも拒否）。
   *  Web からは原本はおろか見本すら安定して読めないため、PC（Rust 経由。CORS
   *  の制約を受けない）だけ true とする。 */
  amazon: boolean
  /** OS の共有ライブラリ（お気に入り等）に書ける。 */
  mediaStoreWrite: boolean
  /** 共有シート（Web Share API）を持つ。 */
  share: boolean
  /** 星ごとのフォルダへコピーして書き出せる。 */
  exportFolders: boolean
  /** 原本の XMP に星を書ける。原本を書き換えるので環境を絞る。 */
  writeMetadata: boolean
  /** 星ごとの star-N/ に分けた ZIP を書き出せる（Web の差分。02章）。 */
  exportZip: boolean
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
    amazon: true,
    mediaStoreWrite: false,
    share: false,
    exportFolders: true,
    writeMetadata: true,
    exportZip: false,
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
    exportZip: true,
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
    exportZip: true,
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
const DEV_ENV_KEY = 'photo-curator:devEnv'

export function detectEnvironment(): Environment {
  if (typeof window === 'undefined') return 'webFolder'
  if ('__TAURI_INTERNALS__' in window) return 'pc'
  // 開発用の切り替え（?env=webPicker）。実機の iPad が無くても、
  // ブラウザペインで `showDirectoryPicker` が使える PC の Chrome から
  // Web（ピッカー）の画面を確かめるための抜け道（本番ビルドには出ない）。
  // クエリは画面遷移で消えるので、一度指定したら sessionStorage に控えて
  // タブを閉じるまで（別画面に移っても）同じ環境のまま確かめられるようにする。
  if (import.meta.dev && typeof location !== 'undefined') {
    const forced = new URLSearchParams(location.search).get('env')
    if (forced === 'webPicker' || forced === 'webFolder') {
      try { sessionStorage.setItem(DEV_ENV_KEY, forced) } catch { /* 無視してよい */ }
      return forced
    }
    try {
      const remembered = sessionStorage.getItem(DEV_ENV_KEY)
      if (remembered === 'webPicker' || remembered === 'webFolder') return remembered
    } catch { /* 無視してよい */ }
  }
  return 'showDirectoryPicker' in window ? 'webFolder' : 'webPicker'
}

export function useCapabilities() {
  const environment = detectEnvironment()
  const capabilities = capabilitiesFor(environment)
  return { environment, capabilities }
}
