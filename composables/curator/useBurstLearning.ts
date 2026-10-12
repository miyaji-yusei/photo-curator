import type { ComputedRef, Ref } from 'vue'
import type { BurstGroup, Photo, Project } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, Session } from '~/lib/core'
import type { BurstQuestion } from '~/utils/burstQuestions'
import { buildBurstQuestions } from '~/utils/burstQuestions'
import { BURST_WINDOW_MS, DEFAULT_BURST_DISTANCE, maxNeighborDistance } from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { View } from './types'

/** 連写の学習・確認が `useCurator` から受け取るもの。 */
export interface BurstLearningDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  error: Ref<string>
  pairPhotos: Ref<Photo[]>
  previewThreshold: Ref<number>
  previewBusy: Ref<boolean>
  previewMaxDistance: Ref<number>
  burstPreviewGroups: Ref<BurstGroup[]>
  currentQuestion: ComputedRef<BurstQuestion | null>
  idsOf: (paths: string[]) => string[]
  thresholdFor: (distance: number) => BurstThreshold
  ensureCoreInputs: () => Promise<CoreInputs>
  loadPairOverrides: () => Promise<PairOverride[]>
  getPairOverrides: () => PairOverride[]
  noteJudgementChanged: (projectId?: string) => void
  refreshProjects: () => Promise<void>
  flushThenPush: () => Promise<void>
  setCore: (next: Session) => void
  saveSession: () => void
  loadCurrentPhotos: () => Promise<void>
}

/** 連写の学習・確認（R7。`useCurator.ts` から切り出した。中身は変えていない）。 */
export function useBurstLearning(deps: BurstLearningDeps) {
  const {
    desktop, activeProject, session, view, error, pairPhotos, previewThreshold, previewBusy, previewMaxDistance,
    burstPreviewGroups, currentQuestion, idsOf, thresholdFor, ensureCoreInputs, loadPairOverrides, getPairOverrides,
    noteJudgementChanged, refreshProjects, flushThenPush, setCore, saveSession, loadCurrentPhotos
  } = deps

  /** 次の出題を用意する。出し尽くしたら確認画面へ進む。 */
  async function showNextPair() {
    const current = session.value
    const learning = current?.learning
    if (!current || !learning) return
    while (learning.index < learning.questions.length) {
      const question = learning.questions[learning.index]!
      const ids = idsOf([question.left.relative_path, question.right.relative_path])
      const photos = activeProject.value && ids.length === 2
        ? await desktop.getPhotosByIds(activeProject.value.id, ids)
        : []
      if (photos.length === 2) {
        pairPhotos.value = photos
        return
      }
      // 行が見つからない（消えた・欠損）問いは飛ばす。
      learning.index += 1
    }
    await finishThresholdLearning()
  }

  async function answerPair(grouped: boolean) {
    const learning = session.value?.learning
    const question = currentQuestion.value
    if (!learning || !question) return
    learning.answers = [...learning.answers, { distance: question.distance, same: grouped }]
    learning.index += 1
    await showNextPair()
    await saveSession()
  }

  /** 判断できない問いは学習に使わない。答えに入れず次を出す。 */
  async function skipCurrentPair() {
    const learning = session.value?.learning
    if (!learning || !currentQuestion.value) return
    learning.index += 1
    await showNextPair()
    await saveSession()
  }

  async function finishThresholdLearning() {
    const current = session.value
    if (!current) return
    const answers = (current.learning?.answers ?? []).map(answer => ({ ...answer }))
    // 答えから距離を決めるのは core。答えが無ければ既定値のまま。
    const distance = core.learnDistance(answers, DEFAULT_BURST_DISTANCE)
    pairPhotos.value = []
    current.stage = 'burst-preview'
    view.value = 'burst-preview'
    await loadPairOverrides()
    await refreshBurstPreview(distance)
    await saveSession()
  }

  let previewMaxFor: CoreInputs | null = null

  /** 距離を当てた結果を取り直す。スライダー操作からも呼ぶ（core は同期で速い）。 */
  async function refreshBurstPreview(distance: number) {
    const current = session.value
    if (!current || !activeProject.value) return
    previewThreshold.value = distance
    previewBusy.value = true
    try {
      const inputs = await ensureCoreInputs()
      if (previewMaxFor !== inputs) {
        previewMaxDistance.value = maxNeighborDistance(inputs.refs, BURST_WINDOW_MS, core.hashDistance)
        previewMaxFor = inputs
      }
      const groups = core.groupBursts(inputs.refs, thresholdFor(distance), getPairOverrides())
      burstPreviewGroups.value = groups
        .filter(group => group.members.length > 1)
        .map(group => toViewGroup(group, inputs))
      current.burstDistance = distance
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '連写のまとめ結果を取得できませんでした。'
    } finally {
      previewBusy.value = false
    }
  }

  /** core のまとまりを、画面が使う `BurstGroup`（写真の id）に写す。 */
  function toViewGroup(group: core.BurstGroup, inputs: CoreInputs): BurstGroup {
    const times = group.members
      .map(path => inputs.byPath.get(path)?.capturedAt)
      .filter((at): at is number => typeof at === 'number')
    return {
      id: group.representative,
      photoIds: idsOf(group.members),
      capturedSpanMs: times.length ? Math.max(...times) - Math.min(...times) : 0,
      similarity: 0,
      accepted: null
    }
  }

  /** さらに質問して距離を詰める。 */
  async function askMorePairs() {
    const current = session.value
    if (!current) return
    const inputs = await ensureCoreInputs()
    current.learning = {
      questions: current.learning?.questions ?? buildBurstQuestions(inputs.refs),
      index: 0,
      answers: []
    }
    current.stage = 'burst-threshold'
    view.value = 'burst-threshold'
    await showNextPair()
    await saveSession()
  }

  /** 確認した距離を保存し、選別へ進む。 */
  async function acceptBurstThreshold() {
    const current = session.value
    if (!current || !activeProject.value) return
    const distance = previewThreshold.value
    try {
      await desktop.saveBurstThreshold(activeProject.value.id, distance)
      noteJudgementChanged()
      activeProject.value.burstThreshold = distance
      await refreshProjects()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '閾値を保存できませんでした。'
    }
    const inputs = await ensureCoreInputs()
    // まとめた連写は代表 1 枚に畳む。まだ何も決めていないので、組み直しで足りる。
    setCore(core.regroup(current.core, inputs.refs, true, thresholdFor(distance), getPairOverrides()))
    current.burstDistance = distance
    current.learning = null
    current.selectedInGroup = []
    current.multiSelect = false
    await enterTournamentAfterRebuild()
  }

  /** core の組が変わったあと、画面を今の組に合わせる。 */
  async function enterTournamentAfterRebuild() {
    const current = session.value
    if (!current) return
    if (current.core.finished) {
      current.stage = 'result'
      view.value = 'result'
    } else {
      current.stage = 'tournament'
      view.value = 'tournament'
      await loadCurrentPhotos()
    }
    await saveSession()
    if (current.core.finished) void flushThenPush()
  }

  /** 設定画面から学習をやり直す。 */
  async function relearnThreshold() {
    if (!activeProject.value) return
    try {
      await desktop.clearBurstThreshold(activeProject.value.id)
      noteJudgementChanged()
      activeProject.value.burstThreshold = null
      await refreshProjects()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '閾値を消去できませんでした。'
    }
  }

  return {
    showNextPair, answerPair, skipCurrentPair, finishThresholdLearning, refreshBurstPreview, toViewGroup,
    askMorePairs, acceptBurstThreshold, enterTournamentAfterRebuild, relearnThreshold
  }
}
