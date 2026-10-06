import type { Ref } from 'vue'
import type { Project, ProjectProgress } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { CoreInputs } from '~/utils/coreInputs'
import type { View } from './types'

/** 進みのイベントの受け取りが `useCurator` から受け取るもの。 */
export interface ProgressEventsDeps {
  desktop: PhotoBackend
  projects: Ref<Project[]>
  activeProject: Ref<Project | null>
  view: Ref<View>
  error: Ref<string>
  taskProgress: Ref<ProjectProgress | null>
  taskWarning: Ref<string | null>
  taskDialog: Ref<boolean>
  analysisProgress: Ref<ProjectProgress | null>
  analysisFailures: Ref<number>
  sidecar: { refreshAccess: (projectId: string) => Promise<unknown> }
  // 再代入される `let`（`sidecarCheckPending`）は関数で渡す。
  getSidecarCheckPending: () => string | null
  setSidecarCheckPending: (projectId: string | null) => void
  refreshProjects: () => Promise<void>
  refreshAnalysisFailures: (projectId?: string) => Promise<void>
  refreshPrepareCounts: (projectId: string, force?: boolean) => Promise<void>
  loadPreview: (projectId: string) => Promise<void>
  refreshPreviewThumbnails: (projectId: string) => Promise<void>
  loadCoreInputs: (projectId?: string) => Promise<CoreInputs | null>
  startDisplayAfterScan: (projectId: string) => Promise<void>
  runSidecarCheck: (project: Project) => Promise<{ kind: string } | null | undefined>
  reloadAfterSidecar: (projectId: string) => Promise<void>
}

/** 進みのイベント（解析・走査・表示用画像）の受け取り（R6。`useCurator.ts` から切り出した。中身は変えていない）。 */
export function useProgressEvents(deps: ProgressEventsDeps) {
  const {
    desktop, projects, activeProject, view, error, taskProgress, taskWarning, taskDialog, analysisProgress,
    analysisFailures, sidecar, getSidecarCheckPending, setSidecarCheckPending, refreshProjects,
    refreshAnalysisFailures, refreshPrepareCounts, loadPreview, refreshPreviewThumbnails, loadCoreInputs,
    startDisplayAfterScan, runSidecarCheck, reloadAfterSidecar
  } = deps

  /** 準備の途中で、格子のサムネイルを少しずつ埋める（ブラウザだけ。PC は元から原本が見える）。 */
  let previewRefreshedAt = 0
  async function refreshDuringPreparation(progress: ProjectProgress) {
    if (desktop.kind !== 'local' || view.value !== 'project') return
    const finished = progress.phase === 'complete' || progress.phase === 'cancelled'
    const now = Date.now()
    if (!finished && now - previewRefreshedAt < 3000) return
    previewRefreshedAt = now
    await (finished ? loadPreview(progress.projectId) : refreshPreviewThumbnails(progress.projectId))
      .catch(() => undefined)
    if (!finished) return
    await refreshProjects().catch(() => undefined)
    await loadCoreInputs(progress.projectId).catch(() => undefined)
  }

  /** 進みのイベントを受け取り始める。止める関数を返す（`unmount` で呼ぶ）。 */
  async function listenProgress() {
  return await desktop.onProjectProgress(async (progress) => {
    if (!activeProject.value || progress.projectId !== activeProject.value.id) return
    // 連写解析（前面・事前生成とも）は待たせない。帯で状況だけ伝える。
    if (progress.task !== 'scan') {
      analysisProgress.value = progress
      analysisFailures.value = progress.failed
      // 警告は解析中の1イベントにしか乗らないので、別に保持して出し続ける。
      if (progress.warning) taskWarning.value = progress.warning
      if (progress.phase === 'error') {
        error.value = progress.message
        void refreshProjects().then(() => {
          activeProject.value = projects.value.find(project => project.id === activeProject.value?.id) ?? activeProject.value
        })
      }
      if (progress.phase === 'complete' || progress.phase === 'cancelled') void refreshAnalysisFailures(progress.projectId)
      void refreshPrepareCounts(progress.projectId, progress.phase === 'complete')
      if (progress.task === 'background') void refreshDuringPreparation(progress)
      return
    }
    taskProgress.value = progress
    if (progress.warning) taskWarning.value = progress.warning
    if (progress.phase === 'complete') {
      taskDialog.value = false
      await refreshProjects()
      activeProject.value = projects.value.find(project => project.id === activeProject.value?.id) ?? activeProject.value
      await loadPreview(progress.projectId)
      void refreshPrepareCounts(progress.projectId, true)
      await loadCoreInputs(progress.projectId).catch(() => undefined)
      // 表示用画像は走査のあとに溜める（開き直さないと始まらなかった）。
      await startDisplayAfterScan(progress.projectId)
      await sidecar.refreshAccess(progress.projectId)
      if (getSidecarCheckPending() === progress.projectId && activeProject.value) {
        // 写真の行ができたので、開いたときの確認をここで行う（取り込んだ星を行へ写せる）。
        setSidecarCheckPending(null)
        const outcome = await runSidecarCheck(activeProject.value)
        if (outcome?.kind === 'pulled') await reloadAfterSidecar(progress.projectId)
      }
    }
    if (progress.phase === 'cancelled' || progress.phase === 'error') {
      taskDialog.value = false
      if (progress.phase === 'error') {
        error.value = progress.message
        // リンクが消えたときなど、状態が変わっているので取り直す。
        await refreshProjects()
        activeProject.value = projects.value.find(project => project.id === activeProject.value?.id) ?? activeProject.value
      }
    }
  })
  }

  return { refreshDuringPreparation, listenProgress }
}
