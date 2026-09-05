/**
 * 実行環境ができること。**「Tauri かどうか」で分岐しない**ための土台。
 *
 * これまで画面は `isDesktop()` を見て出し分けていたが、その中身は
 * 「Tauri ランタイムが在るか」でしかなかった。**Android も Tauri なので true になり**、
 * 1 グループ 10 枚（デスクトップ既定）で出たり、物理キーボードの案内が出たりする。
 *
 * 「どの実装か」ではなく「何ができるか」を持てば、環境が増えても画面側は
 * 表を 1 行足すだけで済む。
 */

/** どの実行環境か。**能力の導出以外に使わない。** */
export type Platform = 'desktop' | 'android' | 'browser'

export interface BackendCapabilities {
  /** フォルダを指定して走査できる。ブラウザだけができない（ピッカー経由になる）。 */
  browseFolders: boolean
  /** 物理キーボードを想定してよい。ショートカットの案内を出すかの判断に使う。 */
  keyboard: boolean
  /** 1 グループに 10 枚並べても 1 枚が潰れない画面か。 */
  largeGroups: boolean
  /** 原本のフルサイズを表示できる。 */
  fullResolution: boolean
  /** 星ごとのフォルダへ書き出せる。 */
  exportFolders: boolean
  /** 原本の XMP に星を書ける。**原本を書き換えるので、触れる環境を絞る。** */
  writeMetadata: boolean
}

/**
 * 環境から能力を引く。**表そのもの**なので、増えるときはここに 1 列足す。
 *
 * Android を desktop と分けているのは画面の広さとキーボードの有無だけで、
 * ファイルを扱う能力（走査・書き出し）は Rust 側が面倒を見るので同じ。
 */
export function capabilitiesFor(platform: Platform): BackendCapabilities {
  switch (platform) {
    case 'desktop':
      return {
        browseFolders: true,
        keyboard: true,
        largeGroups: true,
        fullResolution: true,
        exportFolders: true,
        writeMetadata: true
      }
    case 'android':
      return {
        browseFolders: true,
        // 指で選ぶ。画面も狭いので既定 4 枚・上限 9 枚に寄せる。
        keyboard: false,
        largeGroups: false,
        fullResolution: true,
        exportFolders: true,
        // 原本の書き換えは、まず PC だけに留める。
        writeMetadata: false
      }
    case 'browser':
      return {
        // ブラウザはフォルダを走査できない。写真ピッカーが唯一の入口。
        browseFolders: false,
        keyboard: false,
        largeGroups: false,
        // 原本はセッション中しか持てないので、リロード後は表示用までしか出せない。
        fullResolution: false,
        exportFolders: false,
        writeMetadata: false
      }
  }
}

/**
 * いまの環境を判定する。
 *
 * Tauri かどうかは `isTauriRuntime()` が見る。そのうえで **Android かどうかは
 * User-Agent で見る**。Android の WebView は必ず `Android` を含む。
 * Rust に問い合わせる手もあるが、非同期になって画面の初期化が濁るので採らない。
 */
export function detectPlatform(isTauri: boolean, userAgent: string): Platform {
  if (!isTauri) return 'browser'
  return /android/i.test(userAgent) ? 'android' : 'desktop'
}
