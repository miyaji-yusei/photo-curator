// PC（Tauri）の Backend。判断は持たない。Rust 側のコマンドを呼ぶだけ
//（コマンドの実装は 段4・src-tauri/src/lib.rs）。
//
// 絵は Tauri のアセットの口（`convertFileSrc`）で直接ファイルパスから出す
// （設計 05 章。L:\ のようなネットワークドライブも同じ扱い）。

import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AmazonPreview, Backend, CreateProjectInput, ExportReport, FolderEntry, FolderSample, RatedPhoto
} from '~/lib/backend'
import type { PairOverride, Session, Sidecar, SidecarSync } from '~/lib/core'
import type {
  AppSettings, Project, ProjectPhoto, ProjectSource, PrepareProgress
} from '~/types/project'

export class TauriBackend implements Backend {
  readonly environment = 'pc' as const

  async listProjects(): Promise<Project[]> {
    return invoke('list_projects')
  }

  async getProject(id: string): Promise<Project | null> {
    return invoke('get_project', { id })
  }

  async createProject(input: CreateProjectInput): Promise<Project> {
    return invoke('create_project', { input })
  }

  async renameProject(id: string, name: string): Promise<void> {
    await invoke('rename_project', { id, name })
  }

  async estimateDeleteSize(id: string): Promise<number> {
    return invoke('estimate_delete_size', { id })
  }

  async deleteProject(id: string): Promise<void> {
    await invoke('delete_project', { id })
  }

  async restartProject(id: string): Promise<void> {
    await invoke('restart_project', { id })
  }

  async loadSettings(): Promise<AppSettings> {
    return invoke('load_settings')
  }

  async saveSettings(settings: AppSettings): Promise<void> {
    await invoke('save_settings', { settings })
  }

  async pickFolder(): Promise<ProjectSource | null> {
    return invoke('pick_folder')
  }

  async listEntries(source: ProjectSource, subPath: string): Promise<FolderEntry[]> {
    return invoke('list_entries', { source, subPath })
  }

  async recentFolders(): Promise<ProjectSource[]> {
    return invoke('recent_folders')
  }

  async sampleFolder(source: ProjectSource, subPath: string, limit: number): Promise<FolderSample> {
    const result: { count: number; samples: number[][] } = await invoke('sample_folder', { source, subPath, limit })
    return {
      count: result.count,
      samples: result.samples.map(bytes => URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' })))
    }
  }

  /** Amazon タブでリンクを貼った直後の下見。原本は読まない（08章）。 */
  async amazonPreview(shareUrl: string): Promise<AmazonPreview> {
    const result: { key: string; name: string; count: number; samples: number[][] } =
      await invoke('amazon_preview', { shareUrl })
    return {
      key: result.key,
      name: result.name,
      count: result.count,
      samples: result.samples.map(bytes => URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' })))
    }
  }

  async prepare(projectId: string, onProgress: (p: PrepareProgress) => void, signal: AbortSignal): Promise<void> {
    const unlisten = await listen<PrepareProgress>(`prepare-progress:${projectId}`, (event) => {
      onProgress(event.payload)
    })
    try {
      const abortListener = () => { void invoke('cancel_prepare', { projectId }) }
      signal.addEventListener('abort', abortListener)
      await invoke('prepare', { projectId })
      signal.removeEventListener('abort', abortListener)
    } finally {
      unlisten()
    }
  }

  async listPhotos(projectId: string): Promise<ProjectPhoto[]> {
    return invoke('list_photos', { projectId })
  }

  async thumbnailUrl(projectId: string, relativePath: string): Promise<string | null> {
    const path: string | null = await invoke('thumbnail_path', { projectId, relativePath })
    return path ? convertFileSrc(path) : null
  }

  async coverUrl(projectId: string): Promise<string | null> {
    const path: string | null = await invoke('cover_path', { projectId })
    return path ? convertFileSrc(path) : null
  }

  async displayUrl(projectId: string, relativePath: string): Promise<string | null> {
    const path: string | null = await invoke('display_path', { projectId, relativePath })
    return path ? convertFileSrc(path) : null
  }

  async originalUrl(projectId: string, relativePath: string): Promise<string | null> {
    const project = await this.getProject(projectId)
    if (project?.source.kind === 'amazon') {
      // 端末には置かない。開くたびに Amazon から取り直す（08章1・6）。
      try {
        const bytes: number[] = await invoke('amazon_original', { projectId, relativePath })
        return URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' }))
      } catch {
        return null
      }
    }
    const path: string | null = await invoke('original_path', { projectId, relativePath })
    return path ? convertFileSrc(path) : null
  }

  async loadSession(projectId: string): Promise<Session | null> {
    return invoke('load_session', { projectId })
  }

  async saveSession(projectId: string, session: Session): Promise<void> {
    await invoke('save_session', { projectId, session })
  }

  async loadOverrides(projectId: string): Promise<PairOverride[]> {
    return invoke('load_overrides', { projectId })
  }

  async saveOverrides(projectId: string, overrides: PairOverride[]): Promise<void> {
    await invoke('save_overrides', { projectId, overrides })
  }

  async loadBurstDistance(projectId: string): Promise<number | null> {
    return invoke('load_burst_distance', { projectId })
  }

  async saveBurstDistance(projectId: string, distance: number): Promise<void> {
    await invoke('save_burst_distance', { projectId, distance })
  }

  async markChanged(projectId: string): Promise<void> {
    await invoke('mark_changed', { projectId })
  }

  async markCompleted(projectId: string): Promise<void> {
    await invoke('mark_completed', { projectId })
  }

  sidecarSupported(project: Project): boolean {
    return project.source.kind === 'folder'
  }

  async checkSidecar(project: Project): Promise<SidecarSync> {
    return invoke('check_sidecar', { projectId: project.id })
  }

  async pushSidecarIfChanged(project: Project): Promise<void> {
    await invoke('push_sidecar', { projectId: project.id })
  }

  async adoptSidecar(project: Project, sidecar: Sidecar): Promise<void> {
    await invoke('adopt_sidecar', { projectId: project.id, sidecar })
  }

  async keepMineSidecar(project: Project, theirs: Sidecar): Promise<void> {
    await invoke('keep_mine_sidecar', { projectId: project.id, theirs })
  }

  async exportFolders(projectId: string, photos: RatedPhoto[]): Promise<ExportReport> {
    return invoke('export_folders', { projectId, photos })
  }

  async exportXmp(projectId: string, photos: RatedPhoto[]): Promise<ExportReport> {
    return invoke('export_xmp', { projectId, photos })
  }

  async exportCsv(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    const csv: string = await invoke('export_csv', { projectId, photos })
    return new Blob([csv], { type: 'text/csv' })
  }

  /** PC は capabilities.exportZip = false（フォルダ分けの方が本来の出口）。
   * インターフェースを満たすためだけの実装。原本を convertFileSrc 経由で読んで詰める。 */
  async exportZip(projectId: string, photos: RatedPhoto[]): Promise<Blob> {
    const { createStoredZip, uniquePath } = await import('~/utils/zip')
    const taken = new Set<string>()
    const entries = []
    for (const photo of photos) {
      const url = (await this.originalUrl(projectId, photo.relativePath)) ?? (await this.displayUrl(projectId, photo.relativePath))
      if (!url) continue
      const blob = await fetch(url).then(r => r.blob())
      const name = photo.relativePath.split('/').pop() ?? photo.relativePath
      entries.push({ path: uniquePath(taken, `star-${photo.rating}/${name}`), blob, modifiedAt: photo.capturedAt ?? undefined })
    }
    return createStoredZip(entries)
  }

  /** PC は capabilities.share = false。呼ばれない想定。 */
  async shareTargets(_projectId: string, _photos: RatedPhoto[]): Promise<File[]> {
    return []
  }

  async storageUsageBytes(): Promise<number> {
    return invoke('storage_usage_bytes')
  }
}
