// プロジェクト・写真の保存の形（設計 03 章）。
// 判断（星・連写・選別の状態遷移）は core（lib/core.ts の Session）が持つ。
// ここは「端末に置くもの」の形だけ。

export type SourceKind = 'folder' | 'amazon'

export interface ProjectSource {
  kind: SourceKind
  /** フォルダなら絶対パス（PC）または表示名（Web フォルダ）。Amazon なら共有リンクの id。 */
  key: string
  /** 表示用の名前（フォルダ名・共有名）。 */
  label: string
}

/** 顔ぶれ・指紋・絵の状態。1 枚ぶん。 */
export interface ProjectPhoto {
  relativePath: string
  capturedAt: number | null
  dHash: string | null
  dHashVersion: number
  /** バイト数。原本の差し替え判定（size + mtime）に使う。 */
  size: number
  mtimeMs: number
  hasThumbnail: boolean
  hasDisplay: boolean
}

export type PrepareTask = 'scan' | 'meta' | 'hash' | 'display'

export interface PrepareProgress {
  task: PrepareTask
  done: number
  total: number
  /** つまずいたときの一言。続けられるなら null。 */
  warning: string | null
}

export interface Project {
  id: string
  name: string
  source: ProjectSource
  photoCount: number
  createdAt: number
  updatedAt: number
  /** 準備がどこまで進んだか。画面はこれだけを見る（出所へ問い合わせない）。 */
  scannedCount: number
  metaHashedCount: number
  displayedCount: number
  /** 準備で見つかった、続けられる範囲のつまずき。 */
  prepareWarning: string | null
  /** 学習した連写の境目。学習していなければ null。 */
  burstDistance: number | null
}

export const DEFAULT_BURST_WINDOW_MS = 4000
export const DEFAULT_BURST_DISTANCE = 9

export interface AppSettings {
  /** 表示用画像の長辺。既定 1024。 */
  displayEdge: number
  /** 選別の既定枚数。capabilities.largeGroups で上限が変わる。 */
  groupSize: number
  /** 連写を自動でまとめるか。 */
  groupBursts: boolean
  /** 開始前に毎回 startSheet を出すか。 */
  confirmBeforeStart: boolean
}

export const DEFAULT_SETTINGS: AppSettings = {
  displayEdge: 1024,
  groupSize: 4,
  groupBursts: true,
  confirmBeforeStart: true
}
