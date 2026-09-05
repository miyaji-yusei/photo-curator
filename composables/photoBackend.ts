import type { BackendCapabilities } from '~/utils/capabilities'
import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSeed, SelectionSession, SelectionSummary
} from '~/types/photo'

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
  /** rating を渡すとその星の写真だけ。選別対象は星で決まる。 */
  getSelectionSeed: (projectId: string, rating?: number) => Promise<SelectionSeed[]>

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

  /** threshold を省くとプロジェクトの学習値、それも無ければ既定値が使われる。 */
  getBurstGroups: (projectId: string, threshold?: number) => Promise<BurstGroup[]>
  /** 閾値学習の出題元。距離での足切りはされていない。 */
  getBurstPairs: (projectId: string) => Promise<BurstPair[]>
  saveBurstThreshold: (projectId: string, threshold: number) => Promise<void>
  clearBurstThreshold: (projectId: string) => Promise<void>
  /**
   * まとまりを見直すための「1続きの写真」。指定した写真の前後 `windowMs` に
   * 入るものを撮影順で返す。まとめの中身も、まとめに入れられる近くの写真も、
   * どちらもこの1本の並びの上にある。
   */
  getBurstNeighborhood: (
    projectId: string, photoIds: string[], windowMs?: number
  ) => Promise<Photo[]>
  /**
   * 見直した結果の形を保存する。渡すのは**例外そのものではなく「こう分かれて
   * いてほしい」という形**で、閾値との食い違いだけが例外として残る。
   * 何度保存しても結果が変わらない。
   */
  saveBurstShape: (
    projectId: string, orderedPhotoIds: string[], blocks: string[][]
  ) => Promise<void>

  startProjectScan: (projectId: string) => Promise<void>
  startBurstAnalysis: (projectId: string) => Promise<void>
  /** まだ解析が要る写真の枚数。0 なら事前生成を起動しない。 */
  getAnalysisBacklog: (projectId: string) => Promise<number>
  /** scan 完了後の事前生成。既に走っていても失敗しない。 */
  startBackgroundAnalysis: (projectId: string) => Promise<void>
  cancelProjectTask: (projectId: string, task: ProjectTask) => Promise<void>

  saveSession: (session: SelectionSession) => Promise<void>
  loadSession: (projectId: string) => Promise<SelectionSession | null>

  /**
   * 原本を表示するための URL。
   * デスクトップは絶対パスを asset プロトコルへ、ブラウザは既に URL なのでそのまま。
   */
  photoUrl: (path: string) => string
  /**
   * 一覧に並べるための URL。解析時に作った 256px のサムネイルを使い回すので
   * 追加のデコードは無い。まだ解析していない写真は原本へ落ちる。
   */
  photoThumbnailUrl: (photo: Photo) => string

  /** 星ごとのフォルダへ書き出す。moveFiles が true なら原本を移動する。 */
  exportByRating: (
    projectId: string, destination: string, ratings: number[], moveFiles: boolean
  ) => Promise<ExportReport>
  /** 星を写真本体の XMP に書き込む。原本を書き換える。 */
  writeRatingsToFiles: (projectId: string, ratings: number[]) => Promise<ExportReport>

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

export type BackendKind = 'tauri' | 'local'

/** どの実装を使うかは Tauri の有無だけで決まる。 */
export const isTauriRuntime = () =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
