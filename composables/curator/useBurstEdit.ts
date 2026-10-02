import type { Ref } from 'vue'
import type { Photo, Project } from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, Session } from '~/lib/core'
import {
  blocksFromCuts, cutAll, cutAroundSelection, cutsFromGroups, moveCut, toggleAt
} from '~/utils/burstEdit'
import { burstNeighborhood } from '~/utils/burstNeighborhood'
import { overridesFromShape } from '~/utils/burstShape'
import { BURST_WINDOW_MS, toPhotoRef } from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import { setRating } from '~/utils/ratingEdit'
import type { SavedSelection } from '~/utils/selectionFlow'

/** まとめの中身を直す画面が `useCurator` から受け取るもの。 */
export interface BurstEditDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  error: Ref<string>
  burstSizeOf: (photoId: string) => number
  idsOf: (paths: string[]) => string[]
  pathOf: (photoId: string) => string | null
  thresholdFor: (distance: number) => BurstThreshold
  currentDistance: () => number
  ensureCoreInputs: () => Promise<CoreInputs>
  loadPairOverrides: () => Promise<PairOverride[]>
  /** `useCurator` が持つ連写の手直し（`pairOverrides`）を置き換える。 */
  setPairOverrides: (overrides: PairOverride[]) => void
  setCore: (next: Session) => void
  applyCore: (next: Session, wait?: boolean) => Promise<void>
  saveSession: () => void
  noteJudgementChanged: (projectId?: string) => void
  enterTournamentAfterRebuild: () => Promise<void>
}

/**
 * まとめの中身を直す画面（境目のバー・代表の指名・まとめの中の ★5／脱落）
 * （U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 */
export function useBurstEdit(deps: BurstEditDeps) {
  const {
    desktop, activeProject, session, error, burstSizeOf, idsOf, pathOf, thresholdFor, currentDistance,
    ensureCoreInputs, loadPairOverrides, setPairOverrides, setCore, applyCore, saveSession, noteJudgementChanged,
    enterTournamentAfterRebuild
  } = deps

  // まとめの中身を直す画面。
  //
  // 状態の中心は **`burstCuts`（隣どうしの境目）** ひとつだけ。まとまりは
  // `burstPhotos` の並びの上で必ず連続しているので、境目の真偽値の列があれば
  // 分割・切り離し・結合・全解除がすべて表せる。詳細は `utils/burstEdit.ts`。
  const burstDialog = ref(false)
  const burstOwner = ref<Photo | null>(null)
  /** 撮影順に並んだ1続きの写真。まとめの中身と、近くの写真の両方が入る。 */
  const burstPhotos = ref<Photo[]>([])
  const burstCuts = ref<boolean[]>([])
  /** 開いたときに元のまとめへ入っていた写真。外の写真と見分けるために持つ。 */
  const burstOriginal = ref<string[]>([])
  const burstPicked = ref<string[]>([])
  /** 利用者が明示的に代表へ指名した写真。まとまりを組み直すときに優先する。 */
  const burstReps = ref<string[]>([])
  const burstBusy = ref(false)

  /**
   * まとめの中身を開く。**ここが「まとまりの形」を直す唯一の場所。**
   *
   * 表示するのは代表のまとめだけではなく、**撮影順で前後 4 秒に入る1続きの写真**。
   * まとめの中身も、まとめに入れられる近くの写真も、同じ1本の並びの上にあるので、
   * 「切る」「繋ぐ」の 2 つだけで分割・切り離し・追加・全解除がすべて表せる。
   */
  async function openBurst(photo: Photo | null) {
    const current = session.value
    if (!photo || !current || !activeProject.value || burstSizeOf(photo.id) < 2) return
    const memberPaths = current.core.members[photo.relativePath] ?? []
    burstOwner.value = photo
    burstDialog.value = true
    burstBusy.value = true
    burstPicked.value = []
    burstReps.value = []
    try {
      const inputs = await ensureCoreInputs()
      // 撮影順の全写真から、前後 4 秒に入る 1 続きを切り出す（時刻で絞るだけ）。
      const run = burstNeighborhood(inputs.photos, memberPaths, BURST_WINDOW_MS)
      // 近くに何も無ければ、まとめの中身だけで組む。
      const ids = run.length ? run.map(item => item.id) : idsOf(memberPaths)
      burstPhotos.value = await desktop.getPhotosByIds(activeProject.value.id, ids)
      const shown = burstPhotos.value.map(item => item.id)
      // いまのまとまり方を境目に起こす。まとめに属さない近くの写真は、
      // それぞれ 1 枚のまとまりとして並ぶ。
      burstCuts.value = cutsFromGroups(shown, Object.values(current.core.members).map(idsOf))
      const memberIds = new Set(idsOf(memberPaths))
      burstOriginal.value = shown.filter(id => memberIds.has(id))
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'まとめを読み込めませんでした。'
      burstPhotos.value = []
      burstCuts.value = []
    } finally {
      burstBusy.value = false
    }
  }

  /** いま画面に見えているまとまりの並び。 */
  const burstBlocks = computed(() =>
    blocksFromCuts(burstPhotos.value.map(photo => photo.id), burstCuts.value)
  )
  const burstPhotoOf = (photoId: string) => burstPhotos.value.find(photo => photo.id === photoId) ?? null
  /** その塊の代表。指名があればそれ、無ければ撮影順の先頭。 */
  const representativeOf = (block: string[]) =>
    block.find(id => burstReps.value.includes(id)) ?? block[0]!

  function toggleBurstPick(photoId: string) {
    burstPicked.value = burstPicked.value.includes(photoId)
      ? burstPicked.value.filter(id => id !== photoId)
      : [...burstPicked.value, photoId]
  }

  /** 選んだ写真を、連続した塊ごとに切り離す。 */
  function splitBurstSelection() {
    if (!burstPicked.value.length) return
    burstCuts.value = cutAroundSelection(
      burstPhotos.value.map(photo => photo.id), burstCuts.value, burstPicked.value
    )
    burstPicked.value = []
  }

  /** 境目をひとつ、切る／つなぐ（バーのタップ）。 */
  function toggleBurstCut(boundaryIndex: number) {
    burstCuts.value = toggleAt(burstCuts.value, boundaryIndex)
  }

  /** 切れている境目を別の境目へずらす（バーのドラッグ）。 */
  function moveBurstCut(from: number, to: number) {
    burstCuts.value = moveCut(burstCuts.value, from, to)
  }

  function scatterBurst() {
    burstCuts.value = cutAll(burstCuts.value)
    burstPicked.value = []
  }

  /**
   * まとめの中で 1 枚の星を決める。★5 の確定と、明らかな脱落（−1）。
   *
   * **人が星を直接決める手直し**なので `ratingEdit` で Session の星を書き換え、
   * 行へも写す。もう一度押したら取り消し（この回の星に戻す）。
   * 仲間の星は動かさない。core の確定は仲間の星を差分で動かすので、
   * ここで付けた差は次の確定でも保たれる。
   */
  async function settleBurstPhoto(photoId: string, decide: (star: number, target: number) => number) {
    const current = session.value
    const path = pathOf(photoId)
    if (!current || !path || !activeProject.value) return
    const star = current.core.ratings[path] ?? 0
    const next = setRating(current.core, path, decide(star, current.core.target_star))
    const photo = burstPhotoOf(photoId)
    if (photo) photo.rating = next.ratings[path] ?? star
    await applyCore(next)
    await saveSession()
  }

  const confirmBurstPhoto = (photoId: string) =>
    settleBurstPhoto(photoId, (star, target) => (star >= MAX_RATING ? target : MAX_RATING))
  const dropBurstPhoto = (photoId: string) =>
    settleBurstPhoto(photoId, (star, target) => (star < target ? target : star - 1))

  /**
   * 選んだ1枚をそのまとまりの代表に指名する。
   * **反映は「この形で戻る」のとき。** 途中で候補やグループを書き換えると、
   * そのあとの切り離しと噛み合わなくなる。
   */
  function makeBurstRepresentative(photoId: string) {
    const block = burstBlocks.value.find(ids => ids.includes(photoId))
    if (!block) return
    // 同じ塊の中の古い指名は外す。代表は塊に1枚。
    burstReps.value = [...burstReps.value.filter(id => !block.includes(id)), photoId]
    burstPicked.value = []
  }

  /**
   * 直した形を確定して選別画面へ戻す。
   *
   * 保存するのは**手直しそのもの**（基準との食い違いだけ）。何度押しても結果は同じ。
   * 反映は core の `regroup`（まだ判断していない写真だけを組み直す）。
   * 代表の指名は core の `setRepresentative` で。
   */
  async function applyBurstShape() {
    const current = session.value
    const project = activeProject.value
    if (!current || !project) return
    burstBusy.value = true
    try {
      const inputs = await ensureCoreInputs()
      const threshold = thresholdFor(currentDistance())
      const runRefs = burstPhotos.value.map(toPhotoRef)
      const blocks = burstBlocks.value.map(block => block.map(pathOf).filter((path): path is string => path !== null))
      const existing = await loadPairOverrides()
      const overrides = overridesFromShape(
        runRefs, blocks, existing,
        (left, right) => core.isSameBurst({ left, right }, threshold)
      )
      await desktop.savePairOverrides(project.id, overrides)
      noteJudgementChanged(project.id)
      setPairOverrides(overrides)

      let next = core.regroup(current.core, inputs.refs, current.settings.groupBursts, threshold, overrides)
      // 指名された代表を、組み直したあとのまとまりに当てる（当てられないものは無視）。
      for (const id of burstReps.value) {
        const wanted = pathOf(id)
        if (!wanted) continue
        const shown = Object.keys(next.members).find(key => next.members[key]?.includes(wanted))
        if (!shown || shown === wanted) continue
        next = core.setRepresentative(next, shown, wanted) ?? next
      }
      setCore(next)
      current.selectedInGroup = []

      burstDialog.value = false
      burstOwner.value = null
      await enterTournamentAfterRebuild()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'まとめの形を保存できませんでした。'
    } finally {
      burstBusy.value = false
    }
  }

  return {
    burstDialog,
    burstPhotos,
    burstCuts,
    burstOriginal,
    burstPicked,
    burstBusy,
    openBurst,
    burstBlocks,
    burstPhotoOf,
    representativeOf,
    toggleBurstPick,
    splitBurstSelection,
    toggleBurstCut,
    moveBurstCut,
    scatterBurst,
    confirmBurstPhoto,
    dropBurstPhoto,
    makeBurstRepresentative,
    applyBurstShape
  }
}
