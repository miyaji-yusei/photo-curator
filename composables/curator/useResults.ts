import type { ComputedRef, Ref } from 'vue'
import type { Photo, PhotoSort, Project, SelectionSummary } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { Session } from '~/lib/core'
import { collapseBursts } from '~/utils/collapseBursts'
import type { View } from './types'

/** 選別結果の一覧が `useCurator` から受け取るもの。 */
export interface ResultsDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  coreSession: ComputedRef<Session | null>
  view: Ref<View>
  error: Ref<string>
}

/**
 * 選別結果の一覧と集計（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 * `selectionSummary`（星ごとの枚数）の持ち主もここ。行の星の書き込みの待ち行列（`useCurator`）も、これを書き換える。
 */
export function useResults(deps: ResultsDeps) {
  const { desktop, activeProject, coreSession, view, error } = deps

  const resultsPhotos = shallowRef<Photo[]>([])
  const resultsTotal = ref(0)
  const resultsOffset = ref(0)
  const resultsBusy = ref(false)
  /** null は全件。数値はその星ちょうど。 */
  const resultsRating = ref<number | null>(null)
  const resultsSort = ref<PhotoSort>('rating')
  const selectionSummary = ref<SelectionSummary | null>(null)
  /** 星が1つでも付いていれば結果を見る意味がある。 */
  const hasSelectionData = computed(() => {
    const counts = selectionSummary.value?.counts ?? []
    return counts.slice(1).some(count => count > 0)
  })
  const ratingCount = (rating: number) => selectionSummary.value?.counts[rating] ?? 0

  /** 選別結果の一覧。中断中でも開ける。 */
  async function openResults(rating?: unknown) {
    if (!activeProject.value) return
    view.value = 'results'
    resultsOffset.value = 0
    resultsPhotos.value = []
    await loadSummary()
    // 星の指定が無ければ、その結果で実際に付いている一番高い星に絞る（全部 ★0 なら「すべて」）。
    // プロジェクトの画面の星の行から来たときは、その星のまま。
    // （`@click="openResults"` はイベントを渡してくるので、数値か null だけを指定とみなす。）
    resultsRating.value = rating === null || typeof rating === 'number' ? rating : highestRating()
    await loadResultsPage(true)
  }

  /** 結果へ戻る（絞り込みはそのまま）。 */
  const returnToResults = () => openResults(resultsRating.value)

  /** 実際に付いている一番高い星（1〜5）。1 枚も付いていなければ null。 */
  function highestRating(): number | null {
    return [5, 4, 3, 2, 1].find(star => ratingCount(star) > 0) ?? null
  }

  /** 結果の格子のタイル。連写は、読み込んだ行の中で星が一番高い 1 枚に畳む。 */
  const resultsTiles = computed(() => collapseBursts(resultsPhotos.value, coreSession.value?.members))

  /** ページの末尾が見えたら次のページ。 */
  function loadMoreResults() {
    if (!resultsBusy.value && resultsPhotos.value.length < resultsTotal.value) void loadResultsPage()
  }

  async function loadSummary() {
    if (!activeProject.value) return
    try {
      selectionSummary.value = await desktop.getSelectionSummary(activeProject.value.id)
    } catch (cause) {
      // 「選別結果を見る」が黙って消えないように、読めなかったことを出す。
      selectionSummary.value = null
      console.warn('選別の集計を読めませんでした', cause)
      error.value = cause instanceof Error ? `選別の集計を読めませんでした（${cause.message}）` : '選別の集計を読めませんでした。'
    }
  }

  async function selectResultsRating(rating: number | null) {
    resultsRating.value = rating
    await loadResultsPage(true)
  }

  /** 結果の読み直し（reset）の世代。reset が来たら、それ以前の読み込みの応答は捨てる（W7）。 */
  let resultsToken = 0
  async function loadResultsPage(reset = false) {
    if (!activeProject.value) return
    const token = reset ? ++resultsToken : resultsToken
    resultsBusy.value = true
    try {
      if (reset) {
        resultsOffset.value = 0
        resultsPhotos.value = []
      }
      const page = await desktop.getProjectPhotoPage(
        activeProject.value.id, resultsOffset.value, 80, resultsRating.value, resultsSort.value
      )
      if (token !== resultsToken) return
      resultsPhotos.value = [...resultsPhotos.value, ...page.photos]
      resultsTotal.value = page.total
      resultsOffset.value += page.photos.length
    } catch (cause) {
      if (token === resultsToken) error.value = cause instanceof Error ? cause.message : '選別結果を読み込めませんでした。'
    } finally {
      // 古い読み込みが、新しい読み込みの「読み込み中」を落とさない。
      if (token === resultsToken) resultsBusy.value = false
    }
  }

  return {
    resultsPhotos,
    resultsTotal,
    resultsOffset,
    resultsBusy,
    resultsRating,
    resultsSort,
    selectionSummary,
    hasSelectionData,
    ratingCount,
    openResults,
    returnToResults,
    resultsTiles,
    loadMoreResults,
    loadSummary,
    selectResultsRating,
    loadResultsPage
  }
}
