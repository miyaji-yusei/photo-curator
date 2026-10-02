import type { Ref } from 'vue'
import type { BurstGroup, Photo, Project } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, Session } from '~/lib/core'
import type { CoreInputs } from '~/utils/coreInputs'
import { applyChanges, reviewChanges } from '~/utils/ratingEdit'
import { syncRatings } from '~/utils/selectionFlow'
import type { RatingChange, SavedSelection } from '~/utils/selectionFlow'
import type { View } from './types'

/** 連写の見直しが `useCurator` から受け取るもの。 */
export interface BurstReviewDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  error: Ref<string>
  idOf: (path: string) => string | null
  thresholdFor: (distance: number) => BurstThreshold
  currentDistance: () => number
  toViewGroup: (group: core.BurstGroup, inputs: CoreInputs) => BurstGroup
  loadCoreInputs: (projectId?: string) => Promise<CoreInputs | null>
  loadPairOverrides: () => Promise<PairOverride[]>
  sessionWithRowRatings: () => Promise<{ base: Session, inputs: CoreInputs, rows: Photo[] }>
  setCore: (next: Session) => void
  saveSession: () => void
  writeRatings: (changes: RatingChange[], wait?: boolean) => Promise<void>
  loadSummary: () => Promise<void>
  returnToResults: () => Promise<void>
}

/** 連写の見直し（選別が終わったあと）（U53 で `useCurator.ts` から切り出した。中身は変えていない）。 */
export function useBurstReview(deps: BurstReviewDeps) {
  const {
    desktop, activeProject, session, view, error, idOf, thresholdFor, currentDistance, toViewGroup,
    loadCoreInputs, loadPairOverrides, sessionWithRowRatings, setCore, saveSession, writeRatings,
    loadSummary, returnToResults
  } = deps

  /** 枚数に対する列数。1画面に収まりやすい並びを枚数ごとに決めてある。 */
  function columnsFor(count: number) {
    const byCount: Record<number, number> = { 1: 1, 2: 2, 3: 3, 4: 2, 5: 3, 6: 3, 7: 4, 8: 4, 9: 3, 10: 5 }
    return byCount[count] ?? Math.min(5, Math.max(1, Math.ceil(Math.sqrt(count))))
  }

  // 連写の見直し（選別が終わったあと）。
  //
  // 選別中、まとめは代表1枚に畳まれ、仲間には代表と同じ星が配られる。そこまでで
  // 「まとめ全体の良し悪し」は決まるが、**その中のどれが一番良いか**はまだ決めて
  // いない。この画面がその1手を引き受ける。
  //
  // グループは保存していない。`getBurstGroups` が学習済みの閾値から**そのつど
  // 引き直す**ので、セッションが終わっても、何度でもここへ戻ってこられる。
  const burstReviewGroups = ref<BurstGroup[]>([])
  const burstReviewIndex = ref(0)
  const burstReviewPhotos = ref<Photo[]>([])
  const burstReviewKept = ref<string[]>([])
  const burstReviewBusy = ref(false)
  const burstReviewLoaded = ref(false)
  const burstReviewColumns = computed(() => columnsFor(burstReviewPhotos.value.length))
  const burstReviewRows = computed(() =>
    Math.max(1, Math.ceil(burstReviewPhotos.value.length / burstReviewColumns.value))
  )

  /** いま見ているまとめ。 */
  const burstReviewGroup = computed(() => burstReviewGroups.value[burstReviewIndex.value] ?? null)

  /**
   * 連写の見直しを開く。**まとめは保存していない**ので、学習済みの閾値から
   * その場で引き直す。2枚以上のものだけが対象。
   */
  async function openBurstReview(focus?: unknown) {
    if (!activeProject.value) return
    view.value = 'burst-review'
    burstReviewLoaded.value = false
    burstReviewBusy.value = true
    burstReviewGroups.value = []
    burstReviewIndex.value = 0
    burstReviewPhotos.value = []
    try {
      const inputs = await loadCoreInputs()
      if (!inputs) throw new Error('プロジェクトが開かれていません。')
      const overrides = await loadPairOverrides()
      // 判断は core の `groupBursts`。2 枚以上のものだけが対象。
      const groups = core.groupBursts(inputs.refs, thresholdFor(currentDistance()), overrides)
      burstReviewGroups.value = groups
        .filter(group => group.members.length > 1)
        .map(group => toViewGroup(group, inputs))
      // 結果の格子の `⧉N` から来たときは、その連写から始める（`focus` は仲間の 1 枚の relativePath）。
      if (typeof focus === 'string') {
        const id = idOf(focus)
        const at = id ? burstReviewGroups.value.findIndex(group => group.photoIds.includes(id)) : -1
        if (at > 0) burstReviewIndex.value = at
      }
      await loadBurstReviewPhotos()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '連写を読み込めませんでした。'
    } finally {
      burstReviewBusy.value = false
      burstReviewLoaded.value = true
    }
  }

  /**
   * いま見ているまとめの写真だけを読む。**全グループぶんを先読みしない。**
   * 連写が数百グループある写真集でも、載るのは常に1グループぶん。
   */
  async function loadBurstReviewPhotos() {
    const group = burstReviewGroup.value
    burstReviewKept.value = []
    if (!activeProject.value || !group) {
      burstReviewPhotos.value = []
      return
    }
    const photos = await desktop.getPhotosByIds(activeProject.value.id, group.photoIds)
    // getPhotosByIds の並びは問わない。まとめの中は撮影順で見せる。
    const order = new Map(group.photoIds.map((id, index) => [id, index]))
    burstReviewPhotos.value = [...photos].sort(
      (left, right) => (order.get(left.id) ?? 0) - (order.get(right.id) ?? 0)
    )
  }

  function toggleBurstReviewKeep(photoId: string) {
    const kept = burstReviewKept.value
    burstReviewKept.value = kept.includes(photoId)
      ? kept.filter(id => id !== photoId)
      : [...kept, photoId]
  }

  /** 次のまとめへ。最後まで来たら結果画面に戻す。 */
  async function advanceBurstReview() {
    if (burstReviewIndex.value + 1 >= burstReviewGroups.value.length) {
      await returnToResults()
      return
    }
    burstReviewIndex.value += 1
    await loadBurstReviewPhotos()
  }

  /**
   * 残す写真を確定する。**残した写真は+1、外した写真は−1。**
   * 通常の選別と違って下げるのは、ここが「星をそろえたあとの絞り込み」だから。
   */
  async function applyBurstReview() {
    if (!activeProject.value || !burstReviewKept.value.length) return
    burstReviewBusy.value = true
    try {
      // 人が星を決める手直し。行の星を土台に `ratingEdit` で +1 / −1 して、変わった行だけ書く。
      const { base } = await sessionWithRowRatings()
      const shown = burstReviewPhotos.value.map(photo => photo.relativePath)
      const kept = burstReviewPhotos.value
        .filter(photo => burstReviewKept.value.includes(photo.id))
        .map(photo => photo.relativePath)
      const next = markRaw(applyChanges(base, reviewChanges(base, shown, kept)))
      // Session を先に待ち行列へ入れてから、行の星を書く（applyCore と同じ順。途中で終了しても開き直しで揃う）。
      if (session.value) {
        setCore(next)
        saveSession()
      }
      await writeRatings(syncRatings(base, next))
      await loadSummary()
      await advanceBurstReview()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '連写の結果を保存できませんでした。'
    } finally {
      burstReviewBusy.value = false
    }
  }

  async function skipBurstReview() {
    burstReviewBusy.value = true
    try {
      await advanceBurstReview()
    } finally {
      burstReviewBusy.value = false
    }
  }

  return {
    burstReviewGroups,
    burstReviewIndex,
    burstReviewPhotos,
    burstReviewKept,
    burstReviewBusy,
    burstReviewLoaded,
    burstReviewColumns,
    burstReviewRows,
    openBurstReview,
    toggleBurstReviewKeep,
    applyBurstReview,
    skipBurstReview
  }
}
