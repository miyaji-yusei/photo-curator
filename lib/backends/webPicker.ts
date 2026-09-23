// Web（ピッカー）＝ iPad の Backend（設計 07 章 段6）。
//
// フォルダは選べない（`browseFolders: false`）。**写真を選ぶ**と、その場で
// 取り込む（解析して thumb・display・dHash・撮影時刻まで作る。原本は置かない。
// 02章「絵の3段階」の通り、ピッカーは拡大でも表示用画像どまり）。
// 出所という概念が無いので `source.key` はプロジェクト id をそのまま使う。
//
// **判断は持たない。** 星・連写のまとめ・選別の状態遷移は core（lib/core.ts）。

import type {
  Backend, CreateProjectInput, ExportReport, FolderEntry, FolderSample, RatedPhoto
} from '~/lib/backend'
import type { PairOverride, Session, Sidecar, SidecarSync } from '~/lib/core'
import type {
  AppSettings, Project, ProjectPhoto, ProjectSource, PrepareProgress
} from '~/types/project'
import { DEFAULT_SETTINGS } from '~/types/project'
import { analyzePhotoFile } from '~/utils/analyzePhoto'
import { idbDelete, idbGet, idbKeysWithPrefix, idbSet } from '~/lib/idb'
import { ObjectUrlCache } from '~/utils/objectUrlCache'
import { createStoredZip } from '~/utils/zip'
import { zipEntriesByRating } from '~/utils/shareExport'

interface StoredProject extends Project {}

export class WebPickerBackend implements Backend {
  readonly environment = 'webPicker' as const
  private readonly thumbCache = new ObjectUrlCache(200)
  private readonly displayCache = new ObjectUrlCache(120)

  // ---- プロジェクト一覧 ----

  async listProjects(): Promise<Project[]> {
    const keys = await idbKeysWithPrefix('project:')
    const projects: Project[] = []
    for (const key of keys) {
      const project = await idbGet<StoredProject>(key)
      if (project) projects.push(project)
    }
    return projects.sort((a, b) => b.updatedAt - a.updatedAt)
  }

  async getProject(id: string): Promise<Project | null> {
    return idbGet<StoredProject>(`project:${id}`)
  }

  /** 02章の作成画面には無い経路（フォルダが無い）。呼ばれない想定。 */
  async createProject(_input: CreateProjectInput): Promise<Project> {
    throw new Error('Web 版（写真を選ぶ）では使いません。importPhotos を使ってください。')
  }

  /**
   * 写真を選んだその場で取り込む。**作成と準備が1つになる**（フォルダが無いので
   * 「フォルダを選ぶ→走査する」の段が無く、選んだ瞬間に全部揃う）。
   */
  async importPhotos(name: string, files: File[]): Promise<Project> {
    const id = crypto.randomUUID()
    const now = Date.now()
    const source: ProjectSource = { kind: 'folder', key: id, label: name }
    const photos: ProjectPhoto[] = []
    let displayedCount = 0
    let failed = 0
    const settings = await this.loadSettings()

    for (const file of files) {
      const relativePath = file.name
      try {
        const analyzed = await analyzePhotoFile(file, settings.displayEdge)
        if (analyzed.thumbnail) await idbSet(`thumb:${id}:${relativePath}`, analyzed.thumbnail)
        if (analyzed.display) await idbSet(`display:${id}:${relativePath}`, analyzed.display)
        const hasDisplay = !!analyzed.display
        if (hasDisplay) displayedCount += 1
        if (analyzed.error) failed += 1
        photos.push({
          relativePath,
          capturedAt: analyzed.capturedAt,
          dHash: analyzed.dHash,
          dHashVersion: analyzed.dHash ? 2 : 0,
          size: file.size,
          mtimeMs: Number.isFinite(file.lastModified) ? file.lastModified : now,
          hasThumbnail: !!analyzed.thumbnail,
          hasDisplay
        })
      } catch {
        failed += 1
      }
    }
    photos.sort((a, b) => a.relativePath.localeCompare(b.relativePath))
    await idbSet(`photos:${id}`, photos)

    const project: StoredProject = {
      id,
      name,
      source,
      photoCount: photos.length,
      createdAt: now,
      updatedAt: now,
      scannedCount: photos.length,
      metaHashedCount: photos.length,
      displayedCount,
      prepareWarning: failed > 0 ? `${failed} 枚を読めませんでした` : null,
      burstDistance: null,
      completedAt: null
    }
    await idbSet(`project:${id}`, project)
    return project
  }

  async renameProject(id: string, name: string): Promise<void> {
    const project = await idbGet<StoredProject>(`project:${id}`)
    if (!project) return
    project.name = name
    project.updatedAt = Date.now()
    await idbSet(`project:${id}`, project)
  }

  async estimateDeleteSize(id: string): Promise<number> {
    const photos = await this.listPhotos(id)
    return photos.length * (80 * 1024 + 10 * 1024)
  }

  async deleteProject(id: string): Promise<void> {
    const photos = await this.listPhotos(id).catch(() => [])
    for (const photo of photos) {
      await idbDelete(`thumb:${id}:${photo.relativePath}`)
      await idbDelete(`display:${id}:${photo.relativePath}`)
    }
    await idbDelete(`project:${id}`)
    await idbDelete(`photos:${id}`)
    await idbDelete(`session:${id}`)
    await idbDelete(`overrides:${id}`)
  }

  async restartProject(id: string): Promise<void> {
    await idbDelete(`session:${id}`)
    await idbDelete(`overrides:${id}`)
    const project = await idbGet<StoredProject>(`project:${id}`)
    if (project) {
      project.burstDistance = null
      project.completedAt = null
      project.updatedAt = Date.now()
      await idbSet(`project:${id}`, project)
    }
  }

  // ---- 設定 ----

  async loadSettings(): Promise<AppSettings> {
    return (await idbGet<AppSettings>('settings')) ?? { ...DEFAULT_SETTINGS }
  }

  async saveSettings(settings: AppSettings): Promise<void> {
    await idbSet('settings', settings)
  }

  // ---- 出所を選ぶ ----
  // フォルダという概念が無い（capabilities.browseFolders = false）。
  // create.vue はこの3つを呼ばず、importPhotos を直接使う。

  async pickFolder(): Promise<ProjectSource | null> {
    return null
  }

  async listEntries(_source: ProjectSource, _subPath: string): Promise<FolderEntry[]> {
    return []
  }

  async recentFolders(): Promise<ProjectSource[]> {
    return []
  }

  async sampleFolder(_source: ProjectSource, _subPath: string, _limit: number): Promise<FolderSample> {
    return { count: 0, samples: [] }
  }

  // ---- 走査・準備 ----
  // importPhotos の時点で終わっている。呼ばれても即完了扱いにする。

  async prepare(projectId: string, onProgress: (p: PrepareProgress) => void): Promise<void> {
    const photos = await this.listPhotos(projectId)
    onProgress({ task: 'display', done: photos.length, total: photos.length, warning: null })
  }

  // ---- 写真 ----

  async listPhotos(projectId: string): Promise<ProjectPhoto[]> {
    return (await idbGet<ProjectPhoto[]>(`photos:${projectId}`)) ?? []
  }

  async thumbnailUrl(projectId: string, relativePath: string): Promise<string | null> {
    const key = `${projectId}:${relativePath}`
    const cached = this.thumbCache.peek(key)
    if (cached) return cached
    const blob = await idbGet<Blob>(`thumb:${projectId}:${relativePath}`)
    if (!blob) return null
    return this.thumbCache.get(key, blob)
  }

  async displayUrl(projectId: string, relativePath: string): Promise<string | null> {
    const key = `${projectId}:${relativePath}`
    const cached = this.displayCache.peek(key)
    if (cached) return cached
    const blob = await idbGet<Blob>(`display:${projectId}:${relativePath}`)
    if (!blob) return null
    return this.displayCache.get(key, blob)
  }

  /** 原本は置かない（02章「絵の3段階」）。拡大は表示用画像どまりで、`ZoomView` が
   * 「原本を読めません・表示用画像で表示中」を出す。 */
  async originalUrl(_projectId: string, _relativePath: string): Promise<string | null> {
    return null
  }

  async coverUrl(projectId: string): Promise<string | null> {
    const photos = await this.listPhotos(projectId)
    const first = photos.find(p => p.hasThumbnail)
    if (!first) return null
    return this.thumbnailUrl(projectId, first.relativePath)
  }

  // ---- 選別の途中 ----

  async loadSession(projectId: string): Promise<Session | null> {
    return idbGet<Session>(`session:${projectId}`)
  }

  async saveSession(projectId: string, session: Session): Promise<void> {
    await idbSet(`session:${projectId}`, session)
  }

  async loadOverrides(projectId: string): Promise<PairOverride[]> {
    return (await idbGet<PairOverride[]>(`overrides:${projectId}`)) ?? []
  }

  async saveOverrides(projectId: string, overrides: PairOverride[]): Promise<void> {
    await idbSet(`overrides:${projectId}`, overrides)
  }

  async loadBurstDistance(projectId: string): Promise<number | null> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    return project?.burstDistance ?? null
  }

  async saveBurstDistance(projectId: string, distance: number): Promise<void> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    if (!project) return
    project.burstDistance = distance
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
  }

  async markChanged(_projectId: string): Promise<void> {
    // サイドカーが無い（capabilities.sidecar = false）。書く先が無いので何もしない。
  }

  async markCompleted(projectId: string): Promise<void> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    if (!project) return
    project.completedAt = Date.now()
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
  }

  // ---- サイドカー ----
  // ピッカーはフォルダを持たないので書き先が無い（05章の表どおり全部 false/no-op）。

  sidecarSupported(_project: Project): boolean {
    return false
  }
  async checkSidecar(_project: Project): Promise<SidecarSync> {
    return 'Settled'
  }
  async pushSidecarIfChanged(_project: Project): Promise<void> {}
  async adoptSidecar(_project: Project, _sidecar: Sidecar): Promise<void> {}
  async keepMineSidecar(_project: Project, _theirs: Sidecar): Promise<void> {}

  // ---- 取り出し ----
  // PC 版のフォルダ分け・XMP は無い。**共有シートと ZIP**（設計02章の差分表）。

  async exportFolders(_projectId: string, _photos: RatedPhoto[]): Promise<ExportReport> {
    return { processed: 0, skipped: 0, failed: 0, errors: ['Web 版はフォルダ分けに対応していません（共有シートか ZIP をお使いください）。'] }
  }

  async exportXmp(_projectId: string, _photos: RatedPhoto[]): Promise<ExportReport> {
    return { processed: 0, skipped: 0, failed: 0, errors: ['Web 版は XMP の書き込みに対応していません（PC 版で使えます）。'] }
  }

  async exportCsv(_projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    const rows = ['relative_path,rating,captured_at']
    for (const photo of photos) {
      rows.push(`${JSON.stringify(photo.relativePath)},${photo.rating ?? 0},${photo.capturedAt ?? ''}`)
    }
    return new Blob([rows.join('\n')], { type: 'text/csv' })
  }

  /** ZIP（`star-N/` に分ける。02章の差分表）。表示用画像しか無いので、それを詰める。 */
  async exportZip(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    const candidates = []
    for (const photo of photos) {
      const blob = await idbGet<Blob>(`display:${projectId}:${photo.relativePath}`)
      if (blob) candidates.push({ name: photo.relativePath, rating: photo.rating, blob, modifiedAt: photo.capturedAt ?? undefined })
    }
    return createStoredZip(zipEntriesByRating(candidates))
  }

  /** 共有シート（Web Share API）。表示用画像を渡す。 */
  async shareTargets(projectId: string, photos: RatedPhoto[]): Promise<File[]> {
    const files: File[] = []
    for (const photo of photos) {
      const blob = await idbGet<Blob>(`display:${projectId}:${photo.relativePath}`)
      if (blob) files.push(new File([blob], photo.relativePath, { type: blob.type || 'image/jpeg' }))
    }
    return files
  }

  // ---- 保存量 ----

  async storageUsageBytes(): Promise<number> {
    if (typeof navigator !== 'undefined' && navigator.storage?.estimate) {
      const estimate = await navigator.storage.estimate()
      return estimate.usage ?? 0
    }
    return 0
  }
}
