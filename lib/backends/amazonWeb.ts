// Web（フォルダ・ピッカー）で Amazon Photos の共有リンクを読む（設計 08 章の縮小版）。
//
// 画像 CDN が CORS 非対応で、**写真をバイトとして読めない**（lib/amazonShare.ts の冒頭）。
// だから PC と違い、サムネイル・表示用画像を作らず、tempLink（`?viewBox=N` 付き）の
// URL をそのまま thumbnailUrl / displayUrl として返す（素の <img src> なら出せる）。
// その結果、次の 3 つが PC と違う（capabilities.amazonDisplayOnly）:
//   - 指紋（dHash）が無い → 連写を自動でまとめない（選別中の「この写真をまとめる」で手で）
//   - 取り出しは CSV だけ（ZIP・共有はバイトが要る）
//   - 端末に絵を置かないので、選別にも網が要る（CON-6 の例外）
//
// Web の Backend は 1 つだけ選ばれる（useBackend）ので、`withAmazonWeb` で包み、
// 出所が amazon のプロジェクトだけをここへ回す。保存の形（IndexedDB の鍵）は
// webFolder・webPicker と同じ `project:` `photos:` `session:` `overrides:` を使う。

import type {
  AmazonPreview, Backend, CreateProjectInput, ExportReport, FolderEntry, FolderSample, RatedPhoto
} from '~/lib/backend'
import type { PairOverride, Session, Sidecar, SidecarSync } from '~/lib/core'
import type {
  AppSettings, Project, ProjectPhoto, ProjectSource, PrepareProgress
} from '~/types/project'
import { idbDelete, idbGet, idbSet } from '~/lib/idb'
import { parseContentDate, parseKey, parseShareUrl, readShare, viewBoxUrl } from '~/lib/amazonShare'

/** サムネイルの長辺（08章 6「絵の 3 段」）。 */
const THUMB_EDGE = 160
const SAMPLE_LIMIT = 12
const ONLY_CSV = 'Web 版の Amazon Photos は CSV だけ書き出せます（ZIP は PC 版で使えます）。'

/** 1 枚ぶんの控え。**鍵は node id**（08章 2.5・落とし穴 4）。 */
interface AmazonEntry {
  name: string
  tempLink: string
}

type Entries = Record<string, AmazonEntry>

class AmazonWebProjects {
  private readonly entriesCache = new Map<string, Entries>()

  constructor(private readonly loadSettings: () => Promise<AppSettings>) {}

  private async entries(projectId: string): Promise<Entries> {
    const cached = this.entriesCache.get(projectId)
    if (cached) return cached
    const stored = (await idbGet<Entries>(`amazonEntries:${projectId}`)) ?? {}
    this.entriesCache.set(projectId, stored)
    return stored
  }

  async preview(shareUrl: string): Promise<AmazonPreview> {
    const source = parseShareUrl(shareUrl)
    if (!source) throw new Error('Amazon Photos の共有リンクの形ではありません。')
    const share = await readShare(source)
    const samples = share.photos
      .slice(0, SAMPLE_LIMIT)
      .flatMap(n => (n.tempLink ? [viewBoxUrl(n.tempLink, THUMB_EDGE)] : []))
    return { key: share.key, name: share.name, count: share.photos.length, samples }
  }

  async create(input: CreateProjectInput): Promise<Project> {
    const id = crypto.randomUUID()
    const now = Date.now()
    const project: Project = {
      id,
      name: input.name,
      source: input.source,
      photoCount: 0,
      createdAt: now,
      updatedAt: now,
      scannedCount: 0,
      metaHashedCount: 0,
      displayedCount: 0,
      prepareWarning: null,
      burstDistance: null,
      completedAt: null
    }
    await idbSet(`project:${id}`, project)
    return project
  }

  /** 一覧を読むだけ（絵は取りに行かない）。撮影時刻は一覧に入っている（08章 7）。 */
  async prepare(projectId: string, onProgress: (p: PrepareProgress) => void, signal: AbortSignal): Promise<void> {
    const project = await idbGet<Project>(`project:${projectId}`)
    if (!project) throw new Error('プロジェクトが見つかりません。')
    const source = parseKey(project.source.key)
    if (!source) throw new Error('出所の形が正しくありません。')
    onProgress({ task: 'scan', done: 0, total: 0, warning: null })
    let share
    try {
      share = await readShare(source)
    } catch (cause) {
      project.prepareWarning = cause instanceof Error ? cause.message : 'Amazon から読めませんでした。'
      project.updatedAt = Date.now()
      await idbSet(`project:${projectId}`, project)
      throw cause
    }
    if (signal.aborted) return

    const entries: Entries = {}
    const photos: ProjectPhoto[] = []
    for (const node of share.photos) {
      if (!node.tempLink) continue
      entries[node.id] = { name: node.name, tempLink: node.tempLink }
      photos.push({
        relativePath: node.id, // 08章 5.2: relativePath は nodeId（星・連写の鍵）
        capturedAt: parseContentDate(node.contentProperties?.contentDate),
        dHash: null,
        dHashVersion: 0,
        size: node.contentProperties?.size ?? 0,
        mtimeMs: 0,
        // 絵は端末に置かないが、URL はいつでも組み立てられる（＝出せる）。
        hasThumbnail: true,
        hasDisplay: true
      })
    }
    await idbSet(`amazonEntries:${projectId}`, entries)
    this.entriesCache.set(projectId, entries)
    await idbSet(`photos:${projectId}`, photos)
    project.photoCount = photos.length
    project.scannedCount = photos.length
    project.metaHashedCount = photos.length
    project.displayedCount = photos.length
    project.prepareWarning = null
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
    onProgress({ task: 'scan', done: photos.length, total: photos.length, warning: null })
    onProgress({ task: 'display', done: photos.length, total: photos.length, warning: null })
  }

  async thumbnailUrl(projectId: string, relativePath: string): Promise<string | null> {
    const entry = (await this.entries(projectId))[relativePath]
    return entry ? viewBoxUrl(entry.tempLink, THUMB_EDGE) : null
  }

  async displayUrl(projectId: string, relativePath: string): Promise<string | null> {
    const entry = (await this.entries(projectId))[relativePath]
    if (!entry) return null
    const { displayEdge } = await this.loadSettings()
    return viewBoxUrl(entry.tempLink, displayEdge)
  }

  /** 拡大は原本（08章 6）。ブラウザが <img> で読むだけで、端末には置かない。 */
  async originalUrl(projectId: string, relativePath: string): Promise<string | null> {
    return (await this.entries(projectId))[relativePath]?.tempLink ?? null
  }

  /** ホームのカード。Amazon は端末に絵が無いので、1 枚だけ網から出す（「網へ行かない」の例外）。 */
  async coverUrl(projectId: string): Promise<string | null> {
    const first = Object.keys(await this.entries(projectId))[0]
    return first ? this.thumbnailUrl(projectId, first) : null
  }

  async estimateDeleteSize(projectId: string): Promise<number> {
    // 絵を置いていないので、消えるのは一覧の控え（1 枚あたり数百バイト）だけ。
    return Object.keys(await this.entries(projectId)).length * 300
  }

  async delete(projectId: string): Promise<void> {
    this.entriesCache.delete(projectId)
    await idbDelete(`project:${projectId}`)
    await idbDelete(`photos:${projectId}`)
    await idbDelete(`amazonEntries:${projectId}`)
    await idbDelete(`session:${projectId}`)
    await idbDelete(`overrides:${projectId}`)
  }

  async restart(projectId: string): Promise<void> {
    await idbDelete(`session:${projectId}`)
    await idbDelete(`overrides:${projectId}`)
    const project = await idbGet<Project>(`project:${projectId}`)
    if (!project) return
    project.burstDistance = null
    project.completedAt = null
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
  }

  /** 先頭 3 列は他の出所と同じ。relativePath が node id で人には読めないので、名前を 4 列目に足す。 */
  async exportCsv(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    const entries = await this.entries(projectId)
    const rows = ['relative_path,rating,captured_at,name']
    for (const photo of photos) {
      const name = entries[photo.relativePath]?.name ?? ''
      rows.push(`${JSON.stringify(photo.relativePath)},${photo.rating ?? 0},${photo.capturedAt ?? ''},${JSON.stringify(name)}`)
    }
    return new Blob([rows.join('\n')], { type: 'text/csv' })
  }
}

/** 出所が amazon のプロジェクトだけを AmazonWebProjects へ回す。それ以外は包んだ Backend のまま。 */
class AmazonAwareBackend implements Backend {
  private readonly amazon: AmazonWebProjects
  private readonly kindCache = new Map<string, boolean>()

  constructor(private readonly base: Backend) {
    this.amazon = new AmazonWebProjects(() => base.loadSettings())
  }

  get environment() {
    return this.base.environment
  }

  private async isAmazon(projectId: string): Promise<boolean> {
    const cached = this.kindCache.get(projectId)
    if (cached !== undefined) return cached
    const project = await idbGet<Project>(`project:${projectId}`)
    const amazon = project?.source.kind === 'amazon'
    if (project) this.kindCache.set(projectId, amazon)
    return amazon
  }

  // ---- プロジェクト一覧（鍵が同じなので包んだ側で読める） ----
  listProjects(): Promise<Project[]> { return this.base.listProjects() }
  getProject(id: string): Promise<Project | null> { return this.base.getProject(id) }
  async createProject(input: CreateProjectInput): Promise<Project> {
    return input.source.kind === 'amazon' ? this.amazon.create(input) : this.base.createProject(input)
  }
  get importPhotos() { return this.base.importPhotos?.bind(this.base) }
  renameProject(id: string, name: string): Promise<void> { return this.base.renameProject(id, name) }
  async estimateDeleteSize(id: string): Promise<number> {
    return (await this.isAmazon(id)) ? this.amazon.estimateDeleteSize(id) : this.base.estimateDeleteSize(id)
  }
  async deleteProject(id: string): Promise<void> {
    const amazon = await this.isAmazon(id)
    this.kindCache.delete(id)
    return amazon ? this.amazon.delete(id) : this.base.deleteProject(id)
  }
  async restartProject(id: string): Promise<void> {
    return (await this.isAmazon(id)) ? this.amazon.restart(id) : this.base.restartProject(id)
  }

  // ---- 設定 ----
  loadSettings(): Promise<AppSettings> { return this.base.loadSettings() }
  saveSettings(settings: AppSettings): Promise<void> { return this.base.saveSettings(settings) }

  // ---- 出所を選ぶ ----
  pickFolder(devPath?: string): Promise<ProjectSource | null> { return this.base.pickFolder(devPath) }
  listEntries(source: ProjectSource, subPath: string): Promise<FolderEntry[]> { return this.base.listEntries(source, subPath) }
  recentFolders(): Promise<ProjectSource[]> { return this.base.recentFolders() }
  sampleFolder(source: ProjectSource, subPath: string, limit: number): Promise<FolderSample> {
    return this.base.sampleFolder(source, subPath, limit)
  }
  amazonPreview(shareUrl: string): Promise<AmazonPreview> { return this.amazon.preview(shareUrl) }

  // ---- 走査・準備 ----
  async prepare(projectId: string, onProgress: (p: PrepareProgress) => void, signal: AbortSignal): Promise<void> {
    return (await this.isAmazon(projectId))
      ? this.amazon.prepare(projectId, onProgress, signal)
      : this.base.prepare(projectId, onProgress, signal)
  }

  // ---- 写真 ----
  listPhotos(projectId: string): Promise<ProjectPhoto[]> { return this.base.listPhotos(projectId) }
  async thumbnailUrl(projectId: string, relativePath: string): Promise<string | null> {
    return (await this.isAmazon(projectId)) ? this.amazon.thumbnailUrl(projectId, relativePath) : this.base.thumbnailUrl(projectId, relativePath)
  }
  async displayUrl(projectId: string, relativePath: string): Promise<string | null> {
    return (await this.isAmazon(projectId)) ? this.amazon.displayUrl(projectId, relativePath) : this.base.displayUrl(projectId, relativePath)
  }
  async originalUrl(projectId: string, relativePath: string): Promise<string | null> {
    return (await this.isAmazon(projectId)) ? this.amazon.originalUrl(projectId, relativePath) : this.base.originalUrl(projectId, relativePath)
  }
  async coverUrl(projectId: string): Promise<string | null> {
    return (await this.isAmazon(projectId)) ? this.amazon.coverUrl(projectId) : this.base.coverUrl(projectId)
  }

  // ---- 選別の途中 ----
  // Amazon はサイドカーを持たない（08章 1）ので、変更の印（sidecarState）は付けない。
  loadSession(projectId: string): Promise<Session | null> { return this.base.loadSession(projectId) }
  async saveSession(projectId: string, session: Session): Promise<void> {
    if (await this.isAmazon(projectId)) await idbSet(`session:${projectId}`, session)
    else await this.base.saveSession(projectId, session)
  }
  loadOverrides(projectId: string): Promise<PairOverride[]> { return this.base.loadOverrides(projectId) }
  async saveOverrides(projectId: string, overrides: PairOverride[]): Promise<void> {
    if (await this.isAmazon(projectId)) await idbSet(`overrides:${projectId}`, overrides)
    else await this.base.saveOverrides(projectId, overrides)
  }
  loadBurstDistance(projectId: string): Promise<number | null> { return this.base.loadBurstDistance(projectId) }
  async saveBurstDistance(projectId: string, distance: number): Promise<void> {
    if (!(await this.isAmazon(projectId))) return this.base.saveBurstDistance(projectId, distance)
    const project = await idbGet<Project>(`project:${projectId}`)
    if (!project) return
    project.burstDistance = distance
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
  }
  async markChanged(projectId: string): Promise<void> {
    if (!(await this.isAmazon(projectId))) await this.base.markChanged(projectId)
  }
  markCompleted(projectId: string): Promise<void> { return this.base.markCompleted(projectId) }

  // ---- サイドカー（Amazon は無し） ----
  sidecarSupported(project: Project): boolean {
    return project.source.kind !== 'amazon' && this.base.sidecarSupported(project)
  }
  async checkSidecar(project: Project): Promise<SidecarSync> {
    return project.source.kind === 'amazon' ? 'Settled' : this.base.checkSidecar(project)
  }
  async pushSidecarIfChanged(project: Project): Promise<void> {
    if (project.source.kind !== 'amazon') await this.base.pushSidecarIfChanged(project)
  }
  async adoptSidecar(project: Project, sidecar: Sidecar): Promise<void> {
    if (project.source.kind !== 'amazon') await this.base.adoptSidecar(project, sidecar)
  }
  async keepMineSidecar(project: Project, theirs: Sidecar): Promise<void> {
    if (project.source.kind !== 'amazon') await this.base.keepMineSidecar(project, theirs)
  }

  // ---- 取り出し（Amazon は CSV だけ） ----
  async exportFolders(projectId: string, photos: RatedPhoto[]): Promise<ExportReport> {
    if (await this.isAmazon(projectId)) return { processed: 0, skipped: 0, failed: 0, errors: [ONLY_CSV] }
    return this.base.exportFolders(projectId, photos)
  }
  async exportXmp(projectId: string, photos: RatedPhoto[]): Promise<ExportReport> {
    if (await this.isAmazon(projectId)) return { processed: 0, skipped: 0, failed: 0, errors: [ONLY_CSV] }
    return this.base.exportXmp(projectId, photos)
  }
  async exportCsv(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    return (await this.isAmazon(projectId)) ? this.amazon.exportCsv(projectId, photos) : this.base.exportCsv(projectId, photos)
  }
  async exportZip(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    if (await this.isAmazon(projectId)) throw new Error(ONLY_CSV)
    return this.base.exportZip(projectId, photos)
  }
  async shareTargets(projectId: string, photos: RatedPhoto[]): Promise<File[]> {
    if (await this.isAmazon(projectId)) return []
    return this.base.shareTargets(projectId, photos)
  }

  // ---- 保存量 ----
  storageUsageBytes(): Promise<number> { return this.base.storageUsageBytes() }
}

export function withAmazonWeb(base: Backend): Backend {
  return new AmazonAwareBackend(base)
}
