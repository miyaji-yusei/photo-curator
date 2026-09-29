export interface Photo {
  id: string
  projectId: string
  path: string
  relativePath: string
  name: string
  capturedAt: number | null
  dHash: string | null
  /**
   * 0〜5 の星。**選別状態を表す唯一の値**。0 は未評価。
   * 選別は星を上げる操作で、次に何を選別するかも星で決まる。
   * 同じ星に集まった写真は、どの経路で来たかに関係なく1つの対象になる。
   */
  rating: number
  /** 解析時に一度だけ作られる 256px のサムネイル。未解析なら null。 */
  thumbnailPath: string | null
  /**
   * 選別画面に出す表示用画像。**まだ作っていなければ null**。
   * サムネイル(160x120 相当)では良し悪しを判断できず、原本(6.7MB)を毎回読むと
   * ネットワーク越しでは重すぎる。その中間がこれ。
   */
  displayPath: string | null
}

/** 星の上限。1ラウンド通過ごとに +1 で頭打ち。 */
export const MAX_RATING = 5

export type PhotoSort = 'rating' | 'name'

/** 1グループぶんの判定結果。星だけを送る。 */
export interface SelectionResult {
  id: string
  rating: number
}

/** `counts[n]` が★n の枚数（0〜5）。 */
export interface SelectionSummary {
  counts: number[]
  total: number
}

export interface ExportReport {
  processed: number
  skipped: number
  failed: number
  errors: string[]
}

export interface PhotoPage {
  photos: Photo[]
  total: number
}

/** `background` は scan 完了後の事前生成。UI をブロックしない。 */
export type ProjectTask = 'scan' | 'burst' | 'background' | 'display'

export interface ProjectProgress {
  projectId: string
  task: ProjectTask
  phase: 'indexing' | 'metadata' | 'hashing' | 'complete' | 'cancelled' | 'error'
  processed: number
  total: number
  message: string
  /** 解析は続くが利用者に伝えるべきこと（連写の時間窓を自動的に狭めた等）。 */
  warning: string | null
  /** 解析できなかった写真の累計。0 でない限り UI に出す。解析自体は続行する。 */
  failed: number
}

export interface Project {
  id: string
  name: string
  folderPath: string
  photoCount: number
  status: 'new' | 'scanning' | 'ready' | 'missing'
  createdAt: number
  updatedAt: number
  /** このプロジェクトで学習済みの連写まとめ閾値。未学習なら null。 */
  burstThreshold: number | null
  burstThresholdLearnedAt: number | null
  /**
   * 写真の出所の種類。`folder` はフォルダ（PC のフォルダ・ブラウザの取り込み）、
   * `amazon` は Amazon Photos の共有リンク（PC だけ）。Amazon のとき `folderPath` はリンクの URL。
   */
  sourceKind: 'folder' | 'amazon'
  /**
   * 写真の出所（ブラウザだけ）。省略はデスクトップ（フォルダ参照）。
   * `picker` は写真ピッカー、`folder` は File System Access のフォルダ、`dev` は開発用の HTTP。
   */
  source?: 'picker' | 'folder' | 'dev'
  /** `folder` のとき、次に読む許可がまだ無ければ 'needs-permission'（ボタンで許可を求める）。 */
  folderAccess?: 'granted' | 'needs-permission'
}

/** Amazon Photos の共有リンクを読んだ結果（作成画面）。 */
export interface AmazonPreview {
  /** `"{host}|{shareId}"`。 */
  key: string
  name: string
  count: number
  /** 見本（最大 12 枚）の JPEG のパス。`photoUrl` で表示する。 */
  samples: string[]
}

export interface TournamentSettings {
  groupSize: number
  groupBursts: boolean
}

export interface BurstGroup {
  id: string
  photoIds: string[]
  capturedSpanMs: number
  similarity: number
  accepted: boolean | null
}

export type SelectionStage =
  | 'settings'
  | 'burst-threshold'
  | 'burst-preview'
  | 'tournament'
  | 'result'
  | 'burst-final'
