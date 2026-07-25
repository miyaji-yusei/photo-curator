import { convertFileSrc } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { BurstGroup, BurstPair, ExportReport, Photo, PhotoPage, PhotoSort, Project, ProjectProgress, ProjectTask, SelectionResult, SelectionSeed, SelectionSession, SelectionSummary } from '~/types/photo'
import { normalizeSession } from '~/utils/tournament'

const isDesktop = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

async function invokeDesktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop()) throw new Error('This feature is available in the Windows desktop app only.')
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<T>(command, args)
  } catch (cause) {
    if (cause instanceof Error) throw cause
    if (typeof cause === 'string' && cause.trim()) throw new Error(cause)
    throw new Error(`デスクトップ処理に失敗しました（${command}）。`)
  }
}

export function useDesktop() {
  return {
    isDesktop,
    chooseFolder: async () => {
      if (!isDesktop()) throw new Error('Folder selection is available in the Windows desktop app only.')
      const selected = await open({ directory: true, multiple: false, title: 'Select a photo folder' })
      return typeof selected === 'string' ? selected : null
    },
    onProjectProgress: async (callback: (progress: ProjectProgress) => void) => {
      if (!isDesktop()) return () => undefined
      const { listen } = await import('@tauri-apps/api/event')
      return listen<ProjectProgress>('project-progress', event => callback(event.payload))
    },
    listProjects: () => invokeDesktop<Project[]>('list_projects'),
    createProject: (name: string, folderPath: string) => invokeDesktop<Project>('create_project', { name, folderPath }),
    // 写真原本は消さない。DB の行とサムネイルだけ。
    deleteProject: (projectId: string) => invokeDesktop<void>('delete_project', { projectId }),
    // rating を渡すとその星ちょうどの写真だけ。null は全件。
    getProjectPhotoPage: (projectId: string, offset = 0, limit = 80, rating: number | null = null, sort: PhotoSort = 'name') =>
      invokeDesktop<PhotoPage>('get_project_photo_page', { projectId, offset, limit, rating, sort }),
    // 判定したグループぶんだけを書く。全件を毎回送らない。
    saveSelectionResults: (projectId: string, entries: SelectionResult[]) =>
      invokeDesktop<void>('save_selection_results', { projectId, entries }),
    resetSelectionResults: (projectId: string) => invokeDesktop<void>('reset_selection_results', { projectId }),
    getSelectionSummary: (projectId: string) => invokeDesktop<SelectionSummary>('get_selection_summary', { projectId }),
    getPhotosByIds: (projectId: string, photoIds: string[]) => invokeDesktop<Photo[]>('get_photos_by_ids', { projectId, photoIds }),
    // rating を渡すとその星の写真だけ。選別対象は星で決まる。
    getSelectionSeed: (projectId: string, rating?: number) =>
      invokeDesktop<SelectionSeed[]>('get_selection_seed', { projectId, rating: rating ?? null }),
    // 星ごとのフォルダへ書き出す。moveFiles が true なら原本を移動する。
    exportByRating: (projectId: string, destination: string, ratings: number[], moveFiles: boolean) =>
      invokeDesktop<ExportReport>('export_by_rating', { projectId, destination, ratings, moveFiles }),
    // 星を写真本体の XMP に書き込む。原本を書き換える。
    writeRatingsToFiles: (projectId: string, ratings: number[]) =>
      invokeDesktop<ExportReport>('write_ratings_to_files', { projectId, ratings }),
    // threshold を省くとプロジェクトの学習値、それも無ければ既定値が使われる。
    getBurstGroups: (projectId: string, threshold?: number) =>
      invokeDesktop<BurstGroup[]>('get_burst_groups', { projectId, threshold: threshold ?? null }),
    // 閾値学習の出題元。距離での足切りはされていない。
    getBurstPairs: (projectId: string) => invokeDesktop<BurstPair[]>('get_burst_pairs', { projectId }),
    saveBurstThreshold: (projectId: string, threshold: number) =>
      invokeDesktop<void>('save_burst_threshold', { projectId, threshold }),
    clearBurstThreshold: (projectId: string) => invokeDesktop<void>('clear_burst_threshold', { projectId }),
    startProjectScan: (projectId: string) => invokeDesktop<void>('start_project_scan', { projectId }),
    startBurstAnalysis: (projectId: string) => invokeDesktop<void>('start_burst_analysis', { projectId }),
    // まだ解析が要る写真の枚数。0 なら事前生成を起動しない。
    getAnalysisBacklog: (projectId: string) => invokeDesktop<number>('get_analysis_backlog', { projectId }),
    // scan 完了後の事前生成。既に走っていても失敗しない（呼び出し側で握り潰す必要はない）。
    startBackgroundAnalysis: (projectId: string) => invokeDesktop<void>('start_background_analysis', { projectId }),
    cancelProjectTask: (projectId: string, task: ProjectTask) => invokeDesktop<void>('cancel_project_task', { projectId, task }),
    saveSession: (session: SelectionSession) => invokeDesktop<void>('save_project_state', { projectId: session.projectId, stateJson: JSON.stringify(session) }),
    loadSession: async (projectId: string) => {
      const raw = await invokeDesktop<string | null>('load_project_state', { projectId })
      if (!raw) return null
      // 閾値学習を入れる前に保存された JSON が残っていることがある。
      return normalizeSession(JSON.parse(raw) as SelectionSession)
    },
    photoUrl: (path: string) => isDesktop() ? convertFileSrc(path) : '',
    // 一覧のような小さく大量に並ぶ場所は、解析時に作った 256px のサムネイルを
    // 使い回す。dHash の計算に使ったのと同じ1枚なので、追加のデコードは無い。
    // まだ解析していない写真は原本にフォールバックする。
    photoThumbnailUrl: (photo: Photo) => {
      if (!isDesktop()) return ''
      return convertFileSrc(photo.thumbnailPath ?? photo.path)
    }
  }
}
