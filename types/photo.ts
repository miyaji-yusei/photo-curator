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
  /**
   * 解析できなかった理由の種類（U58）。`unsupported`＝読めたのに復号できない非対応の形式（選別の対象から外す。
   * 行そのものは残る）、`transient`＝一時的、省略・null＝失敗なし。
   */
  analysisErrorKind?: AnalysisErrorKind | null
}

export type AnalysisErrorKind = 'unsupported' | 'transient'

/** 準備の状態を 1 回で返す口の値（R4）。 */
export interface PrepareState {
  /** まだ解析が要る写真の枚数。 */
  analysisBacklog: number
  /** まだ表示用画像が要る枚数。 */
  displayBacklog: number
  /** 解析できなかった枚数（非対応を含む）。 */
  failed: number
  /** うち、対応していない形式の枚数。 */
  unsupported: number
}

/** 解析できなかった写真 1 枚（U58）。警告の「一覧を見る」に出す。 */
export interface AnalysisFailure {
  relativePath: string
  name: string
  kind: AnalysisErrorKind
  reason: string
  /** 失敗した時刻（ms）。 */
  at: number | null
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
  /** 組の RAW の .xmp に書けた数（メタデータへの書き込みだけ。無ければ 0 扱い）。 */
  pairedRawProcessed?: number
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
   * 同名の JPEG と RAW を 1 枚の写真として扱う（RAW＋JPEG 同時撮影のとき、組の RAW を対象から外す）。
   * プロジェクトごとの設定で、既定は true。変えたら再走査で反映される（U46）。
   */
  pairRawJpeg: boolean
  /**
   * `pairRawJpeg` を切り替えた時刻（ms）。0・省略は「作ったまま一度も切り替えていない」。
   * サイドカーで端末どうしの設定が違うとき、新しく切り替えた方を採るのに使う（U48）。
   */
  pairRawJpegAt?: number
  /**
   * 写真の出所（ブラウザだけ）。省略はデスクトップ（フォルダ参照）。
   * `picker` は写真ピッカー、`folder` は File System Access のフォルダ、`dev` は開発用の HTTP。
   */
  source?: 'picker' | 'folder' | 'dev' | 'amazon'
  /** `folder` のとき、次に読む許可がまだ無ければ 'needs-permission'（ボタンで許可を求める）。 */
  folderAccess?: 'granted' | 'needs-permission'
}

/** Amazon の結果を書き出したもの（ブラウザ）。`skipped` は原本を取れず ZIP に入らなかった枚数。 */
export interface AmazonExport {
  blob: Blob
  fileName: string
  count: number
  skipped: number
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
