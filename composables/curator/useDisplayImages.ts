import type { Ref } from 'vue'
import type { Project } from '~/types/photo'
import type { DisplaySettings, PhotoBackend } from '~/composables/photoBackend'
import { builtDisplayCount, displayEdgePlan } from '~/utils/displayEdge'

/** 表示用画像の設定が `useCurator` から受け取るもの。 */
export interface DisplayImagesDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  error: Ref<string>
  taskWarning: Ref<string | null>
  /** 選別の先読み（表示用の場所を含む行）を捨てる。 */
  clearPrefetched: () => void
}

/**
 * 表示用画像の設定（長辺・残りの生成量・作り直し）（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 * `displayBacklog` は準備の進み（`useCurator` の `refreshPrepareCounts`・`prepareLines`）も読み書きする。
 */
export function useDisplayImages(deps: DisplayImagesDeps) {
  const { desktop, activeProject, error, taskWarning, clearPrefetched } = deps

  // 選別画面に出す表示用画像の設定。既定は全体、プロジェクトごとに上書きできる。
  const displaySettings = ref<DisplaySettings | null>(null)
  /** このプロジェクトで実際に使う長辺。 */
  const displayEdge = ref(0)
  const displayBacklog = ref(0)
  const displayBusy = ref(false)
  /** 「大きな画像で選別する」= 大きい方の長辺を選んでいるか。 */
  const largeDisplay = computed({
    get: () => displayEdge.value >= (displaySettings.value?.largeEdge ?? 1536),
    set: (on: boolean) => {
      const settings = displaySettings.value
      if (settings) void requestDisplayEdge(on ? settings.largeEdge : settings.defaultEdge)
    }
  })
  /** 「表示用画像を作り直します。よろしいですか」の確認。OK までは px を変えない（選択は元のまま）。 */
  const displayEdgeDialog = ref(false)
  const pendingDisplayEdge = ref<number | null>(null)

  // ---- 表示用画像 ----------------------------------------------------------

  /**
   * 表示用画像の設定と、残っている生成量を読む。
   * プロジェクトを開くたびに呼ぶので、外で作られた分もここで拾える。
   */
  async function refreshDisplayState() {
    if (!activeProject.value) return
    // 表示用画像が替わりうるので、先読みした行（表示用の場所を含む）は捨てる。
    clearPrefetched()
    try {
      // プロジェクトの上書きを反映した実効値（読むだけ。上書きは消さない）。
      const settings = await desktop.getDisplaySettings(activeProject.value.id)
      displaySettings.value = settings
      displayEdge.value = settings.projectEdge ?? settings.edge
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
    } catch (cause) {
      // 設定が読めなくても選別は続けられる。表示用が無ければ原本に落ちるだけ。
      // ただし設定のカードが黙って消えるので、読めなかったことは通知に出す。
      displaySettings.value = null
      console.warn('表示用画像の設定を読めませんでした', cause)
      taskWarning.value = '表示用画像の設定を読めませんでした。'
    }
  }

  /**
   * 長辺を変えて作り直す。
   *
   * **下げるときは原本を読み直さない**（保存済みを縮めるだけ）。上げるときは
   * 原本が要るので通信量が増える。UI にその違いを出しておく。
   */
  async function applyDisplayEdge(edge: number) {
    if (!activeProject.value || displayBusy.value) return
    displayBusy.value = true
    try {
      displayEdge.value = await desktop.saveProjectDisplayEdge(activeProject.value.id, edge)
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
      await desktop.startDisplayGeneration(activeProject.value.id)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '表示用の設定を変えられませんでした。'
    } finally {
      displayBusy.value = false
    }
  }

  /** 走査が終わったプロジェクトの表示用画像を、選んだ長辺で作り始める。 */
  async function startDisplayAfterScan(projectId: string) {
    if (activeProject.value?.id !== projectId) return
    await refreshDisplayState()
    if (displayBacklog.value > 0) desktop.startDisplayGeneration(projectId).catch(() => undefined)
  }

  /**
   * プロジェクトの画面から px を変える。作り直しが要るとき（作られた画像があり、値が変わる）だけ
   * 確認を出す。要らなければ保存だけ。変えられないとき・同じ値のときは何もしない。
   */
  async function requestDisplayEdge(edge: number) {
    const project = activeProject.value
    if (!project || displayBusy.value) return
    const plan = displayEdgePlan({
      current: displayEdge.value,
      next: edge,
      canRebuild: displaySettings.value?.canRebuild,
      rebuildsOnChange: displaySettings.value?.rebuildsOnChange,
      builtCount: builtDisplayCount(project.photoCount, displayBacklog.value)
    })
    if (plan === 'locked' || plan === 'same') return
    if (plan === 'confirm') {
      pendingDisplayEdge.value = edge
      displayEdgeDialog.value = true
      return
    }
    await applyDisplayEdge(edge)
  }

  /** 確認の OK。選んだ px で作り直す。 */
  async function confirmDisplayEdge() {
    const edge = pendingDisplayEdge.value
    displayEdgeDialog.value = false
    pendingDisplayEdge.value = null
    if (edge !== null) await applyDisplayEdge(edge)
  }

  /** 確認のキャンセル。選択は元のまま、作り直さない。 */
  function cancelDisplayEdge() {
    displayEdgeDialog.value = false
    pendingDisplayEdge.value = null
  }

  /** 明示的に作り直す。壊れたときや、途中で止まったときの逃げ道。 */
  async function regenerateDisplayImages() {
    if (!activeProject.value || displayBusy.value) return
    displayBusy.value = true
    try {
      await desktop.resetDisplayImages(activeProject.value.id)
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
      await desktop.startDisplayGeneration(activeProject.value.id)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '作り直しを始められませんでした。'
    } finally {
      displayBusy.value = false
    }
  }

  return {
    displaySettings,
    displayEdge,
    displayBacklog,
    displayBusy,
    largeDisplay,
    displayEdgeDialog,
    pendingDisplayEdge,
    refreshDisplayState,
    applyDisplayEdge,
    startDisplayAfterScan,
    requestDisplayEdge,
    confirmDisplayEdge,
    cancelDisplayEdge,
    regenerateDisplayImages
  }
}
