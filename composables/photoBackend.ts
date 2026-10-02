import type { BackendCapabilities } from '~/utils/capabilities'

/** 表示用サイズの選択肢と、いまの既定。設定画面がそのまま使う。 */
export interface DisplaySettings {
  edge: number
  choices: number[]
  defaultEdge: number
  largeEdge: number
  /** 長辺を変えたあとで、作った画像を作り直せるか。ブラウザは原本を持たないので false。 */
  canRebuild?: boolean
  /** 長辺を変えると、作った画像を作り直すか。Web の Amazon は URL で出すので作り直さない（既定は true）。 */
  rebuildsOnChange?: boolean
  /** プロジェクトを渡して読んだときの、そのプロジェクトの実効の長辺。 */
  projectEdge?: number
}
import type {
  AmazonExport, AmazonPreview, ExportReport, Photo, PhotoPage, PhotoSort, Project,
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
  /** `projectId` を渡すと、そのプロジェクトの実効の長辺（`projectEdge`）と作り直せるか（`canRebuild`）も返す。 */
  getDisplaySettings: (projectId?: string) => Promise<DisplaySettings>
  saveDisplayEdge: (edge: number) => Promise<number>
  /** null を渡すと全体の設定に戻す。戻り値は解決後の長辺。 */
  saveProjectDisplayEdge: (projectId: string, edge: number | null) => Promise<number>
  /** 「同名の JPEG と RAW を 1 枚の写真として扱う」の設定を保存する。反映は次の走査から。戻り値は保存後の値。 */
  saveProjectPairRaw: (projectId: string, enabled: boolean) => Promise<boolean>
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
  // 口だけ。**判断は持たない**（`useSidecarSync` が core の `sidecarPlan` を呼ぶ）。

  /** この出所にサイドカーを書けるか。`readonly` は読むだけ、`none` は読み書きとも無い。 */
  sidecarSupported: (projectId: string) => Promise<SidecarAccess>
  /** サイドカーの JSON の文字列。無ければ null。 */
  readSidecar: (projectId: string) => Promise<string | null>
  /** 原子的に書く（一時ファイル → rename）。退避（`catalog.<id>.json`）に使う。 */
  writeSidecar: (projectId: string, json: string, fileName?: string) => Promise<void>
  /**
   * `catalog.json` を楽観ロックで書く（設計書 §4.4）: ロック → 読んで `expected`（判断に使った中身。
   * 無かったなら null）と同じか確かめる → 一時ファイル → 置き換え → 読み戻して確かめる → ロックを放す。
   * 見た版と違えば書かずに `changed`、ほかの端末が書いている最中なら `locked`。
   */
  writeSidecarChecked: (projectId: string, json: string, expected: string | null) => Promise<SidecarWriteResult>
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

  /**
   * 選んだ写真を星ごとのフォルダへ書き出す。moveFiles が true なら原本を移動する。
   * **対象は `photoIds`**（画面が連写の仲間まで広げて決める。星では選ばない）。
   * Amazon は原本を取ってきて置く（移動はできない）。
   */
  exportPhotos: (
    projectId: string, destination: string, photoIds: string[], moveFiles: boolean
  ) => Promise<ExportReport>
  /** 選んだ写真（`photoIds`）の星を、写真本体の XMP に書き込む。原本を書き換える。 */
  writeRatingsToPhotos: (projectId: string, photoIds: string[]) => Promise<ExportReport>
  /**
   * CSV を保存する。PC は保存ダイアログで選んだ場所に書き、ブラウザはダウンロードにする。
   * 保存ダイアログを閉じたら `false`。
   */
  saveCsv: (fileName: string, text: string) => Promise<boolean>

  // ---- Amazon Photos の共有リンク（PC だけ。`capabilities.amazon` が true のとき）----
  // 走査・準備・表示用・書き出しの分岐は Rust の入口が `source_kind` で行う。画面は分岐しない。

  /** 共有リンクを読み、名前・枚数・見本（最大 12 枚）を返す。 */
  amazonPreview?: (shareUrl: string) => Promise<AmazonPreview>
  /** 共有リンクのプロジェクトを作る（走査はこのあと `startProjectScan`）。 */
  createAmazonProject?: (name: string, shareUrl: string) => Promise<Project>
  /**
   * Amazon の結果の ZIP（ブラウザ）。対象の写真（`photoIds`）の原本を取って `star-N/` に分ける
   * （取れなければ reject。文に「CSV だけ書き出せます」を含む）。CSV は `saveCsv` で、全部の出所が同じ。
   */
  exportAmazon?: (projectId: string, photoIds: string[]) => Promise<AmazonExport>

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

export type SidecarWriteResult = 'written' | 'changed' | 'locked'

/**
 * 端末が覚える、サイドカーの控え。
 *
 * U34 から、変わったかどうかは印（`localChanged`）ではなく「見た版の比較キー（`seenKey`）と
 * 今の比較キーが違うか」で決める。古い 3 つ（`seenAt`・`seenBy`・`localChanged`）は、
 * まだ新しい項目が無い控え（`seenToken` が null／無い）から `legacy:` の控えを作るためと、
 * 古い版のアプリに戻したときのために書き続ける。
 */
export interface SidecarState {
  /** 最後に読んだ／書いたサイドカーの `updatedAt`。未確認は 0。 */
  seenAt: number
  /** 同じく `updatedBy`。未確認は空文字。 */
  seenBy: string
  /** 古い形の「判断が変わった」印。新しい控えがあるときは判断に使わない。 */
  localChanged: boolean
  /** 最後に読んだ／書いた版の見分け（core の `sidecarToken`）。null／無いは古い形。空は一度も見ていない。 */
  seenToken?: string | null
  /** その版を読んだ／書いたときの、この端末の選別状況の比較キー。空は「分からない＝変更あり」。 */
  seenKey?: string
  /** その版のやり直しの世代。 */
  seenEpoch?: string | null
  /** この端末の選別状況のやり直しの世代。 */
  localEpoch?: string | null
  /** 「この端末の状況を残す」を選んだあと（自動では書かない）。 */
  detached?: boolean
}

export interface DeviceIdentity {
  id: string
  name: string
}

export type BackendKind = 'tauri' | 'local'

/** どの実装を使うかは Tauri の有無だけで決まる。 */
export const isTauriRuntime = () =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
