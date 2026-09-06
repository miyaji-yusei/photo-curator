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

export interface SelectionSeed {
  id: string
  rating: number
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
   * 写真の出所。**画面はこれだけを見る。**
   *
   * `folderPath` は `smb://192.168.11.8/Share/2021_06_13` のような機械の
   * 言葉で、そのまま出すと読みにくい。人の言葉は `sourceLabel`、生パスは
   * 「技術情報」にだけ出す。
   */
  sourceKind: SourceKind
  sourceLabel: string
}

/** 写真の出所の種類。画面のアイコンと、できることの分岐に使う。 */
export type SourceKind = 'folder' | 'album' | 'nas' | 'imported'

/** 裏で進む 1 本ぶんの状態。 */
export interface PrepStage {
  state: 'idle' | 'running' | 'done' | 'error'
  done: number
  /** 走査中は枚数が確定しないので null。画面は「120 / ?」と出す。 */
  total: number | null
}

/**
 * プロジェクトの準備状況。**3 本を 1 か所で持つ。**
 *
 * 以前は走査の進捗と表示用画像の残数が別々の仕組みで出ていて、画面の
 * 2 か所に違う進捗が並び、しかもプロジェクトを切り替えても前の値が残っていた。
 */
export interface Prep {
  projectId: string
  scan: PrepStage
  meta: PrepStage
  preview: PrepStage
  previewEdge: number
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

/**
 * 隣り合う2枚。判断の単位はグループではなくこのペア。
 * グループは「連続するペアがすべて閾値を満たす区間」として導かれるので、
 * 「1枚目はまとめないが2と3枚目はまとめる」はペア単位の判断だけで表現できる。
 */
export interface BurstPair {
  id: string
  leftPhotoId: string
  rightPhotoId: string
  /** dHash のハミング距離。小さいほど構図が近い。 */
  distance: number
  gapMs: number
}

export interface PairAnswer {
  pairId: string
  distance: number
  grouped: boolean
}

/** 距離順に並べた候補ペアに対する二分探索の状態。 */
export interface ThresholdState {
  lowIdx: number
  highIdx: number
  answers: PairAnswer[]
  skipped: string[]
}

/**
 * 1グループぶんの判断。選び間違えたときに1手ずつ戻すために積む。
 * survivors と ratings は差分で戻せるので、スナップショットは持たない。
 * 大量写真では survivors が数千件になり、毎クリック保存する JSON が肥大化する。
 */
export interface SelectionStep {
  groupIndex: number
  /** そのグループで通した写真。1枚も選ばなかったときは空。 */
  chosen: string[]
}

export type SelectionStage =
  | 'settings'
  | 'burst-threshold'
  | 'burst-preview'
  | 'tournament'
  | 'result'
  | 'burst-final'

export interface SelectionSession {
  /**
   * セッションの形の版。**合わない保存は捨てる。**
   *
   * 星もサムネイルも写真の側に持っているので、捨てて困るのは
   * 「いまどこまで選んだか」だけ。形を変えるたびに移行を書くより、
   * 最初からやり直してもらう方が確実で、失うものも小さい。
   */
  version?: number
  projectId: string
  settings: TournamentSettings
  candidates: string[]
  groups: string[][]
  groupIndex: number
  selectedInGroup: string[]
  multiSelect: boolean
  ratings: Record<string, number>
  survivors: string[]
  /** 1手戻すための履歴。ラウンドをまたいでは戻せないので開始時に空にする。 */
  history: SelectionStep[]
  /** この回で選別している星。ここに集まった写真だけが対象になる。 */
  targetRating: number
  /**
   * まとめた連写。キーが代表写真の id、値は代表を含む全メンバー。
   * トーナメントには代表だけを出し、1カードとして扱う。
   */
  burstMembers: Record<string, string[]>
  /**
   * まとめの中で**もう星が決まった**写真。★5 で確定したものと、明らかに脱落と
   * して下げたものが入る。
   *
   * これが無いと `spreadBurstRatings` が「代表と同じ星」を配るときに上書きして
   * しまい、せっかく下げた星が元に戻る。
   */
  burstSettled: string[]
  round: number
  /** 閾値を適用した結果のグループ。確認画面と、以降のまとめ処理で使う。 */
  burstGroups: BurstGroup[]
  /** 距離順に並べた候補ペア。出題元。 */
  burstPairs: BurstPair[]
  burstThresholdState: ThresholdState
  /** 学習・調整済みの閾値。未決定なら null。 */
  burstThreshold: number | null
  stage: SelectionStage
  updatedAt: number
  seed?: number
}
