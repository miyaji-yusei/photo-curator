// Web（フォルダ）の Backend。File System Access API（本番）と、
// 開発用の HTTP 配信（`pnpm dev` のときだけ。server/api/dev-folder/）の
// 2 通りで同じフォルダを読む（設計 07 章 段2-3）。
//
// **判断は持たない。** 星・連写のまとめ・選別の状態遷移は core（lib/core.ts）。

import type {
  Backend, CreateProjectInput, ExportReport, FolderEntry, RatedPhoto
} from '~/lib/backend'
import type { PairOverride, Session, Sidecar, SidecarSync } from '~/lib/core'
import * as core from '~/lib/core'
import type {
  AppSettings, Project, ProjectPhoto, ProjectSource, PrepareProgress
} from '~/types/project'
import { DEFAULT_SETTINGS } from '~/types/project'
import { analyzePhotoFile } from '~/utils/analyzePhoto'
import { idbDelete, idbGet, idbKeysWithPrefix, idbSet } from '~/lib/idb'
import { ObjectUrlCache } from '~/utils/objectUrlCache'

// ---------------------------------------------------------------------------
// FolderIO: 実フォルダへの読み書き。native（handle）と dev（HTTP）の 2 実装。
// ---------------------------------------------------------------------------

interface RawEntry {
  name: string
  isDirectory: boolean
  size: number
  mtimeMs: number
}

interface FolderIO {
  list(subPath: string): Promise<RawEntry[]>
  readFile(subPath: string, name: string, mtimeMs: number): Promise<File>
  readText(subPath: string): Promise<string | null>
  /** 書けたら true。書けない（許可が無い・dev モード）なら false。 */
  writeText(subPath: string, content: string): Promise<boolean>
  canWrite(): Promise<boolean>
}

function joinPath(base: string, name: string): string {
  return base ? `${base}/${name}` : name
}

/** 開発用。`server/api/dev-folder/` を叩くだけ。**本番には出てこない**
 *（作る場所が `pickFolder(devPath)` を通ったときだけなので）。 */
class DevHttpFolderIO implements FolderIO {
  constructor(private readonly root: string) {}

  async list(subPath: string): Promise<RawEntry[]> {
    const url = `/api/dev-folder/list?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(subPath)}`
    const response = await fetch(url)
    if (!response.ok) throw new Error(`一覧を読めませんでした（${response.status}）`)
    return response.json()
  }

  async readFile(subPath: string, name: string, mtimeMs: number): Promise<File> {
    const full = joinPath(subPath, name)
    const url = `/api/dev-folder/file?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(full)}`
    const response = await fetch(url)
    if (!response.ok) throw new Error(`読めませんでした（${response.status}）`)
    const blob = await response.blob()
    return new File([blob], name, { lastModified: mtimeMs, type: blob.type })
  }

  async readText(subPath: string): Promise<string | null> {
    const url = `/api/dev-folder/file?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(subPath)}`
    const response = await fetch(url)
    if (!response.ok) return null
    return response.text()
  }

  async writeText(): Promise<boolean> {
    // 開発用の配信は読むだけ。サイドカーは読み取り専用として振る舞う。
    return false
  }

  async canWrite(): Promise<boolean> {
    return false
  }
}

/** 本番。File System Access API。 */
class HandleFolderIO implements FolderIO {
  constructor(private readonly root: FileSystemDirectoryHandle) {}

  private async resolveDir(subPath: string, create: boolean): Promise<FileSystemDirectoryHandle> {
    let dir = this.root
    if (!subPath) return dir
    for (const part of subPath.split('/').filter(Boolean)) {
      dir = await dir.getDirectoryHandle(part, { create })
    }
    return dir
  }

  async list(subPath: string): Promise<RawEntry[]> {
    const dir = await this.resolveDir(subPath, false)
    const result: RawEntry[] = []
    // @ts-expect-error -- entries() は非同期イテレータ（型定義がまだ薄い場合がある）
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind === 'directory') {
        result.push({ name, isDirectory: true, size: 0, mtimeMs: 0 })
      } else {
        const file = await (handle as FileSystemFileHandle).getFile()
        result.push({ name, isDirectory: false, size: file.size, mtimeMs: file.lastModified })
      }
    }
    return result
  }

  async readFile(subPath: string, name: string): Promise<File> {
    const dir = await this.resolveDir(subPath, false)
    const handle = await dir.getFileHandle(name)
    return handle.getFile()
  }

  async readText(subPath: string): Promise<string | null> {
    try {
      const parts = subPath.split('/')
      const name = parts.pop() ?? ''
      const dir = await this.resolveDir(parts.join('/'), false)
      const handle = await dir.getFileHandle(name)
      const file = await handle.getFile()
      return await file.text()
    } catch {
      return null
    }
  }

  async writeText(subPath: string, content: string): Promise<boolean> {
    try {
      if (!(await this.canWrite())) return false
      const parts = subPath.split('/')
      const name = parts.pop() ?? ''
      const dir = await this.resolveDir(parts.join('/'), true)
      const handle = await dir.getFileHandle(name, { create: true })
      const writable = await handle.createWritable()
      await writable.write(content)
      await writable.close()
      return true
    } catch {
      return false
    }
  }

  async canWrite(): Promise<boolean> {
    try {
      // @ts-expect-error -- queryPermission は File System Access API 固有
      const state = await this.root.queryPermission?.({ mode: 'readwrite' })
      if (state === 'granted') return true
      // @ts-expect-error -- requestPermission も同様
      const requested = await this.root.requestPermission?.({ mode: 'readwrite' })
      return requested === 'granted'
    } catch {
      return false
    }
  }
}

// ---------------------------------------------------------------------------
// 保存の形（IndexedDB）
// ---------------------------------------------------------------------------

interface StoredProject extends Project {
  /** 'dev:<絶対パス>' または 'handle:<id>'。 */
  ioKey: string
}

interface SidecarState {
  seenAt: number
  seenBy: string
  localChanged: boolean
}

const VIDEO_EXTENSIONS = new Set(['.mp4', '.mov', '.avi', '.mkv', '.m4v', '.webm'])

function isVideoName(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return VIDEO_EXTENSIONS.has(name.slice(dot).toLowerCase())
}

async function deviceId(): Promise<string> {
  const existing = await idbGet<string>('deviceId')
  if (existing) return existing
  const id = crypto.randomUUID()
  await idbSet('deviceId', id)
  return id
}

function deviceName(): string {
  if (typeof navigator === 'undefined') return 'このブラウザ'
  const ua = navigator.userAgent
  if (/windows/i.test(ua)) return 'Windows PC'
  if (/mac/i.test(ua)) return 'Mac'
  return 'このブラウザ'
}

export class WebFolderBackend implements Backend {
  readonly environment = 'webFolder' as const
  private readonly thumbCache = new ObjectUrlCache(200)
  private readonly displayCache = new ObjectUrlCache(120)

  private async ioFor(project: Project): Promise<FolderIO> {
    const stored = project as StoredProject
    const ioKey = stored.ioKey ?? project.source.key
    if (ioKey.startsWith('dev:')) return new DevHttpFolderIO(ioKey.slice(4))
    if (ioKey.startsWith('handle:')) {
      const handle = await idbGet<FileSystemDirectoryHandle>(ioKey)
      if (!handle) throw new Error('フォルダの handle を見つけられませんでした。もう一度選んでください。')
      return new HandleFolderIO(handle)
    }
    throw new Error('出所の形が分かりません。')
  }

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

  async createProject(input: CreateProjectInput): Promise<Project> {
    const id = crypto.randomUUID()
    const now = Date.now()
    const project: StoredProject = {
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
      completedAt: null,
      ioKey: input.source.key
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
    // 表示用画像・サムネイルの概算（実測値。設計 02 章）。
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
    await idbDelete(`sidecarState:${id}`)
  }

  async restartProject(id: string): Promise<void> {
    await idbDelete(`session:${id}`)
    await idbDelete(`overrides:${id}`)
    await idbDelete(`sidecarState:${id}`)
    const project = await idbGet<StoredProject>(`project:${id}`)
    if (project) {
      project.burstDistance = null
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

  async pickFolder(devPath?: string): Promise<ProjectSource | null> {
    if (devPath) {
      const name = devPath.split(/[\\/]/).filter(Boolean).pop() ?? devPath
      return { kind: 'folder', key: `dev:${devPath}`, label: name }
    }
    if (typeof window === 'undefined' || !('showDirectoryPicker' in window)) return null
    try {
      // @ts-expect-error -- showDirectoryPicker はまだ TS 標準 lib に無い
      const handle: FileSystemDirectoryHandle = await window.showDirectoryPicker({ mode: 'readwrite' })
      const id = crypto.randomUUID()
      await idbSet(`handle:${id}`, handle)
      return { kind: 'folder', key: `handle:${id}`, label: handle.name }
    } catch {
      return null
    }
  }

  async listEntries(source: ProjectSource, subPath: string): Promise<FolderEntry[]> {
    const io = source.key.startsWith('dev:')
      ? new DevHttpFolderIO(source.key.slice(4))
      : new HandleFolderIO((await idbGet<FileSystemDirectoryHandle>(source.key))!)
    const raw = await io.list(subPath)
    const entries: FolderEntry[] = []
    for (const item of raw) {
      if (item.isDirectory) {
        const children = await io.list(joinPath(subPath, item.name)).catch(() => [])
        const hasPhotos = children.some(c => !c.isDirectory && !isVideoName(c.name))
        const hasChildren = children.some(c => c.isDirectory)
        if (!hasPhotos && !hasChildren) continue // 空フォルダは出さない
        entries.push({ name: item.name, key: joinPath(subPath, item.name), hasPhotos, hasChildren, sample: null })
      }
    }
    return entries
  }

  async recentFolders(): Promise<ProjectSource[]> {
    return (await idbGet<ProjectSource[]>('recentFolders')) ?? []
  }

  // ---- 走査・準備 ----

  async prepare(projectId: string, onProgress: (p: PrepareProgress) => void, signal: AbortSignal): Promise<void> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    if (!project) throw new Error('プロジェクトが見つかりません。')
    const io = await this.ioFor(project)
    await core.init()

    // scan: 再帰的に写真を集める。
    const found: { subPath: string; entry: RawEntry }[] = []
    const walk = async (subPath: string, depth: number) => {
      if (depth > 6) return
      const entries = await io.list(subPath)
      for (const entry of entries) {
        if (signal.aborted) return
        if (entry.isDirectory) {
          await walk(joinPath(subPath, entry.name), depth + 1)
        } else if (!isVideoName(entry.name)) {
          found.push({ subPath, entry })
        }
      }
    }
    await walk('', 0)
    if (signal.aborted) return

    const existing = (await idbGet<ProjectPhoto[]>(`photos:${projectId}`)) ?? []
    const byPath = new Map(existing.map(p => [p.relativePath, p]))
    const photos: ProjectPhoto[] = found.map(({ subPath, entry }) => {
      const relativePath = joinPath(subPath, entry.name)
      const prior = byPath.get(relativePath)
      const changed = !prior || prior.size !== entry.size || prior.mtimeMs !== entry.mtimeMs
      return changed
        ? {
            relativePath, capturedAt: null, dHash: null, dHashVersion: 0,
            size: entry.size, mtimeMs: entry.mtimeMs, hasThumbnail: false, hasDisplay: false
          }
        : prior
    })
    await idbSet(`photos:${projectId}`, photos)
    project.photoCount = photos.length
    project.scannedCount = photos.length
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
    onProgress({ task: 'scan', done: photos.length, total: photos.length, warning: null })

    // meta/hash/display: まだのものだけ。50 枚ごとに保存。
    const settings = await this.loadSettings()
    let metaDone = photos.filter(p => p.capturedAt !== null || p.dHash !== null).length
    let displayDone = photos.filter(p => p.hasDisplay).length
    let sinceSave = 0
    let failed = 0

    for (let index = 0; index < photos.length; index += 1) {
      if (signal.aborted) break
      const photo = photos[index]!
      if (photo.hasThumbnail && photo.hasDisplay) continue
      try {
        const file = await io.readFile(
          photo.relativePath.includes('/') ? photo.relativePath.slice(0, photo.relativePath.lastIndexOf('/')) : '',
          photo.relativePath.includes('/') ? photo.relativePath.slice(photo.relativePath.lastIndexOf('/') + 1) : photo.relativePath,
          photo.mtimeMs
        )
        const analyzed = await analyzePhotoFile(file, settings.displayEdge)
        if (analyzed.error) {
          failed += 1
        } else {
          photos[index] = {
            ...photo,
            capturedAt: analyzed.capturedAt,
            dHash: analyzed.dHash,
            dHashVersion: 2,
            hasThumbnail: !!analyzed.thumbnail,
            hasDisplay: !!analyzed.display
          }
          if (analyzed.thumbnail) await idbSet(`thumb:${projectId}:${photo.relativePath}`, analyzed.thumbnail)
          if (analyzed.display) await idbSet(`display:${projectId}:${photo.relativePath}`, analyzed.display)
          metaDone += 1
          displayDone += 1
        }
      } catch {
        failed += 1
      }
      sinceSave += 1
      if (sinceSave >= 50) {
        await idbSet(`photos:${projectId}`, photos)
        sinceSave = 0
      }
      onProgress({ task: 'display', done: index + 1, total: photos.length, warning: failed > 0 ? `${failed} 枚を読めませんでした` : null })
    }
    await idbSet(`photos:${projectId}`, photos)
    project.metaHashedCount = metaDone
    project.displayedCount = displayDone
    project.prepareWarning = failed > 0 ? `${failed} 枚を読めませんでした` : null
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
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

  async originalUrl(projectId: string, relativePath: string): Promise<string | null> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    if (!project) return null
    try {
      const io = await this.ioFor(project)
      const dir = relativePath.includes('/') ? relativePath.slice(0, relativePath.lastIndexOf('/')) : ''
      const name = relativePath.includes('/') ? relativePath.slice(relativePath.lastIndexOf('/') + 1) : relativePath
      const file = await io.readFile(dir, name, 0)
      return URL.createObjectURL(file)
    } catch {
      return null
    }
  }

  // ---- 選別の途中 ----

  async loadSession(projectId: string): Promise<Session | null> {
    return idbGet<Session>(`session:${projectId}`)
  }

  async saveSession(projectId: string, session: Session): Promise<void> {
    await idbSet(`session:${projectId}`, session)
    await this.markChanged(projectId)
  }

  async loadOverrides(projectId: string): Promise<PairOverride[]> {
    return (await idbGet<PairOverride[]>(`overrides:${projectId}`)) ?? []
  }

  async saveOverrides(projectId: string, overrides: PairOverride[]): Promise<void> {
    await idbSet(`overrides:${projectId}`, overrides)
    await this.markChanged(projectId)
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
    await this.markChanged(projectId)
  }

  async markChanged(projectId: string): Promise<void> {
    const state = (await idbGet<SidecarState>(`sidecarState:${projectId}`)) ?? { seenAt: 0, seenBy: '', localChanged: false }
    state.localChanged = true
    await idbSet(`sidecarState:${projectId}`, state)
  }

  async markCompleted(projectId: string): Promise<void> {
    const project = await idbGet<StoredProject>(`project:${projectId}`)
    if (!project) return
    project.completedAt = Date.now()
    project.updatedAt = Date.now()
    await idbSet(`project:${projectId}`, project)
  }

  // ---- サイドカー ----

  sidecarSupported(project: Project): boolean {
    return project.source.kind === 'folder'
  }

  private sidecarPath = '.photo-curator/catalog.json'

  async checkSidecar(project: Project): Promise<SidecarSync> {
    if (!this.sidecarSupported(project)) return 'Settled'
    const io = await this.ioFor(project)
    const text = await io.readText(this.sidecarPath)
    const state = (await idbGet<SidecarState>(`sidecarState:${project.id}`)) ?? { seenAt: 0, seenBy: '', localChanged: false }
    const remote = text ? core.sidecarFromJson(text) : null
    return core.sidecarDecide(state.seenAt, state.seenBy, state.localChanged, remote)
  }

  async pushSidecarIfChanged(project: Project): Promise<void> {
    if (!this.sidecarSupported(project)) return
    const state = (await idbGet<SidecarState>(`sidecarState:${project.id}`)) ?? { seenAt: 0, seenBy: '', localChanged: false }
    if (!state.localChanged) return
    const io = await this.ioFor(project)
    if (!(await io.canWrite())) return
    const session = await this.loadSession(project.id)
    const overrides = await this.loadOverrides(project.id)
    const burstDistance = await this.loadBurstDistance(project.id)
    const photos: Record<string, { rating: number }> = {}
    if (session) {
      for (const [path, rating] of Object.entries(session.ratings)) photos[path] = { rating }
    }
    const sidecar: Sidecar = {
      version: 1,
      updatedAt: Date.now(),
      updatedBy: await deviceId(),
      updatedByName: deviceName(),
      photos,
      burstOverrides: overrides,
      sessions: session ? { tournament: session } : {},
      burstDistance: burstDistance ?? undefined
    }
    const json = core.sidecarToJson(sidecar)
    const ok = await io.writeText(this.sidecarPath, json)
    if (ok) {
      await idbSet(`sidecarState:${project.id}`, { seenAt: sidecar.updatedAt, seenBy: sidecar.updatedBy, localChanged: false })
    }
  }

  async adoptSidecar(project: Project, sidecar: Sidecar): Promise<void> {
    if (sidecar.sessions.tournament) await idbSet(`session:${project.id}`, sidecar.sessions.tournament)
    await idbSet(`overrides:${project.id}`, sidecar.burstOverrides)
    if (sidecar.burstDistance !== undefined) await this.saveBurstDistance(project.id, sidecar.burstDistance)
    await idbSet(`sidecarState:${project.id}`, { seenAt: sidecar.updatedAt, seenBy: sidecar.updatedBy, localChanged: false })
  }

  async keepMineSidecar(project: Project, theirs: Sidecar): Promise<void> {
    const io = await this.ioFor(project)
    const asidePath = `.photo-curator/catalog.${theirs.updatedBy.slice(0, 12)}.json`
    await io.writeText(asidePath, core.sidecarToJson(theirs))
    const state = (await idbGet<SidecarState>(`sidecarState:${project.id}`)) ?? { seenAt: 0, seenBy: '', localChanged: false }
    state.localChanged = true
    await idbSet(`sidecarState:${project.id}`, state)
    await this.pushSidecarIfChanged(project)
  }

  // ---- 取り出し ----

  async exportFolders(_projectId: string, _photos: RatedPhoto[]): Promise<ExportReport> {
    // Web（フォルダ）は ZIP／CSV のみ（設計 02 章の表）。
    return { processed: 0, skipped: 0, failed: 0, errors: ['Web 版はフォルダ分けに対応していません（ZIP をお使いください）。'] }
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

  // ---- 保存量 ----

  async storageUsageBytes(): Promise<number> {
    if (typeof navigator !== 'undefined' && navigator.storage?.estimate) {
      const estimate = await navigator.storage.estimate()
      return estimate.usage ?? 0
    }
    return 0
  }
}
