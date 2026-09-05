import { convertFileSrc } from '@tauri-apps/api/core'
import { capabilitiesFor, detectPlatform } from '~/utils/capabilities'
import { open } from '@tauri-apps/plugin-dialog'
import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSeed, SelectionSession, SelectionSummary
} from '~/types/photo'
import { normalizeSession } from '~/utils/tournament'
import type { DisplaySettings, PhotoAlbum, PhotoBackend } from '~/composables/photoBackend'
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
    listPhotoAlbums: () => invokeDesktop<PhotoAlbum[]>('list_photo_albums'),
    connectNas: (host: string, share: string, user: string, password: string) =>
      invokeDesktop<PhotoAlbum[]>('connect_nas', { host, share, user, password }),
    disconnectNas: () => invokeDesktop<void>('disconnect_nas'),
    onProjectProgress: async (callback: (progress: ProjectProgress) => void) => {
      if (!isTauriRuntime()) return () => undefined
      const { listen } = await import('@tauri-apps/api/event')
      return listen<ProjectProgress>('project-progress', event => callback(event.payload))
    },
    listProjects: () => invokeDesktop<Project[]>('list_projects'),
    createProject: (name: string, folderPath: string) => invokeDesktop<Project>('create_project', { name, folderPath }),
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
    getSelectionSeed: (projectId: string, rating?: number) =>
      invokeDesktop<SelectionSeed[]>('get_selection_seed', { projectId, rating: rating ?? null }),
    exportByRating: (projectId: string, destination: string, ratings: number[], moveFiles: boolean) =>
      invokeDesktop<ExportReport>('export_by_rating', { projectId, destination, ratings, moveFiles }),
    writeRatingsToFiles: (projectId: string, ratings: number[]) =>
      invokeDesktop<ExportReport>('write_ratings_to_files', { projectId, ratings }),
    getBurstGroups: (projectId: string, threshold?: number) =>
      invokeDesktop<BurstGroup[]>('get_burst_groups', { projectId, threshold: threshold ?? null }),
    getBurstPairs: (projectId: string) => invokeDesktop<BurstPair[]>('get_burst_pairs', { projectId }),
    saveBurstThreshold: (projectId: string, threshold: number) =>
      invokeDesktop<void>('save_burst_threshold', { projectId, threshold }),
    clearBurstThreshold: (projectId: string) => invokeDesktop<void>('clear_burst_threshold', { projectId }),
    getBurstNeighborhood: (projectId: string, photoIds: string[], windowMs?: number) =>
      invokeDesktop<Photo[]>('get_burst_neighborhood', { projectId, photoIds, windowMs: windowMs ?? null }),
    saveBurstShape: (projectId: string, orderedPhotoIds: string[], blocks: string[][]) =>
      invokeDesktop<void>('save_burst_shape', { projectId, orderedPhotoIds, blocks }),
    startProjectScan: (projectId: string) => invokeDesktop<void>('start_project_scan', { projectId }),
    startBurstAnalysis: (projectId: string) => invokeDesktop<void>('start_burst_analysis', { projectId }),
    getAnalysisBacklog: (projectId: string) => invokeDesktop<number>('get_analysis_backlog', { projectId }),
    startBackgroundAnalysis: (projectId: string) => invokeDesktop<void>('start_background_analysis', { projectId }),
    cancelProjectTask: (projectId: string, task: ProjectTask) => invokeDesktop<void>('cancel_project_task', { projectId, task }),
    saveSession: (session: SelectionSession) =>
      invokeDesktop<void>('save_project_state', { projectId: session.projectId, stateJson: JSON.stringify(session) }),
    loadSession: async (projectId: string) => {
      const raw = await invokeDesktop<string | null>('load_project_state', { projectId })
      if (!raw) return null
      // 閾値学習を入れる前に保存された JSON が残っていることがある。
      return normalizeSession(JSON.parse(raw) as SelectionSession)
    },
    photoUrl: (path: string) => isTauriRuntime() ? convertFileSrc(path) : '',
    photoThumbnailUrl: (photo: Photo) => {
      if (!isTauriRuntime()) return ''
      return convertFileSrc(photo.thumbnailPath ?? photo.path)
    },
    // 表示用 → サムネイル → 原本。原本まで落ちるのは生成が追いつく前だけ。
    photoDisplayUrl: (photo: Photo) => {
      if (!isTauriRuntime()) return ''
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
