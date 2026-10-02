import type { Ref, ShallowRef } from 'vue'
import type { Project } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { PairOverride } from '~/lib/core'
import type { CoreInputs } from '~/utils/coreInputs'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { ClashChoice, useSidecarSync } from '~/composables/useSidecarSync'

type SidecarSync = ReturnType<typeof useSidecarSync>

/** サイドカーを呼ぶ側の薄い層が `useCurator` から受け取るもの。 */
export interface SidecarActionsDeps {
  desktop: PhotoBackend
  sidecar: SidecarSync
  sidecarClash: SidecarSync['clash']
  /** 行の星の書き込みの待ち行列（W1）。 */
  ratingsQueue: { flush: () => Promise<void> }
  /** 選別の途中（封筒）の保存の待ち行列。 */
  saveQueue: { flush: () => Promise<void> }
  projects: Ref<Project[]>
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  coreInputs: ShallowRef<CoreInputs | null>
  /** `useCurator` が持つ連写の手直し（`pairOverrides`）を置き換える。 */
  setPairOverrides: (overrides: PairOverride[]) => void
  refreshProjects: () => Promise<void>
  loadPreview: (projectId: string) => Promise<void>
  loadSummary: () => Promise<void>
}

/**
 * サイドカー（`useSidecarSync`）を呼ぶ側の薄い層。保存の列を書き終えてから呼び、取り込んだら画面を読み直す
 * （U53 で `useCurator.ts` から切り出した。中身は変えていない。**サイドカーの判断は useSidecarSync と core**）。
 */
export function useSidecarActions(deps: SidecarActionsDeps) {
  const {
    desktop, sidecar, sidecarClash, ratingsQueue, saveQueue, projects, activeProject, session, coreInputs,
    setPairOverrides, refreshProjects, loadPreview, loadSummary
  } = deps

  /** 開いたときのサイドカーの確認。書く・取り込むは片付け、食い違いだけダイアログを出す。 */
  async function runSidecarCheck(project: Project) {
    await core.init()
    return sidecar.checkOnOpen(project)
  }

  /**
   * 選別画面から戻ったとき・選別を始める前の確認（設計書 §4.5）。保存の列を書き終えてから読む。
   * 取り込んだら画面を読み直して true を返す。
   */
  async function syncAtBreak(project: Project): Promise<boolean> {
    await ratingsQueue.flush()
    await saveQueue.flush()
    const outcome = await runSidecarCheck(project)
    if (outcome?.kind !== 'pulled') return false
    await reloadAfterSidecar(project.id)
    return true
  }

  /** 取り込んだあとに、画面が持っている分を読み直す。 */
  async function reloadAfterSidecar(projectId: string) {
    if (activeProject.value?.id !== projectId) return
    await saveQueue.flush()
    const value = await desktop.loadSession(projectId)
    if (value) value.core = markRaw(value.core)
    session.value = value
    coreInputs.value = null
    setPairOverrides([])
    await refreshProjects().catch(() => undefined)
    activeProject.value = projects.value.find(item => item.id === projectId) ?? activeProject.value
    await loadPreview(projectId).catch(() => undefined)
    await loadSummary()
  }

  /**
   * 食い違いのダイアログの答え（5 択）。選ぶまで選別は始められない（「この端末の状況を残す」が先へ進む役）。
   * 端末の選別状況が変わる答え（取り込む・混ぜる）のあとは、画面が持っている分を読み直す。
   */
  async function resolveSidecarClash(choice: ClashChoice) {
    const projectId = sidecarClash.value?.projectId
    if (!projectId) return
    await ratingsQueue.flush()
    await saveQueue.flush()
    if (await sidecar.resolve(choice)) await reloadAfterSidecar(projectId)
  }

  /** 「今すぐ保存」。開いたときと同じ判断（取り込んだら読み直す）。 */
  async function saveSidecarNow() {
    await ratingsQueue.flush()
    await saveQueue.flush()
    const project = activeProject.value
    if (!project) return
    const outcome = await sidecar.saveNow(project)
    if (outcome?.kind === 'pulled') await reloadAfterSidecar(project.id)
  }

  /** 切り離し中（「この端末の状況を残す」のあと）の「NAS に書き込む」。 */
  async function writeSidecarToNas() {
    await ratingsQueue.flush()
    await saveQueue.flush()
    if (activeProject.value) await sidecar.writeToNas(activeProject.value)
  }

  return {
    runSidecarCheck,
    syncAtBreak,
    reloadAfterSidecar,
    resolveSidecarClash,
    saveSidecarNow,
    writeSidecarToNas
  }
}
