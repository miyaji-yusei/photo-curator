import { convertFileSrc } from '@tauri-apps/api/core'
import { capabilitiesFor, detectPlatform } from '~/utils/capabilities'
import { open, save } from '@tauri-apps/plugin-dialog'
import type {
  AmazonPreview, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSummary
} from '~/types/photo'
import type { PairOverride } from '~/lib/core'
import { init as initCore } from '~/lib/core'
import type { SavedSelection } from '~/utils/selectionFlow'
import { parseSavedSelection, serializeSavedSelection } from '~/utils/selectionFlow'
import type { DeviceIdentity, DisplaySettings, PhotoBackend, SidecarAccess, SidecarState } from '~/composables/photoBackend'
import { isTauriRuntime } from '~/composables/photoBackend'

async function invokeDesktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) throw new Error('This feature is available in the Windows desktop app only.')
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<T>(command, args)
  } catch (cause) {
    if (cause instanceof Error) throw cause
    if (typeof cause === 'string' && cause.trim()) throw new Error(cause)
    throw new Error(`デスクトップ処理に失敗しました（${command}）。`)
  }
}

/**
 * デスクトップ（Tauri）向けの実装。各メソッドは Rust の
 * `#[tauri::command]` と 1:1 で対応する。
 */
export function createTauriBackend(): PhotoBackend {
  /** 出所の種類。原本の URL を取るとき、Amazon かどうかを引く（`listProjects`・作成で更新する）。 */
  const sourceKinds = new Map<string, Project['sourceKind']>()
  const remember = (projects: Project[]) => {
    for (const project of projects) sourceKinds.set(project.id, project.sourceKind)
    return projects
  }
  return {
    kind: 'tauri',
    // Android も Tauri なので、UA まで見て初めて desktop と分かれる。
    capabilities: capabilitiesFor(
      detectPlatform(isTauriRuntime(), typeof navigator === 'undefined' ? '' : navigator.userAgent)
    ),
    chooseFolder: async () => {
      if (!isTauriRuntime()) throw new Error('Folder selection is available in the Windows desktop app only.')
      const selected = await open({ directory: true, multiple: false, title: 'Select a photo folder' })
      return typeof selected === 'string' ? selected : null
    },
    onProjectProgress: async (callback: (progress: ProjectProgress) => void) => {
      if (!isTauriRuntime()) return () => undefined
      const { listen } = await import('@tauri-apps/api/event')
      return listen<ProjectProgress>('project-progress', event => callback(event.payload))
    },
    listProjects: async () => remember(await invokeDesktop<Project[]>('list_projects')),
    createProject: async (name: string, folderPath: string) =>
      remember([await invokeDesktop<Project>('create_project', { name, folderPath })])[0]!,
    amazonPreview: (shareUrl: string) => invokeDesktop<AmazonPreview>('amazon_preview', { shareUrl }),
    createAmazonProject: async (name: string, shareUrl: string) =>
      remember([await invokeDesktop<Project>('create_amazon_project', { name, shareUrl })])[0]!,
    deleteProject: (projectId: string) => invokeDesktop<void>('delete_project', { projectId }),
    getProjectPhotoPage: (projectId: string, offset = 0, limit = 80, rating: number | null = null, sort: PhotoSort = 'name') =>
      invokeDesktop<PhotoPage>('get_project_photo_page', { projectId, offset, limit, rating, sort }),
    saveSelectionResults: (projectId: string, entries: SelectionResult[]) =>
      invokeDesktop<void>('save_selection_results', { projectId, entries }),
    resetSelectionResults: (projectId: string) => invokeDesktop<void>('reset_selection_results', { projectId }),
    moveRating: (
      projectId: string, fromRating: number, toRating: number,
      includeIds: string[] | null = null, excludeIds: string[] = []
    ) => invokeDesktop<number>('move_rating', { projectId, fromRating, toRating, includeIds, excludeIds }),
    getSelectionSummary: (projectId: string) => invokeDesktop<SelectionSummary>('get_selection_summary', { projectId }),
    getPhotosByIds: (projectId: string, photoIds: string[]) => invokeDesktop<Photo[]>('get_photos_by_ids', { projectId, photoIds }),
    getCoreInputs: (projectId: string) => invokeDesktop<Photo[]>('get_core_inputs', { projectId }),
    exportPhotos: (projectId: string, destination: string, photoIds: string[], moveFiles: boolean) =>
      invokeDesktop<ExportReport>('export_photos', { projectId, destination, photoIds, moveFiles }),
    writeRatingsToPhotos: (projectId: string, photoIds: string[]) =>
      invokeDesktop<ExportReport>('write_ratings_to_photos', { projectId, photoIds }),
    saveCsv: async (fileName: string, text: string) => {
      if (!isTauriRuntime()) throw new Error('CSV の保存はデスクトップアプリで使えます。')
      const chosen = await save({
        title: 'CSV を保存', defaultPath: fileName, filters: [{ name: 'CSV', extensions: ['csv'] }]
      })
      if (!chosen) return false
      await invokeDesktop<void>('write_text_file', { path: chosen, text })
      return true
    },
    saveBurstThreshold: (projectId: string, threshold: number) =>
      invokeDesktop<void>('save_burst_threshold', { projectId, threshold }),
    clearBurstThreshold: (projectId: string) => invokeDesktop<void>('clear_burst_threshold', { projectId }),
    getPairOverrides: (projectId: string) => invokeDesktop<PairOverride[]>('get_pair_overrides', { projectId }),
    savePairOverrides: (projectId: string, overrides: PairOverride[]) =>
      invokeDesktop<void>('save_pair_overrides', { projectId, overrides }),
    startProjectScan: (projectId: string) => invokeDesktop<void>('start_project_scan', { projectId }),
    startBurstAnalysis: (projectId: string) => invokeDesktop<void>('start_burst_analysis', { projectId }),
    getAnalysisBacklog: (projectId: string) => invokeDesktop<number>('get_analysis_backlog', { projectId }),
    startBackgroundAnalysis: (projectId: string) => invokeDesktop<void>('start_background_analysis', { projectId }),
    cancelProjectTask: (projectId: string, task: ProjectTask) => invokeDesktop<void>('cancel_project_task', { projectId, task }),
    saveSession: (projectId: string, selection: SavedSelection | null) =>
      invokeDesktop<void>('save_project_state', {
        projectId, stateJson: selection ? serializeSavedSelection(selection) : 'null'
      }),
    loadSession: async (projectId: string) => {
      const raw = await invokeDesktop<string | null>('load_project_state', { projectId })
      if (!raw) return null
      // Session の読み戻しは core（wasm）が行う。旧版の形は null（最初から）。
      await initCore()
      return parseSavedSelection(raw)
    },
    sidecarSupported: (projectId: string) => invokeDesktop<SidecarAccess>('sidecar_supported', { projectId }),
    readSidecar: (projectId: string) => invokeDesktop<string | null>('read_sidecar', { projectId }),
    writeSidecar: (projectId: string, json: string, fileName = 'catalog.json') =>
      invokeDesktop<void>('write_sidecar', { projectId, json, fileName }),
    loadSidecarState: (projectId: string) => invokeDesktop<SidecarState>('load_sidecar_state', { projectId }),
    saveSidecarState: (projectId: string, state: SidecarState) =>
      invokeDesktop<void>('save_sidecar_state', { projectId, state }),
    deviceIdentity: () => invokeDesktop<DeviceIdentity>('device_identity'),
    photoUrl: (path: string) => isTauriRuntime() ? convertFileSrc(path) : '',
    photoOriginalUrl: async (photo: Photo) => {
      if (!isTauriRuntime()) return ''
      if (sourceKinds.get(photo.projectId) !== 'amazon') return convertFileSrc(photo.path)
      const path = await invokeDesktop<string>('amazon_original', { projectId: photo.projectId, photoId: photo.id })
      return convertFileSrc(path)
    },
    // Amazon の写真の `path` は node id で、ファイルではない。絵がまだ無いときは空にする。
    photoThumbnailUrl: (photo: Photo) => {
      if (!isTauriRuntime()) return ''
      if (sourceKinds.get(photo.projectId) === 'amazon') {
        return photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : ''
      }
      return convertFileSrc(photo.thumbnailPath ?? photo.path)
    },
    // 表示用 → サムネイル → 原本。原本まで落ちるのは生成が追いつく前だけ。
    photoDisplayUrl: (photo: Photo) => {
      if (!isTauriRuntime()) return ''
      if (sourceKinds.get(photo.projectId) === 'amazon') {
        const path = photo.displayPath ?? photo.thumbnailPath
        return path ? convertFileSrc(path) : ''
      }
      return convertFileSrc(photo.displayPath ?? photo.thumbnailPath ?? photo.path)
    },
    getDisplaySettings: () => invokeDesktop<DisplaySettings>('get_display_settings'),
    saveDisplayEdge: (edge: number) => invokeDesktop<number>('save_display_edge', { edge }),
    saveProjectDisplayEdge: (projectId: string, edge: number | null) =>
      invokeDesktop<number>('save_project_display_edge', { projectId, edge }),
    getDisplayBacklog: (projectId: string) => invokeDesktop<number>('get_display_backlog', { projectId }),
    startDisplayGeneration: (projectId: string) => invokeDesktop<void>('start_display_generation', { projectId }),
    resetDisplayImages: (projectId: string) => invokeDesktop<void>('reset_display_images', { projectId })
  }
}
