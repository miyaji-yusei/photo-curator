import type { Ref } from 'vue'
import type { Photo, Project } from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { Session } from '~/lib/core'
import type { CoreInputs } from '~/utils/coreInputs'
import type { MoveSelection } from '~/utils/ratingMove'
// `selectedCount` は選別画面側の computed と名前がぶつかるので別名にする。
import {
  createMoveSelection, isSelected as isMovePicked, selectedCount as countMoveSelection,
  setSelectAll, toMoveArgs, toggleSelection
} from '~/utils/ratingMove'
import { applyChanges, moveRatings, pathsWithRating } from '~/utils/ratingEdit'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { View } from './types'

/** レートの移動が `useCurator` から受け取るもの。 */
export interface MoveDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  error: Ref<string>
  notify: (text: string) => void
  ensureCoreInputs: () => Promise<CoreInputs>
  pathOf: (photoId: string) => string | null
  setCore: (next: Session) => void
  saveSession: () => void
  noteJudgementChanged: (projectId?: string) => void
  loadSummary: () => Promise<void>
  loadResultsPage: (reset?: boolean) => Promise<void>
}

/** レートの移動（U53 で `useCurator.ts` から切り出した。中身は変えていない）。 */
export function useMove(deps: MoveDeps) {
  const {
    desktop, activeProject, session, view, error, notify, ensureCoreInputs, pathOf, setCore, saveSession,
    noteJudgementChanged, loadSummary, loadResultsPage
  } = deps

  // レートの移動
  const moveDialog = ref(false)
  const moveFrom = ref(0)
  const moveTo = ref(0)
  const moveBusy = ref(false)
  const movePhotos = shallowRef<Photo[]>([])
  const moveTotal = ref(0)
  const moveOffset = ref(0)
  /** ダイアログ内に出すエラー。画面上部に出すとモーダルに隠れて気づけない。 */
  const moveError = ref('')
  /**
   * 既定は「全選択」。個別のチェックは**ここからの差分**だけを持つ。
   * 5,000 枚の id を並べて持たないための形。詳細は `utils/ratingMove.ts`。
   */
  const moveSelection = ref<MoveSelection>(createMoveSelection())
  const moveSelectedCount = computed(() => countMoveSelection(moveSelection.value, moveTotal.value))
  const isMoveSelected = (photoId: string) => isMovePicked(moveSelection.value, photoId)

  // ---- レートの移動 --------------------------------------------------------

  /**
   * ある星の写真をまとめて別の星へ移す。
   *
   * 選択状態は id の集合ではなく **「全選択からの差分」** で持つ。
   * 既定が全選択なので、id を並べる持ち方だと開いた瞬間に 5,000 件をフロントへ
   * 載せることになる。差分なら、利用者が実際に触った枚数しか持たない。
   * 「全解除」を押すと `moveSelectAll` が反転し、差分の意味も反転する。
   */
  function openMoveDialog(rating: number) {
    moveFrom.value = rating
    // 移動先の初期値は、上限に居るときだけ1つ下。それ以外は1つ上。
    moveTo.value = rating >= MAX_RATING ? rating - 1 : rating + 1
    moveSelection.value = createMoveSelection()
    movePhotos.value = []
    moveOffset.value = 0
    moveTotal.value = 0
    moveError.value = ''
    moveDialog.value = true
    void loadMovePage(true)
  }

  /** 移動の一覧の読み直し（reset）の世代（W7）。 */
  let moveToken = 0
  async function loadMovePage(reset = false) {
    if (!activeProject.value) return
    const token = reset ? ++moveToken : moveToken
    moveBusy.value = true
    try {
      if (reset) {
        moveOffset.value = 0
        movePhotos.value = []
      }
      const page = await desktop.getProjectPhotoPage(
        activeProject.value.id, moveOffset.value, 80, moveFrom.value, 'name'
      )
      if (token !== moveToken) return
      movePhotos.value = [...movePhotos.value, ...page.photos]
      moveTotal.value = page.total
      moveOffset.value += page.photos.length
    } catch (cause) {
      if (token === moveToken) moveError.value = cause instanceof Error ? cause.message : '写真を読み込めませんでした。'
    } finally {
      if (token === moveToken) moveBusy.value = false
    }
  }

  function toggleMoveSelection(photoId: string) {
    moveSelection.value = toggleSelection(moveSelection.value, photoId)
  }

  /** 全選択・全解除は、差分の基準そのものを切り替える。 */
  function setMoveSelectAll(all: boolean) {
    moveSelection.value = setSelectAll(all)
  }

  async function runMove() {
    if (!activeProject.value || moveFrom.value === moveTo.value) return
    moveBusy.value = true
    moveError.value = ''
    const { includeIds, excludeIds } = toMoveArgs(moveSelection.value)
    try {
      // セッションに無い写真（あとから増えた写真など）で移す対象（U52 D10）。移す前の星で選ぶ。
      // セッションに入れないと比較キーに映らず同期されないうえ、次の取り込みで行の星が 0 に戻される。
      let outside: string[] = []
      if (session.value) {
        const ratings = session.value.core.ratings
        const include = includeIds ? new Set(includeIds) : null
        const exclude = new Set(excludeIds)
        outside = (await desktop.getCoreInputs(activeProject.value.id))
          .filter(row => !(row.relativePath in ratings) && row.rating === moveFrom.value
            && (include ? include.has(row.id) : !exclude.has(row.id)))
          .map(row => row.relativePath)
      }
      const moved = await desktop.moveRating(
        activeProject.value.id, moveFrom.value, moveTo.value, includeIds, excludeIds
      )
      noteJudgementChanged()
      // 進行中のセッションが持つ星も合わせる。人が星を決める手直しなので `ratingEdit` で。
      // Session に無い写真は、上で選んだ `outside` として足す。行への書き込みは上の `moveRating` が済ませている。
      if (session.value) {
        await ensureCoreInputs()
        const excluded = new Set(excludeIds.map(pathOf))
        const paths = includeIds
          ? includeIds.map(pathOf).filter((path): path is string => path !== null)
          : pathsWithRating(session.value.core, moveFrom.value).filter(path => !excluded.has(path))
        const added = Object.fromEntries(outside.map(path => [path, moveTo.value]))
        setCore(applyChanges(moveRatings(session.value.core, paths, moveTo.value), added))
        await saveSession()
      }
      moveDialog.value = false
      await loadSummary()
      if (view.value === 'results') await loadResultsPage(true)
      error.value = ''
      notify(`${moved.toLocaleString()} 枚を ★${moveFrom.value} から ★${moveTo.value} へ移しました。`)
    } catch (cause) {
      moveError.value = cause instanceof Error ? cause.message : 'レートを移動できませんでした。'
    } finally {
      moveBusy.value = false
    }
  }

  return {
    moveDialog,
    moveFrom,
    moveTo,
    moveBusy,
    movePhotos,
    moveTotal,
    moveOffset,
    moveError,
    moveSelectedCount,
    isMoveSelected,
    openMoveDialog,
    loadMovePage,
    toggleMoveSelection,
    setMoveSelectAll,
    runMove
  }
}
