import type { BackendCapabilities } from '~/utils/capabilities'

/** 写真の出所。path はそのまま createProject に渡せる。 */
export interface PhotoAlbum {
  path: string
  name: string
  count: number
}

/** 表示用サイズの選択肢と、いまの既定。設定画面がそのまま使う。 */
export interface NasSettings {
  host: string
  share: string
  user: string
  rememberPassword: boolean
  /** 覚えていれば入っている。 */
  password: string
  /** この環境でパスワードを預かれるか。false ならトグルを出さない。 */
  canRememberPassword: boolean
}

export interface NasCredentials {
  host: string
  share: string
  user: string
  rememberPassword: boolean
  password: string
}

export interface DisplaySettings {
  edge: number
  choices: number[]
  defaultEdge: number
  largeEdge: number
}
import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoPage, PhotoSort, Prep, Project, ProjectProgress, ProjectTask, SelectionResult, SelectionSeed, SelectionSession, SelectionSummary, SourceKind
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
  /**
   * この端末で選べる写真の出所。
   * デスクトップは空（フォルダ選択を出す）。**Android はアルバムの一覧**で、
   * 走査できるフォルダが無いため、これが「フォルダを選ぶ」の代わりになる。
   */
  listPhotoAlbums: () => Promise<PhotoAlbum[]>
  /**
   * NAS へ繋ぐ。繋がったら共有の直下のフォルダを返す。
   * **認証情報は保存しない。** アプリを終了すると消える。
   */
  connectNas: (
    host: string, share: string, user: string, password: string
  ) => Promise<PhotoAlbum[]>
  /**
   * いま繋いでいる NAS のフォルダ一覧。**繋ぎ直さずに取り直せる。**
   * これが無いと、画面は connectNas の戻り値を持ち回るしかない。
   */
  listNasFolders: () => Promise<PhotoAlbum[]>
  disconnectNas: () => Promise<void>
  /**
   * 前回の繋ぎ先。ダイアログを開くたびに読む。
   *
   * ホスト・共有名・利用者名は毎回入れ直すのが煩わしいだけで秘密ではない。
   * パスワードは「覚える」を選んだときだけ、端末の鍵で包んで預けてある。
   */
  getNasSettings: () => Promise<NasSettings>
  saveNasSettings: (settings: NasCredentials) => Promise<void>

  /** 解析の進捗。戻り値を呼ぶと購読を解除する。 */
  onProjectProgress: (callback: (progress: ProjectProgress) => void) => Promise<() => void>

  listProjects: () => Promise<Project[]>
  createProject: (
    name: string,
    folderPath: string,
    source?: { kind: SourceKind; label: string }
  ) => Promise<Project>
  /** 準備の 3 本立て。プロジェクトごとに取り直す。 */
  getProjectPrep: (projectId: string) => Promise<Prep>
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
