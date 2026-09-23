// 入出力の口。**判断を持たない**（設計 07 章 段2-3）。
// 実装は 3 つ: tauri（lib/backends/tauri.ts）・webFolder（lib/backends/webFolder.ts）・
// webPicker（段6・未着手）。画面はこのインターフェースだけを見る。

import type { Session, Sidecar, PairOverride, SidecarSync } from '~/lib/core'
import type { AppSettings, Project, ProjectPhoto, PrepareProgress, ProjectSource } from '~/types/project'

export interface RatedPhoto { relativePath: string; rating: number; capturedAt: number | null }

export interface ExportReport {
  processed: number
  skipped: number
  failed: number
  errors: string[]
}

export interface CreateProjectInput {
  name: string
  source: ProjectSource
  /** 直下に写真が無いフォルダを「まとめて 1 プロジェクトに」したときだけ true。 */
  flatten?: boolean
}

/** 作成画面のフォルダ一覧の 1 行。 */
export interface FolderEntry {
  name: string
  /** このフォルダを選んだときのキー（相対パス等）。 */
  key: string
  /** 直下に写真があるか。無ければ「中へ」でしか進めない（作成済みでない限り）。 */
  hasPhotos: boolean
  /** 下位フォルダを持つか。 */
  hasChildren: boolean
  /** 見本 1 枚（サムネイル URL）。まだ取れていなければ null。 */
  sample: string | null
}

export interface Backend {
  readonly environment: 'pc' | 'webFolder' | 'webPicker'

  // ---- プロジェクト一覧 ----
  listProjects(): Promise<Project[]>
  getProject(id: string): Promise<Project | null>
  createProject(input: CreateProjectInput): Promise<Project>
  renameProject(id: string, name: string): Promise<void>
  /** 消える容量（バイト）を先に見積もる。実際には消さない。 */
  estimateDeleteSize(id: string): Promise<number>
  deleteProject(id: string): Promise<void>
  /** 星・履歴・手直し・基準・時間を消す。顔ぶれ・指紋・絵は残す。 */
  restartProject(id: string): Promise<void>

  // ---- 設定 ----
  loadSettings(): Promise<AppSettings>
  saveSettings(settings: AppSettings): Promise<void>

  // ---- 出所を選ぶ ----
  /** OS のダイアログでフォルダを選ぶ（PC）／許可を求める（Web フォルダ）。 */
  pickFolder(devPath?: string): Promise<ProjectSource | null>
  /** 選んだフォルダの中身（作成画面の一覧・「中へ」）。 */
  listEntries(source: ProjectSource, subPath: string): Promise<FolderEntry[]>
  recentFolders(): Promise<ProjectSource[]>

  // ---- 走査・準備 ----
  /** scan → meta/hash → display を順に行う。50 枚ごと・中断で保存。 */
  prepare(projectId: string, onProgress: (progress: PrepareProgress) => void, signal: AbortSignal): Promise<void>

  // ---- 写真 ----
  listPhotos(projectId: string): Promise<ProjectPhoto[]>
  thumbnailUrl(projectId: string, relativePath: string): Promise<string | null>
  displayUrl(projectId: string, relativePath: string): Promise<string | null>
  originalUrl(projectId: string, relativePath: string): Promise<string | null>

  // ---- 選別の途中（core の Session） ----
  loadSession(projectId: string): Promise<Session | null>
  saveSession(projectId: string, session: Session): Promise<void>
  loadOverrides(projectId: string): Promise<PairOverride[]>
  saveOverrides(projectId: string, overrides: PairOverride[]): Promise<void>
  loadBurstDistance(projectId: string): Promise<number | null>
  saveBurstDistance(projectId: string, distance: number): Promise<void>
  /** 判断が変わった（星・手直し・基準・やり直し）ときに呼ぶ。サイドカーの書き時の判定に使う。 */
  markChanged(projectId: string): Promise<void>
  /** 「結果を見る」を選んだとき。ホームの完了状態に使う。 */
  markCompleted(projectId: string): Promise<void>

  // ---- サイドカー ----
  sidecarSupported(project: Project): boolean
  checkSidecar(project: Project): Promise<SidecarSync>
  /** 端末のものを書く。変更が無ければ何もしない（呼ぶ側が判断済みの前提でも、ここでもう一度確かめる）。 */
  pushSidecarIfChanged(project: Project): Promise<void>
  adoptSidecar(project: Project, sidecar: Sidecar): Promise<void>
  keepMineSidecar(project: Project, theirs: Sidecar): Promise<void>

  // ---- 取り出し ----
  exportFolders(projectId: string, photos: RatedPhoto[]): Promise<ExportReport>
  /** PC だけ。原本の XMP に星を書く。 */
  exportXmp(projectId: string, photos: RatedPhoto[]): Promise<ExportReport>
  exportCsv(projectId: string, photos: RatedPhoto[]): Promise<Blob>

  // ---- 保存量 ----
  storageUsageBytes(): Promise<number>
}
