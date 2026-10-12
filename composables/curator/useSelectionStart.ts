import type { Ref } from 'vue'
import type { Project, TournamentSettings } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, Session } from '~/lib/core'
import { buildBurstQuestions } from '~/utils/burstQuestions'
import { DEFAULT_BURST_DISTANCE } from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import type { SavedSelection } from '~/utils/selectionFlow'
import { canStartSelection } from '~/utils/selectionGate'
import { SLIDESHOW_GROUP_SIZE, clampGroupSize, tournamentGroupSize } from '~/utils/groupSize'
import type { GroupSizeLimits } from '~/utils/groupSize'
import type { View } from './types'

/** 選別の開始が `useCurator` から受け取るもの。 */
export interface SelectionStartDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  loading: Ref<boolean>
  error: Ref<string>
  settings: TournamentSettings
  groupLimits: GroupSizeLimits
  taskDialog: Ref<boolean>
  scanRunning: Ref<boolean>
  taskWarning: Ref<string | null>
  restartDialog: Ref<boolean>
  restartForStart: Ref<boolean>
  pendingTournamentSettings: Ref<TournamentSettings | null>
  sidecarClash: Ref<unknown>
  sidecarChecking: Ref<boolean>
  hasSelectionData: Ref<boolean>
  syncAtBreak: (project: Project) => Promise<boolean>
  loadSummary: () => Promise<void>
  loadCoreInputs: (projectId?: string) => Promise<CoreInputs | null>
  ensureCoreInputs: () => Promise<CoreInputs>
  loadPairOverrides: () => Promise<PairOverride[]>
  getPairOverrides: () => PairOverride[]
  thresholdFor: (distance: number) => BurstThreshold
  currentDistance: () => number
  noteJudgementChanged: (projectId?: string) => void
  setCore: (next: Session) => void
  saveSession: () => void
  loadCurrentPhotos: () => Promise<void>
  showNextPair: () => Promise<void>
  refreshBurstPreview: (distance: number) => Promise<void>
}

/** 選別の開始（R5。`useCurator.ts` から切り出した。中身は変えていない）。 */
export function useSelectionStart(deps: SelectionStartDeps) {
  const {
    desktop, activeProject, session, view, loading, error, settings, groupLimits, taskDialog, scanRunning, taskWarning,
    restartDialog, restartForStart, pendingTournamentSettings, sidecarClash, sidecarChecking, hasSelectionData,
    syncAtBreak, loadSummary, loadCoreInputs, ensureCoreInputs, loadPairOverrides, getPairOverrides,
    thresholdFor, currentDistance, noteJudgementChanged, setCore, saveSession, loadCurrentPhotos,
    showNextPair, refreshBurstPreview
  } = deps

  function enterMethod() {
    if (!canStartSelection({ photoCount: activeProject.value?.photoCount ?? 0, scanRunning: scanRunning.value, sidecarClash: !!sidecarClash.value, sidecarChecking: sidecarChecking.value })) return
    view.value = 'method'
  }

  /** 方法の選択の「スライドショー」。設定の画面は同じで、枚数の設定は出さず 1 枚ずつにする。 */
  function openSlideshowSettings() {
    openSettings()
    settings.groupSize = SLIDESHOW_GROUP_SIZE
  }

  function openSettings() {
    const prior = session.value?.settings
    settings.groupSize = tournamentGroupSize(prior?.groupSize ?? groupLimits.default, groupLimits)
    settings.groupBursts = prior?.groupBursts ?? false
    view.value = 'settings'
  }

  // 解析の完了を待たない。scan 後の事前生成で出来ているぶんをそのまま使い、
  // 未解析が残っていても選別画面へ進む。残りはバックグラウンドで進み続ける。
  async function beginTournament() {
    if (!canStartSelection({ hasProject: !!activeProject.value, taskDialog: taskDialog.value, scanRunning: scanRunning.value, sidecarClash: !!sidecarClash.value }) || !activeProject.value) return
    // 始める前にサイドカーを確かめる（設計書 §4.5）。この端末が未着手で、別の端末が進めていれば、
    // ここで確認なしに取り込む。取り込んだら始めずにプロジェクトの画面へ戻し、続きから再開してもらう
    // （このまま始めると、取り込んだ星を全部 0 にしてしまう）。食い違えばダイアログを出して止める。
    if (await syncAtBreak(activeProject.value)) {
      view.value = 'project'
      return
    }
    if (sidecarClash.value) {
      view.value = 'project'
      return
    }
    // 星が 1 つでも付いている（取り込んだ星を含む）と、開始は星を全部 0 にする。
    // 「最初からやり直す」と同じ確認を先に出す。「キャンセル」なら何もしない。
    await loadSummary()
    if (hasSelectionData.value) {
      restartForStart.value = true
      restartDialog.value = true
      return
    }
    await startTournament()
  }

  async function startTournament() {
    if (!canStartSelection({ hasProject: !!activeProject.value, taskDialog: taskDialog.value, scanRunning: scanRunning.value, sidecarClash: !!sidecarClash.value }) || !activeProject.value) return
    pendingTournamentSettings.value = { ...settings }
    if (settings.groupBursts) {
      taskWarning.value = null
      // 起動できなくても選別自体は始められる。ここで止めない。
      desktop.startBurstAnalysis(activeProject.value.id).catch(() => undefined)
    }
    await finishTournamentStart()
  }

  async function finishTournamentStart() {
    const project = activeProject.value
    const chosen = pendingTournamentSettings.value
    if (!project || !chosen) return
    loading.value = true
    try {
      // 最初から選び直すので、前回の星と落選は消す。数字を膨らませない。
      await desktop.resetSelectionResults(project.id).catch(() => undefined)
      noteJudgementChanged(project.id)
      const inputs = await loadCoreInputs(project.id)
      if (!inputs?.refs.length) throw new Error('選別できる写真がありません。')
      await loadPairOverrides()
      const learned = project.burstThreshold
      const initial = core.startRound(
        inputs.refs, chosen.groupSize, 0, chosen.groupBursts,
        thresholdFor(learned ?? DEFAULT_BURST_DISTANCE), getPairOverrides()
      )
      openSelection(initial, chosen, inputs)
      await enterStage()
      await saveSession()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別を準備できませんでした。'
    } finally {
      pendingTournamentSettings.value = null
      loading.value = false
    }
  }

  /**
   * 始めた選別を封筒に入れる。開始時の行き先を決める。
   * - 連写をまとめない設定 / 候補ペアが無い → そのまま選別
   * - 学習済みの距離がある → 質問を飛ばして確認画面へ
   * - それ以外 → 連写の学習から
   * 学習・確認の間の Session は既定か学習済みの距離で組んだ仮のもので、
   * 「この設定で選別を始める」で `regroup` して確定する。
   */
  function openSelection(initial: Session, chosen: TournamentSettings, inputs: CoreInputs) {
    const learned = activeProject.value?.burstThreshold ?? null
    const questions = chosen.groupBursts ? buildBurstQuestions(inputs.refs) : []
    const stage = !chosen.groupBursts || !questions.length
      ? 'tournament'
      : learned === null ? 'burst-threshold' : 'burst-preview'
    session.value = {
      v: 2,
      core: markRaw(initial),
      stage,
      settings: { ...chosen },
      multiSelect: false,
      selectedInGroup: [],
      learning: stage === 'burst-threshold' ? { questions, index: 0, answers: [] } : null,
      burstDistance: chosen.groupBursts ? learned : null,
      updatedAt: Date.now()
    }
  }

  /** セッションの stage に合わせて画面と必要なデータを揃える。 */
  async function enterStage() {
    const current = session.value
    if (!current) return
    await ensureCoreInputs()
    if (current.stage === 'burst-threshold') {
      view.value = 'burst-threshold'
      await showNextPair()
      return
    }
    if (current.stage === 'burst-preview') {
      view.value = 'burst-preview'
      await loadPairOverrides()
      await refreshBurstPreview(currentDistance())
      return
    }
    if (current.stage === 'result' || current.core.finished) {
      current.stage = 'result'
      view.value = 'result'
      return
    }
    current.stage = 'tournament'
    view.value = 'tournament'
    await loadCurrentPhotos()
  }

  async function resumeSession() {
    // 別の端末の記録との食い違いを選ぶまで、選別は始めさせない。
    if (!canStartSelection({ scanRunning: scanRunning.value, sidecarClash: !!sidecarClash.value, sidecarChecking: sidecarChecking.value })) return
    if (!session.value) return openSettings()
    // 別の環境で作られたセッションは、この端末の上限を超える枚数を持ちうる。
    const clamped = clampGroupSize(session.value.settings.groupSize, groupLimits)
    session.value.settings.groupSize = clamped
    if (session.value.core.group_size !== clamped) setCore(core.resize(session.value.core, clamped))
    await enterStage()
  }

  return {
    enterMethod, openSlideshowSettings, openSettings, beginTournament, startTournament,
    finishTournamentStart, openSelection, enterStage, resumeSession
  }
}
