import type { Ref } from 'vue'
import type { Photo, Project, TournamentSettings } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, Session } from '~/lib/core'
import { joinSpanOverrides } from '~/utils/burstShape'
import { DEFAULT_BURST_DISTANCE } from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { SaveQueue } from '~/utils/saveQueue'
import type { SerialQueue } from '~/utils/serialQueue'
import { isSlideshowSize, tournamentGroupSize } from '~/utils/groupSize'
import type { GroupSizeLimits } from '~/utils/groupSize'
import type { View } from './types'

/** 選別中の操作が `useCurator` から受け取るもの。 */
export interface TournamentActionsDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  loading: Ref<boolean>
  error: Ref<string>
  settings: TournamentSettings
  groupLimits: GroupSizeLimits
  groupSizeDialog: Ref<boolean>
  nextRoundDialog: Ref<boolean>
  nextRoundGroupSize: Ref<number>
  nextRoundRating: Ref<number>
  saveQueue: SaveQueue<SavedSelection>
  ratingsQueue: SerialQueue
  pathOf: (photoId: string) => string | null
  getDeletingProjectId: () => string | null
  getPairOverrides: () => PairOverride[]
  setPairOverrides: (overrides: PairOverride[]) => void
  applyCore: (next: Session, wait?: boolean) => Promise<void>
  setCore: (next: Session) => void
  loadCurrentPhotos: () => Promise<void>
  loadCoreInputs: (projectId?: string) => Promise<CoreInputs | null>
  ensureCoreInputs: () => Promise<CoreInputs>
  loadPairOverrides: () => Promise<PairOverride[]>
  sessionWithRowRatings: () => Promise<{ base: Session, inputs: CoreInputs, rows: Photo[] }>
  thresholdFor: (distance: number) => BurstThreshold
  currentDistance: () => number
  noteJudgementChanged: (projectId?: string) => void
  flushThenPush: () => Promise<void>
  enterTournamentAfterRebuild: () => Promise<void>
  openSelection: (initial: Session, chosen: TournamentSettings, inputs: CoreInputs) => void
  enterStage: () => Promise<void>
}

/** 選別中の操作（R8。`useCurator.ts` から切り出した。中身は変えていない）。 */
export function useTournamentActions(deps: TournamentActionsDeps) {
  const {
    desktop, activeProject, session, view, loading, error, settings, groupLimits, groupSizeDialog,
    nextRoundDialog, nextRoundGroupSize, nextRoundRating, saveQueue, ratingsQueue, pathOf,
    getDeletingProjectId, getPairOverrides, setPairOverrides, applyCore, setCore, loadCurrentPhotos,
    loadCoreInputs, ensureCoreInputs, loadPairOverrides, sessionWithRowRatings, thresholdFor, currentDistance,
    noteJudgementChanged, flushThenPush, enterTournamentAfterRebuild, openSelection, enterStage
  } = deps

  /**
   * 選別の途中を保存する。**待たない**（待ち行列に入れて戻る）。入れる時点の写しを渡すので、
   * 書くまでの間に画面が封筒を書き換えても、この時点の状態が書かれる（core の Session は丸ごと置き換える決め）。
   */
  function saveSession() {
    const current = session.value
    const project = activeProject.value
    if (!current || !project || project.id === getDeletingProjectId()) return
    current.updatedAt = Date.now()
    saveQueue.enqueue(project.id, {
      ...current,
      settings: { ...current.settings },
      selectedInGroup: [...current.selectedInGroup],
      learning: current.learning
        ? { ...current.learning, answers: [...current.learning.answers] }
        : null
    })
  }

  async function toggleChoice(photoId: string) {
    const current = session.value
    const path = pathOf(photoId)
    // 今の組に無い写真（組が進んだあとに届いた古い操作）は無視する。core の `advance` は組に無い
    // 写真を黙って捨てるので、通すと新しい組が「選ばず」で丸ごと落ちる（W5）。
    if (!current || !path || !current.core.current.includes(path)) return
    if (!current.multiSelect) {
      current.selectedInGroup = [path]
      await confirmChoices()
      return
    }
    const index = current.selectedInGroup.indexOf(path)
    if (index >= 0) current.selectedInGroup.splice(index, 1)
    else current.selectedInGroup.push(path)
    await saveSession()
  }

  /**
   * このグループの判断を確定して次へ進む。**判断は core の `advance`。**
   * 選んだ写真が空でも進める。良い写真が 1 枚も無いグループはあるので、
   * その場合は「1 枚も通さない」という判断として記録される。
   */
  async function confirmChoices() {
    const current = session.value
    if (!current) return
    await advanceWith(core.advance(current.core, [...current.selectedInGroup]))
  }

  /** core が返した次の Session を受け取り、星を行に写して次の組へ。 */
  async function advanceWith(next: Session) {
    const current = session.value
    if (!current) return
    // 封筒に写る画面の状態は、Session を差し替える前に整える（applyCore が先に封筒を待ち行列へ入れる）。
    current.selectedInGroup = []
    current.multiSelect = false
    if (next.finished) current.stage = 'result'
    // 行の星の書き込み・集計は待たずに次の組を出す（W1）。終わったときだけ、結果の数字のために待つ。
    await applyCore(next, false)
    if (next.finished) {
      await ratingsQueue.flush()
      view.value = 'result'
    } else {
      await loadCurrentPhotos()
    }
    await saveSession()
    // ラウンドが終わったら書く（変更があるときだけ）。待たない。
    if (next.finished) void flushThenPush()
  }

  /**
   * スライドショーで 1 枚ぶんを決める。**判断は既存の処理をそのまま通す**
   * （残す＝`advance`、落とす＝選ばずに `advance`、★5＝`keepAndTop`）。
   * 複数選択が残っていると単なる選択のトグルになるので、先に切る。
   */
  async function decideSlide(kind: 'keep' | 'drop' | 'top', photoId: string) {
    const current = session.value
    if (!current) return
    // 飛ばしている間に組が進んだ・戻された（Backspace）とき、古い写真への判断は捨てる（W5）。
    const path = pathOf(photoId)
    if (!path || !current.core.current.includes(path)) return
    current.multiSelect = false
    if (kind === 'keep') await toggleChoice(photoId)
    else if (kind === 'top') await confirmPhoto(photoId)
    else await skipGroup()
  }

  /** このグループからは 1 枚も通さない。 */
  async function skipGroup() {
    if (!session.value) return
    session.value.selectedInGroup = []
    await confirmChoices()
  }

  /**
   * 迷う必要のない 1 枚を ★5 で確定する。**判断は core の `keepAndTop`**。
   * 複数枚選択中は、選んでいた写真も残したままその組を確定する。
   * 以降のラウンドには出ない。「1 つ戻す」で元の星に返る。
   */
  async function confirmPhoto(photoId: string) {
    const current = session.value
    const path = pathOf(photoId)
    if (!current || !path || !current.core.current.includes(path)) return
    const selected = current.multiSelect ? [...current.selectedInGroup] : []
    await advanceWith(core.keepAndTop(current.core, selected, path))
  }

  /** 直前の 1 グループぶんの判断を取り消してやり直す。**判断は core の `undo`。** */
  async function undoChoice() {
    const current = session.value
    if (!current || !current.core.history.length) return
    // 戻した星（仲間の分も）は、前後の差で行にも戻る。
    await applyCore(core.undo(current.core), false)
    current.selectedInGroup = []
    current.multiSelect = false
    current.stage = 'tournament'
    view.value = 'tournament'
    await loadCurrentPhotos()
    await saveSession()
  }

  /** 選別の途中で 1 グループの表示枚数を変える。済んだぶんはそのまま。 */
  async function applyGroupSize(size: number) {
    const current = session.value
    if (!current) return
    setCore(core.resize(current.core, size))
    current.settings = { ...current.settings, groupSize: size }
    current.selectedInGroup = []
    current.multiSelect = false
    groupSizeDialog.value = false
    await enterTournamentAfterRebuild()
  }

  /**
   * 選別中の設定の「連写をまとめる」。**その場で今の組に効かせる**（core の `regroup` は
   * まだ判断していない写真だけを組み直すので、済んだ組はそのまま）。
   */
  async function applyGroupBursts(on: boolean) {
    const current = session.value
    const project = activeProject.value
    if (!current || !project || current.settings.groupBursts === on) return
    try {
      current.settings = { ...current.settings, groupBursts: on }
      if (on) {
        // 距離が決まっていなければ、学習済み（無ければ既定）を使う。ハッシュ値は裏で作っておく。
        current.burstDistance ??= project.burstThreshold ?? DEFAULT_BURST_DISTANCE
        desktop.startBurstAnalysis(project.id).catch(() => undefined)
      }
      const inputs = await loadCoreInputs(project.id) ?? await ensureCoreInputs()
      await loadPairOverrides()
      setCore(core.regroup(current.core, inputs.refs, on, thresholdFor(currentDistance()), getPairOverrides()))
      noteJudgementChanged(project.id)
      current.selectedInGroup = []
      current.multiSelect = false
      await enterTournamentAfterRebuild()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '連写のまとめを切り替えられませんでした。'
    }
  }

  /**
   * 複数選択中の「この写真をまとめる」。選んだ代表（とその仲間）の撮影順で最初から最後までの
   * 隣どうしを全部 `join` の手直しにして保存し、`core.regroup` で今のラウンドに反映する。
   * あいだに挟まる選んでいない写真も同じまとまりに入る（仕様）。
   */
  async function groupSelectedAsBurst() {
    const current = session.value
    const project = activeProject.value
    if (!current || !project || current.selectedInGroup.length < 2) return
    try {
      const inputs = await ensureCoreInputs()
      const existing = await loadPairOverrides()
      const merged = joinSpanOverrides(
        inputs.refs.map(ref => ref.relative_path), current.selectedInGroup, current.core.members, existing
      )
      if (!merged) return
      await desktop.savePairOverrides(project.id, merged)
      noteJudgementChanged(project.id)
      setPairOverrides(merged)
      setCore(core.regroup(current.core, inputs.refs, current.settings.groupBursts, thresholdFor(currentDistance()), merged))
      current.selectedInGroup = []
      current.multiSelect = false
      await enterTournamentAfterRebuild()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '写真をまとめられませんでした。'
    }
  }

  function openNextRoundDialog(rating: number) {
    nextRoundGroupSize.value = tournamentGroupSize(session.value?.settings.groupSize ?? groupLimits.default, groupLimits)
    nextRoundRating.value = rating
    nextRoundDialog.value = true
  }

  /**
   * 指定した星の写真を選別する。
   *
   * 対象は星だけで決まるので、**どんな経路でその星に辿り着いた写真も同じ回に
   * 合流する**。以前は「通過した写真」「落選した写真」という別々の集合を持って
   * いたため、一度分かれると二度と一緒に選別できなかった。
   */
  async function startRatingSelection(rating: number) {
    const project = activeProject.value
    if (!project) return
    nextRoundDialog.value = false
    loading.value = true
    try {
      const chosen: TournamentSettings = { ...(session.value?.settings ?? settings) }
      // スライドショーのセッションは方式を保つ（次のラウンドも 1 枚ずつ）。
      if (!isSlideshowSize(chosen.groupSize) && nextRoundGroupSize.value >= 2) chosen.groupSize = nextRoundGroupSize.value

      // 星は行が持ち主。行の星を入れた土台から、その星ちょうどの写真で始める。
      const { base, inputs } = await sessionWithRowRatings()
      await loadPairOverrides()
      const count = inputs.photos.filter(photo => base.ratings[photo.relativePath] === rating).length
      const distance = session.value?.burstDistance ?? project.burstThreshold ?? DEFAULT_BURST_DISTANCE
      const started = core.roundFor(
        core.resize(base, chosen.groupSize), inputs.refs, rating,
        chosen.groupBursts, thresholdFor(distance), getPairOverrides()
      )
      if (!started) {
        error.value = `★${rating} の写真が ${count} 枚しかないため、選別できません。`
        return
      }
      openSelection(started, chosen, inputs)
      await enterStage()
      await saveSession()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別を準備できませんでした。'
    } finally {
      loading.value = false
    }
  }

  return {
    saveSession, toggleChoice, confirmChoices, decideSlide, confirmPhoto, undoChoice, applyGroupSize,
    applyGroupBursts, groupSelectedAsBurst, openNextRoundDialog, startRatingSelection
  }
}
