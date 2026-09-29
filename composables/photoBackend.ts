import type { BackendCapabilities } from '~/utils/capabilities'

/** 表示用サイズの選択肢と、いまの既定。設定画面がそのまま使う。 */
export interface DisplaySettings {
  edge: number
  choices: number[]
  defaultEdge: number
  largeEdge: number
}
import type {
  AmazonPreview, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSummary
} from '~/types/photo'
import type { PairOverride } from '~/lib/core'
import type { SavedSelection } from '~/utils/selectionFlow'

/**
 * 画面が使うデータ操作の全体。**フロントとバックエンドの唯一の接点**で、
 * `app.vue` / `utils/*` / `types/*` はここより下を知らない。
 *
 * 実装は 2 つ:
 * - `TauriBackend` … デスクトップ。PC の SQLite と原本フォルダ
 * - `LocalBackend`  … iPad などのブラウザ。端末内の DB とサムネイル
 *
 * 将来 PC のプロジェクトを遠隔選別する `HttpBackend` を足すときも、
 * このインターフェースを実装するだけで画面側は変えずに済む。
 */
export interface PhotoBackend {
  /** どの実装か。画面の出し分けに使う。 */
  readonly kind: BackendKind

  /**
   * この環境ができること。**画面はこれだけを見て出し分ける。**
   * 「Tauri かどうか」で分岐すると、Android を desktop と取り違える。
   */
  readonly capabilities: BackendCapabilities

  chooseFolder: () => Promise<string | null>

  /** 解析の進捗。戻り値を呼ぶと購読を解除する。 */
  onProjectProgress: (callback: (progress: ProjectProgress) => void) => Promise<() => void>

  listProjects: () => Promise<Project[]>
  createProject: (name: string, folderPath: string) => Promise<Project>
  /** 写真原本は消さない。DB の行とサムネイルだけ。 */
  deleteProject: (projectId: string) => Promise<void>

  /** rating を渡すとその星ちょうどの写真だけ。null は全件。 */
  getProjectPhotoPage: (
    projectId: string, offset?: number, limit?: number,
    rating?: number | null, sort?: PhotoSort
  ) => Promise<PhotoPage>
  getPhotosByIds: (projectId: string, photoIds: string[]) => Promise<Photo[]>
  /**
   * core に渡す写真の行。**その星に関係なく全件**（欠損を除く）を 1 回で返す。
   * 並べ替えは呼び出し側（`utils/coreInputs.ts`）が撮影順にする。
   * サムネイルなどの URL は持たない（写真そのものの表示は `getPhotosByIds`）。
   */
  getCoreInputs: (projectId: string) => Promise<Photo[]>

  /** 判定したグループぶんだけを書く。全件を毎回送らない。 */
  saveSelectionResults: (projectId: string, entries: SelectionResult[]) => Promise<void>
  resetSelectionResults: (projectId: string) => Promise<void>
  /**
   * ある星の写真をまとめて別の星へ移す。移した枚数を返す。
   * `includeIds` を渡すとその写真だけ、渡さなければ `excludeIds` を除いた
   * **その星の全件**が対象。5,000 枚ぶんの id を送らずに済ませるため、
   * 「全選択からの差分」だけを渡す形にしてある。
   */
  moveRating: (
    projectId: string, fromRating: number, toRating: number,
    includeIds?: string[] | null, excludeIds?: string[]
  ) => Promise<number>
  getSelectionSummary: (projectId: string) => Promise<SelectionSummary>

  /** 学習した連写の距離をプロジェクトに覚えさせる（次の選別で質問を省く）。 */
  saveBurstThreshold: (projectId: string, threshold: number) => Promise<void>
  clearBurstThreshold: (projectId: string) => Promise<void>
  /**
   * 手で直した連写の例外（core の `PairOverride`、鍵は relativePath）。
   * **例外そのもの**を保存する。「こう分かれていてほしい」という形からの
   * 導き方は `utils/burstShape.ts`。
   */
  getPairOverrides: (projectId: string) => Promise<PairOverride[]>
  /** そのプロジェクトの手直しを、渡したものに丸ごと入れ替える。 */
  savePairOverrides: (projectId: string, overrides: PairOverride[]) => Promise<void>

  /** 表示用画像の設定と生成。 */
  getDisplaySettings: () => Promise<DisplaySettings>
  saveDisplayEdge: (edge: number) => Promise<number>
  /** null を渡すと全体の設定に戻す。戻り値は解決後の長辺。 */
  saveProjectDisplayEdge: (projectId: string, edge: number | null) => Promise<number>
  /** まだ表示用画像が要る枚数。0 なら生成を起動しない。 */
  getDisplayBacklog: (projectId: string) => Promise<number>
  startDisplayGeneration: (projectId: string) => Promise<void>
  /** 作り直しのために印を消す。呼んだあと startDisplayGeneration する。 */
  resetDisplayImages: (projectId: string) => Promise<void>

  startProjectScan: (projectId: string) => Promise<void>
  startBurstAnalysis: (projectId: string) => Promise<void>
  /** まだ解析が要る写真の枚数。0 なら事前生成を起動しない。 */
  getAnalysisBacklog: (projectId: string) => Promise<number>
  /** scan 完了後の事前生成。既に走っていても失敗しない。 */
  startBackgroundAnalysis: (projectId: string) => Promise<void>
  cancelProjectTask: (projectId: string, task: ProjectTask) => Promise<void>

  /** 封筒（`v: 2`）ごと保存する。`core` は core の Session そのまま。null は途中の選別を消す。 */
  saveSession: (projectId: string, selection: SavedSelection | null) => Promise<void>
  /** 旧版の形（`v` が無い）は null。旧データは引き継がない。 */
  loadSession: (projectId: string) => Promise<SavedSelection | null>

  // ---- サイドカー（写真のフォルダ直下の `.photo-curator/catalog.json`）----
  // 口だけ。**4 通りの判断は持たない**（`useSidecarSync` が core の `sidecarDecide` を呼ぶ）。

  /** この出所にサイドカーを書けるか。`readonly` は読むだけ、`none` は読み書きとも無い。 */
  sidecarSupported: (projectId: string) => Promise<SidecarAccess>
  /** サイドカーの JSON の文字列。無ければ null。 */
  readSidecar: (projectId: string) => Promise<string | null>
  /** 原子的に書く（一時ファイル → rename）。`fileName` は退避（`catalog.<id>.json`）のときだけ変える。 */
  writeSidecar: (projectId: string, json: string, fileName?: string) => Promise<void>
  /** 端末が覚える、最後に読んだ／書いたサイドカーの印と、判断が変わったか。 */
  loadSidecarState: (projectId: string) => Promise<SidecarState>
  saveSidecarState: (projectId: string, state: SidecarState) => Promise<void>
  /** この端末の id と名前。id は初回に作って残す。 */
  deviceIdentity: () => Promise<DeviceIdentity>

  /**
   * 原本を表示するための URL。
   * デスクトップは絶対パスを asset プロトコルへ、ブラウザは既に URL なのでそのまま。
   */
  photoUrl: (path: string) => string
  /**
   * 拡大のときだけ使う、原本の URL。フォルダの写真は `photoUrl(photo.path)` と同じ。
   * Amazon の写真は、ここで原本を取ってきて端末に置き（あれば使い回す）、そのパスの URL を返す。
   * 取れなければ reject する（リンクが消えていたら「このリンクは削除されたか、無効です。」）。
   */
  photoOriginalUrl: (photo: Photo) => Promise<string>
  /**
   * 一覧に並べるための URL。解析時に作った 256px のサムネイルを使い回すので
   * 追加のデコードは無い。まだ解析していない写真は原本へ落ちる。
   */
  photoThumbnailUrl: (photo: Photo) => string
  /**
   * **選別画面に出すための URL。** 表示用画像 → サムネイル → 原本 の順に落ちる。
   * 原本まで落ちるのはまだ生成が追いついていないときだけ。
   */
  photoDisplayUrl: (photo: Photo) => string

  /** 星ごとのフォルダへ書き出す。moveFiles が true なら原本を移動する。 */
  exportByRating: (
    projectId: string, destination: string, ratings: number[], moveFiles: boolean
  ) => Promise<ExportReport>
  /** 星を写真本体の XMP に書き込む。原本を書き換える。 */
  writeRatingsToFiles: (projectId: string, ratings: number[]) => Promise<ExportReport>

  // ---- Amazon Photos の共有リンク（PC だけ。`capabilities.amazon` が true のとき）----
  // 走査・準備・表示用・書き出しの分岐は Rust の入口が `source_kind` で行う。画面は分岐しない。

  /** 共有リンクを読み、名前・枚数・見本（最大 12 枚）を返す。 */
  amazonPreview?: (shareUrl: string) => Promise<AmazonPreview>
  /** 共有リンクのプロジェクトを作る（走査はこのあと `startProjectScan`）。 */
  createAmazonProject?: (name: string, shareUrl: string) => Promise<Project>

  // ---- ブラウザだけが持つ機能 ------------------------------------------
  // デスクトップはフォルダ走査と原本パスがあるので必要ない。
  // 画面側は存在を確かめてから呼ぶ。

  /** 写真ピッカーで選ばれたファイルを取り込み、そのまま解析する。 */
  importPhotos?: (projectId: string, files: File[]) => Promise<void>
  /**
   * 共有シートや ZIP 書き出しに渡せる原本。
   * iOS には永続的なファイルハンドルが無いため、**リロードすると失われる**。
   */
  originalFile?: (photoId: string) => File | null
}

export type SidecarAccess = 'readwrite' | 'readonly' | 'none'

export interface SidecarState {
  /** 最後に読んだ／書いたサイドカーの `updatedAt`。未確認は 0。 */
  seenAt: number
  /** 同じく `updatedBy`。未確認は空文字。 */
  seenBy: string
  /** 判断（星・連写の手直し・学習した距離・やり直し）が変わったか。 */
  localChanged: boolean
}

export interface DeviceIdentity {
  id: string
  name: string
}

export type BackendKind = 'tauri' | 'local'

/** どの実装を使うかは Tauri の有無だけで決まる。 */
export const isTauriRuntime = () =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
