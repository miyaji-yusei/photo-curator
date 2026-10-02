import type {
  AmazonPreview, BurstGroup, Photo, Project, ProjectProgress,
  SelectionResult, TournamentSettings
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import { previewRefreshPlan, withNewThumbnails } from '~/utils/previewRefresh'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, PhotoRef, Session } from '~/lib/core'
import {
  blocksFromCuts, cutAll, cutAroundSelection, cutsFromGroups, moveCut, toggleAt
} from '~/utils/burstEdit'
import { buildBurstQuestions } from '~/utils/burstQuestions'
import { burstNeighborhood } from '~/utils/burstNeighborhood'
import { joinSpanOverrides, overridesFromShape } from '~/utils/burstShape'
import { createSaveQueue } from '~/utils/saveQueue'
import { createSerialQueue } from '~/utils/serialQueue'
import {
  BURST_WINDOW_MS, D_HASH_VERSION, DEFAULT_BURST_DISTANCE,
  buildCoreInputs, maxNeighborDistance, toPhotoRef
} from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import { applyChanges, reviewChanges, setRating } from '~/utils/ratingEdit'
import { healRatings, syncRatings } from '~/utils/selectionFlow'
import { prepareProgress, projectStatus } from '~/utils/projectStatus'
import type { CardState, CardStatus, PrepareLine } from '~/utils/projectStatus'
import type { RatingChange, SavedSelection } from '~/utils/selectionFlow'
import { SLIDESHOW_GROUP_SIZE, clampGroupSize, groupSizeLimits, isSlideshowSize, tournamentGroupSize } from '~/utils/groupSize'
import { registerAutoPush, useSidecarSync } from '~/composables/useSidecarSync'
import type { ClashChoice } from '~/composables/useSidecarSync'
import type { View } from '~/composables/curator/types'
import { useExport } from '~/composables/curator/useExport'
import { useResults } from '~/composables/curator/useResults'
import { useMove } from '~/composables/curator/useMove'
import { useDisplayImages } from '~/composables/curator/useDisplayImages'
import { useZoom } from '~/composables/curator/useZoom'

/**
 * 行の星を**読む・消す・動かす**メソッド。選別の 1 タップは行の星の書き込みを待たずに次の組を出す（W1）ので、
 * これらは入っている書き込みが終わってから走らせる（自分の書き込みを読み損ねない）。
 * 行の星に触るメソッドを PhotoBackend に足したら、ここにも足すこと。`getPhotosByIds`・`saveSession` は
 * 1 タップの道なので入れない。
 */
const ROW_RATING_METHODS = new Set<string>([
  'getSelectionSummary', 'getCoreInputs', 'getProjectPhotoPage', 'getAnalysisBacklog',
  'resetSelectionResults', 'moveRating', 'exportPhotos', 'exportAmazon', 'writeRatingsToPhotos',
  'saveCsv', 'deleteProject'
])

function withRatingsBarrier(base: PhotoBackend, waitForWrites: () => Promise<void>): PhotoBackend {
  return new Proxy(base, {
    get(target, key, receiver) {
      const value = Reflect.get(target, key, receiver)
      if (typeof key !== 'string' || typeof value !== 'function' || !ROW_RATING_METHODS.has(key)) return value
      return async (...args: unknown[]) => {
        await waitForWrites()
        return (value as (...a: unknown[]) => unknown).apply(target, args)
      }
    }
  })
}

/** テストでは偽の `PhotoBackend` を渡す（既定は実行環境に合う実装）。 */
export function createCurator(backend: PhotoBackend = useDesktop()) {
  /**
   * 行の星の書き込みの直列キュー（W1）。1 タップごとの書き込み・集計の完了を待たずに次の組を出す。
   * 入れた順に 1 本ずつ書く（「1 つ戻す」が前の書き込みを追い越さない）。空になったら集計を 1 回だけ読み直す。
   */
  const ratingsQueue = createSerialQueue(
    (cause) => { error.value = cause instanceof Error ? cause.message : '選別結果を保存できませんでした。' },
    async () => {
      const project = activeProject.value
      if (!project) return
      try {
        // 自分の待ち行列の中なので、待たない素の backend を使う。
        const summary = await backend.getSelectionSummary(project.id)
        if (activeProject.value?.id === project.id) selectionSummary.value = summary
      } catch {
        selectionSummary.value = null
      }
    }
  )
  const desktop = withRatingsBarrier(backend, () => ratingsQueue.flush())
  const { notify } = useNotice()
  // サイドカー（写真のフォルダの `.photo-curator/catalog.json`）。開き方の判断は core の sidecarPlan が行う。
  // ほかの端末で切り替えた「同名の JPEG と RAW を 1 枚として扱う」を取り込んだら（U48）、画面のプロジェクトを
  // 読み直す（お知らせは useSidecarSync が出す。写真への反映は「写真を再読み込み」で、自動では走査しない）。
  const sidecar = useSidecarSync(desktop, {
    onSettingsAdopted: async (projectId) => {
      await refreshProjects()
      if (activeProject.value?.id === projectId) {
        activeProject.value = projects.value.find(item => item.id === projectId) ?? activeProject.value
      }
    }
  })
  const {
    access: sidecarAccess, clash: sidecarClash, busy: sidecarBusy,
    message: sidecarMessage, notice: sidecarNotice, detached: sidecarDetached, savedAt: sidecarSavedAt
  } = sidecar
  /** 写真の行がまだ無いまま開いたプロジェクト。走査が済んでから確かめる。 */
  let sidecarCheckPending: string | null = null
  let stopAutoPush: (() => void) | undefined
  let stopCloseListener: (() => void) | undefined
  const projects = ref<Project[]>([])
  /** ホームの行・サイドバーの点が使う、プロジェクトごとの状態と見本。端末にある値だけで決める。 */
  const projectCards = ref<Record<string, { status: CardStatus, thumbnailUrl: string | null }>>({})
  /** 開いているプロジェクトの、まだ解析が要る枚数（準備の進み）。 */
  const analysisBacklog = ref(0)
  const activeProject = ref<Project | null>(null)
  const previewPhotos = shallowRef<Photo[]>([])
  const previewTotal = ref(0)
  const tournamentPhotos = ref<Photo[]>([])
  /**
   * 選別の途中（封筒）。`core` が core の Session そのもの。
   * **core の Session は必ず丸ごと置き換える**（`setCore`）。中身を書き換えない。
   */
  const session = ref<SavedSelection | null>(null)
  // core に渡す全写真（撮影順）と、relativePath / id の対応表。星は持たない（古くなるので）。
  const coreInputs = shallowRef<CoreInputs | null>(null)
  let pairOverrides: PairOverride[] = []
  const view = ref<View>('home')
  const loading = ref(false)
  const error = ref('')
  /**
   * 選別の途中（封筒）の保存。**画面は待たない。** 1 本ずつ・未書き込みは最新の 1 つだけ。
   * 閉じる・ホームへ戻る・背面へ回る・窓を閉じるときは `flush` してからサイドカーを書く。
   */
  const saveQueue = createSaveQueue<SavedSelection>(
    (projectId, envelope) => desktop.saveSession(projectId, envelope),
    (cause) => { error.value = cause instanceof Error ? cause.message : '選別の途中を保存できませんでした。' }
  )
  /** 保存を書き終えてから、サイドカーに変更があれば書く（サイドカーは封筒も持つので順序が要る）。 */
  async function flushThenPush() {
    // 行の星の書き込みが先（サイドカーの「変わった」印も、その書き込みのあとに付く）。
    await ratingsQueue.flush()
    await saveQueue.flush()
    await sidecar.pushAuto(pushableProject())
  }
  /** 削除中のプロジェクトの id。待ち行列にも、サイドカーの書き込み・変更の印にも、触らせない。 */
  let deletingProjectId: string | null = null
  const pushableProject = () => {
    const project = activeProject.value
    return project && project.id !== deletingProjectId ? project : null
  }
  const createDialog = ref(false)
  /** 作成ダイアログのタブ。Amazon は `capabilities.amazon` が true のときだけ選べる。 */
  const createTab = ref<'folder' | 'amazon'>('folder')
  const amazonUrl = ref('')
  const amazonPreview = ref<AmazonPreview | null>(null)
  const amazonLoading = ref(false)
  const amazonError = ref('')
  const projectName = ref('')
  const folderPath = ref('')
  /** (開発用) フォルダの絶対パス。`pnpm dev` のときだけ作成ダイアログに出す。 */
  const devFolderPath = ref('')
  // 開発用の配信（server/api/dev-folder/）は、Nuxt の開発サーバーが動くブラウザ版でだけ使える。
  // `tauri dev` の画面では使えない（Rust に渡しても読めない）。
  const isDev = import.meta.dev && desktop.kind === 'local'
  const taskProgress = ref<ProjectProgress | null>(null)
  const taskWarning = ref<string | null>(null)
  const taskDialog = ref(false)
  // 連写解析は前面をブロックしない。ダイアログではなく帯で知らせるだけにする。
  const analysisProgress = ref<ProjectProgress | null>(null)
  const analysisFailures = ref(0)
  // 選別画面に出す表示用画像の設定（`composables/curator/useDisplayImages.ts`）。
  // 先読みの表は下で作るので、捨てる関数として渡す（呼ばれるのは作ったあと）。
  const {
    displaySettings, displayEdge, displayBacklog, displayBusy, largeDisplay, displayEdgeDialog, pendingDisplayEdge,
    refreshDisplayState, applyDisplayEdge, startDisplayAfterScan, requestDisplayEdge, confirmDisplayEdge,
    cancelDisplayEdge, regenerateDisplayImages
  } = useDisplayImages({ desktop, activeProject, error, taskWarning, clearPrefetched: () => prefetched.clear() })
  const pendingTournamentSettings = ref<TournamentSettings | null>(null)
  /**
   * 1 グループの枚数の既定と上限。デスクトップは 10 枚、iPad などブラウザは
   * 画面が狭く指で選ぶので既定 4 枚（2×2）・上限 9 枚（3×3）にする。
   */
  const groupLimits = groupSizeLimits(desktop.capabilities.largeGroups)
  const settings = reactive<TournamentSettings>({ groupSize: groupLimits.default, groupBursts: false })
  /** キーボードが無い環境ではショートカットの案内を出さない。 */
  const isTouchOnly = computed(() => !desktop.capabilities.keyboard)
  let stopProgressListener: (() => void) | undefined

  // 狭い画面ではドロワーを常設しない。
  // Vuetify の useDisplay() は、この構成（vite-plugin-vuetify + build.transpile）だと
  // plugin 側と別インスタンスを掴むことがあり、1265px でも mdAndUp が false になった。
  // 判定を matchMedia に任せて取り違えを避ける。CSS 側のブレークポイントとも揃う。
  const COMPACT_QUERY = '(max-width: 960px)'
  const isCompact = ref(false)
  const drawerOpen = ref(false)
  /** 広い画面でサイドバーをアイコンだけの細い状態に畳む。 */
  const drawerRail = ref(false)
  let compactQuery: MediaQueryList | undefined
  // permanent でも v-model が false だとドロワーは画面外へ寄ってしまう。
  // 幅に合わせて開閉状態も揃える。広い画面では開き、狭い画面では閉じて始める。
  const syncCompact = (event: MediaQueryList | MediaQueryListEvent) => {
    isCompact.value = event.matches
    drawerOpen.value = !event.matches
  }

  // 閾値の学習・確認まわり
  const pairPhotos = ref<Photo[]>([])
  const previewThreshold = ref(0)
  const previewBusy = ref(false)
  /** 確認画面に出す、まとまった連写（2 枚以上）。core の `groupBursts` の結果を写したもの。 */
  const burstPreviewGroups = ref<BurstGroup[]>([])
  const previewMaxDistance = ref(64)

  // 選別中の枚数変更とプロジェクト削除
  const groupSizeDialog = ref(false)
  /** 選別中の「？」ヘルプ（U24）。開いている間は、選別のキーを後ろの画面へ流さない。 */
  const helpDialog = ref(false)
  const pendingGroupSize = ref(10)
  /** 「選別中の設定」で選んでいる方式。true＝スライドショー（1 枚ずつ）、false＝トーナメント。 */
  const pendingSlideshow = ref(false)
  const deleteDialog = ref(false)
  const deleteTarget = ref<Project | null>(null)
  const deleteBusy = ref(false)

  /** 枚数に対する列数。1画面に収まりやすい並びを枚数ごとに決めてある。 */
  function columnsFor(count: number) {
    const byCount: Record<number, number> = { 1: 1, 2: 2, 3: 3, 4: 2, 5: 3, 6: 3, 7: 4, 8: 4, 9: 3, 10: 5 }
    return byCount[count] ?? Math.min(5, Math.max(1, Math.ceil(Math.sqrt(count))))
  }
  /** 写真を見比べている画面かどうか。余白の詰め方を変える。 */
  const isSelecting = computed(() =>
    view.value === 'tournament' || view.value === 'burst-threshold' || view.value === 'burst-review'
  )

  // 次ラウンドと選別結果ビュー
  const nextRoundDialog = ref(false)
  const nextRoundGroupSize = ref(10)
  const nextRoundRating = ref(0)
  const restartDialog = ref(false)
  /** 確認のダイアログが「選別を開始」から開かれたか（確定すると、やり直しではなく開始へ進む）。 */
  const restartForStart = ref(false)
  watch(restartDialog, open => { if (!open) restartForStart.value = false })
  const restartBusy = ref(false)

  // 拡大表示（`composables/curator/useZoom.ts`）
  const {
    zoomPhoto, zoomList, openZoom, zoomSrc, zoomError, zoomLoading, zoomIndex, stepZoom, onZoomKeydown
  } = useZoom(desktop)

  // 一覧の列数。`'auto'` は今までどおり画面幅にまかせる。
  // null ではなく文字列にしてあるのは、mandatory な v-btn-toggle が null を
  // 「未選択」と解釈して、勝手に先頭の 3 列へ寄せてしまうため。
  type GridDensity = 3 | 5 | 8 | 'auto'
  const densityOptions: { value: GridDensity, icon: string, label: string }[] = [
    { value: 3, icon: 'mdi-view-grid-outline', label: '3列' },
    { value: 5, icon: 'mdi-view-comfy-outline', label: '5列' },
    { value: 8, icon: 'mdi-view-module-outline', label: '8列' },
    { value: 'auto', icon: 'mdi-view-dashboard-variant-outline', label: '自動' }
  ]
  const previewDensity = ref<GridDensity>('auto')
  const resultsDensity = ref<GridDensity>('auto')
  const moveDensity = ref<GridDensity>('auto')
  const gridClass = (density: GridDensity) => (density === 'auto' ? '' : 'is-fixed')
  const gridStyle = (density: GridDensity) =>
    density === 'auto' ? undefined : { '--grid-columns': String(density) }
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

  // ---- core（判断）との橋 --------------------------------------------------
  //
  // 判断は全部 core（wasm）が行う。ここでは core に渡す・返ってきたものを画面と
  // 保存に写すだけ。写真の鍵は relativePath、画面の行（Photo）の id は uuid。

  /** core の Session。無ければ null。 */
  const coreSession = computed<Session | null>(() => session.value?.core ?? null)
  const pathOf = (photoId: string) => coreInputs.value?.byId.get(photoId)?.relativePath ?? null
  const idOf = (path: string) => coreInputs.value?.byPath.get(path)?.id ?? null
  const idsOf = (paths: string[]) => paths.map(idOf).filter((id): id is string => id !== null)

  /** 星が最大なら「確定」扱い。別のフラグは持たない。 */
  const isConfirmed = (photoId: string) => {
    const path = pathOf(photoId)
    return path !== null && (coreSession.value?.ratings[path] ?? 0) >= MAX_RATING
  }
  /** その写真が代表しているまとめの枚数。1 なら単独。 */
  const burstSizeOf = (photoId: string) => {
    const path = pathOf(photoId)
    return (path !== null ? coreSession.value?.members[path]?.length : undefined) ?? 1
  }

  /** 今の組（写真の id）。core の `current` を対応表で写したもの。 */
  const currentGroup = computed(() => idsOf(coreSession.value?.current ?? []))
  const targetStar = computed(() => coreSession.value?.target_star ?? 0)
  const roundNumber = computed(() => coreSession.value?.round ?? 1)
  /** スライドショー選別か。**方式は 1 グループの枚数（1 枚）で表す**（core の Session は同じ）。 */
  const isSlideshow = computed(() => isSlideshowSize(coreSession.value?.group_size ?? 0))
  const canUndo = computed(() => (coreSession.value?.history.length ?? 0) > 0)
  const survivorCount = computed(() => coreSession.value?.survivors.length ?? 0)
  const remainingGroups = computed(() => {
    const current = coreSession.value
    if (!current) return 0
    return (current.current.length ? 1 : 0) + Math.ceil(current.queue.length / Math.max(1, current.group_size))
  })
  const remainingPhotos = computed(() =>
    coreSession.value ? coreSession.value.current.length + coreSession.value.queue.length : 0
  )
  /** このラウンドで済んだ組の割合（%）。 */
  const roundProgress = computed(() => {
    const done = coreSession.value?.history.length ?? 0
    const total = done + remainingGroups.value
    return total ? (done / total) * 100 : 100
  })
  const selectedCount = computed(() => coreSession.value?.survivors.length ?? 0)
  // 連写の学習。質問と答えは封筒（`learning`）に持つので、リロードしても続きから。
  const askedCount = computed(() => session.value?.learning?.answers.length ?? 0)
  const learningPosition = computed(() => (session.value?.learning?.index ?? 0) + 1)
  const learningTotal = computed(() => session.value?.learning?.questions.length ?? 0)
  const currentQuestion = computed(() => {
    const learning = session.value?.learning
    return learning ? learning.questions[learning.index] ?? null : null
  })
  const currentPair = computed(() => {
    const question = currentQuestion.value
    if (!question) return null
    return { gapMs: Math.abs((question.left.captured_at ?? 0) - (question.right.captured_at ?? 0)) }
  })
  const burstGroupCount = computed(() => burstPreviewGroups.value.length)
  const groupedPhotoCount = computed(() =>
    burstPreviewGroups.value.reduce((total, group) => total + group.photoIds.length, 0)
  )
  const maxPairDistance = computed(() => Math.max(previewMaxDistance.value, previewThreshold.value))
  const progressValue = computed(() => {
    const progress = taskProgress.value
    return progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0
  })
  // 完了・中断・エラーの後も taskProgress は残る。それを見て「読み込み中」に
  // していたので、一度 scan するとボタンが回りっぱなしになっていた。
  const scanRunning = computed(() => {
    const progress = taskProgress.value
    return !!progress && progress.task === 'scan'
      && progress.phase !== 'complete' && progress.phase !== 'cancelled' && progress.phase !== 'error'
  })
  const analysisRunning = computed(() => {
    const progress = analysisProgress.value
    return !!progress && progress.phase !== 'complete' && progress.phase !== 'cancelled' && progress.phase !== 'error'
  })
  const analysisValue = computed(() => {
    const progress = analysisProgress.value
    return progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0
  })

  function formatDate(value: number) { return new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium' }).format(value) }
  function shortPath(path: string) { return path.length > 55 ? `…${path.slice(-54)}` : path }
  function fileName(path: string) { return path.split(/[\\/]/).filter(Boolean).at(-1) ?? '新しいプロジェクト' }
  function statusLabel(project: Project) {
    if (project.status === 'ready') return `${project.photoCount.toLocaleString()} 枚`
    if (project.status === 'scanning') return '読み込み中'
    if (project.status === 'missing' && project.sourceKind === 'amazon') return 'リンクが無効'
    return '未読み込み'
  }

  /** いま開いているプロジェクトは Amazon Photos の共有リンクか。 */
  const isAmazon = computed(() => activeProject.value?.sourceKind === 'amazon')

  async function refreshProjects() {
    // デスクトップは PC の DB、ブラウザは端末内の DB。どちらも一覧を返す。
    projects.value = await desktop.listProjects()
    void refreshProjectCards()
  }

  /** 開いたときの準備（解析・表示用画像）が始められなかったとき。黙らず、通知に出す。 */
  function warnPrepareNotStarted(what: string, cause: unknown) {
    console.warn(`${what}を始められませんでした`, cause)
    taskWarning.value = `${what}を始められませんでした。${cause instanceof Error ? cause.message : ''}`.trim()
  }

  let cardsToken = 0
  /** 各プロジェクトの状態と見本を読み直す（網へは行かない）。新しい呼び出しがあれば古い結果は捨てる。 */
  async function refreshProjectCards() {
    const token = ++cardsToken
    // 書き途中の選別を先に書き終える（行の状態は保存したものから読む）。
    await saveQueue.flush()
    const list = projects.value
    const entries = await Promise.all(list.map(async (project) => {
      try {
        // 状態を決める 4 つのどれかが読めなかったら、状態を作らない（0・null にして
        // 「準備完了」「選別なし」と誤表示しない）。見本だけは読めなくても状態に関係しない。
        const [analysis, display, saved, summary, first] = await Promise.all([
          desktop.getAnalysisBacklog(project.id),
          desktop.getDisplayBacklog(project.id),
          desktop.loadSession(project.id),
          desktop.getSelectionSummary(project.id),
          desktop.getProjectPhotoPage(project.id, 0, 1).catch(() => null)
        ])
        const status = projectStatus({
          project, analysisBacklog: analysis, displayBacklog: display,
          session: saved?.core ?? null,
          keptCount: summary ? summary.counts.slice(1).reduce((sum, count) => sum + count, 0) : 0
        })
        const photo = first?.photos[0]
        return [project.id, { status, thumbnailUrl: photo ? desktop.photoThumbnailUrl(photo) : null }] as const
      } catch (cause) {
        // 前に読めた値があればそのまま残す（無ければ状態の行を出さない）。
        console.warn('プロジェクトの状態を読めませんでした', project.id, cause)
        const previous = projectCards.value[project.id]
        return previous ? [project.id, previous] as const : null
      }
    }))
    if (token !== cardsToken) return
    projectCards.value = Object.fromEntries(entries.filter(entry => entry !== null))
  }

  /** 準備の進み 3 行。開いているプロジェクトの、走査の途中は走査の進みを使う。 */
  const prepareLines = computed<PrepareLine[]>(() => {
    const project = activeProject.value
    if (!project) return []
    const progress = taskProgress.value
    const scanning = scanRunning.value && progress?.projectId === project.id
    return prepareProgress({
      project: scanning ? { ...project, status: 'scanning', photoCount: progress!.processed } : project,
      analysisBacklog: analysisBacklog.value,
      displayBacklog: displayBacklog.value,
      session: null,
      keptCount: 0
    })
  })

  /** 準備の未処理の数を読み直す（進捗のイベントごとには 1 秒に 1 回まで）。 */
  let prepareCountsAt = 0
  async function refreshPrepareCounts(projectId: string, force = false) {
    const now = Date.now()
    if (!force && now - prepareCountsAt < 1000) return
    prepareCountsAt = now
    const [analysis, display] = await Promise.all([
      desktop.getAnalysisBacklog(projectId).catch(() => 0),
      desktop.getDisplayBacklog(projectId).catch(() => 0)
    ])
    if (activeProject.value?.id !== projectId) return
    analysisBacklog.value = analysis
    displayBacklog.value = display
  }

  const PREVIEW_PAGE = 120
  let previewMoreBusy = false
  /** 読み直し（先頭から）の世代。新しい読み直しが始まったら、古い読み直し・続きの応答は捨てる（W7）。 */
  let previewToken = 0
  /** 先頭から読み直す。すでにページ送りで読んだ分は、その数まで読み直す（準備の途中の更新でスクロールが戻らないように）。 */
  async function loadPreview(projectId: string) {
    const token = ++previewToken
    const page = await desktop.getProjectPhotoPage(projectId, 0, Math.max(PREVIEW_PAGE, previewPhotos.value.length))
    if (token !== previewToken) return
    if (activeProject.value && activeProject.value.id !== projectId) return
    previewPhotos.value = page.photos
    previewTotal.value = page.total
  }

  /**
   * 準備の途中の更新（W11）。サムネイルがまだ無い行だけを読み直して、その場で置き換える
   * （全件の読み直しは、準備が終わったときだけ）。読み込み中に一覧が読み直されたら捨てる。
   */
  async function refreshPreviewThumbnails(projectId: string) {
    const plan = previewRefreshPlan(previewPhotos.value, previewTotal.value, PREVIEW_PAGE)
    if (plan.kind === 'full') return loadPreview(projectId)
    if (!plan.ids.length) return
    const token = previewToken
    const fresh = await desktop.getPhotosByIds(projectId, plan.ids)
    if (token !== previewToken || activeProject.value?.id !== projectId) return
    const next = withNewThumbnails(previewPhotos.value, fresh)
    if (next) previewPhotos.value = next
  }

  /** 格子の末尾が見えたら次のページ（全部を見られる）。 */
  async function loadMorePreview() {
    const project = activeProject.value
    if (!project || previewMoreBusy || previewPhotos.value.length >= previewTotal.value) return
    previewMoreBusy = true
    const token = previewToken
    try {
      const page = await desktop.getProjectPhotoPage(project.id, previewPhotos.value.length, PREVIEW_PAGE)
      if (activeProject.value?.id !== project.id || token !== previewToken) return
      previewPhotos.value = [...previewPhotos.value, ...page.photos]
      previewTotal.value = page.total
    } catch {
      // 次のスクロールでもう一度読む。
    } finally {
      previewMoreBusy = false
    }
  }

  async function loadCurrentPhotos(ids = currentGroup.value) {
    if (!activeProject.value || !ids.length) {
      tournamentPhotos.value = []
      return
    }
    // 先読みで行を読んでいれば、それを使う（組が変わるたびに読み直さない）。
    const ahead = prefetched.get(ids.join('\u0000'))
    tournamentPhotos.value = ahead && ahead.projectId === activeProject.value.id
      ? ahead.photos
      : await desktop.getPhotosByIds(activeProject.value.id, ids)
    void prefetchNextGroups()
  }

  /**
   * 次の組の先読み。組を出したら、`queue` の先頭 `groupSize` 枚（まとまりは代表）の表示用画像を
   * 読んで `decode()` しておく（Tauri は同期の URL、Web は blob の URL）。最大 2 組分。古いものは捨てる。
   * `Image` を持ち続けることでデコード済みの絵を手放さない。
   */
  const PREFETCH_GROUPS = 2
  type Prefetched = { projectId: string, photos: Photo[], images: HTMLImageElement[] }
  const prefetched = new Map<string, Prefetched>()
  /** 読み込み中の先読み。連打で `loadCurrentPhotos` が続けて呼ばれても、同じ組を重複して読まない。 */
  const prefetching = new Map<string, Promise<Prefetched>>()
  function readAhead(projectId: string, key: string, ids: string[]): Promise<Prefetched> {
    const flightKey = `${projectId}\u0001${key}`
    const running = prefetching.get(flightKey)
    if (running) return running
    const started = (async () => {
      const photos = await desktop.getPhotosByIds(projectId, ids)
      const images = photos.map((photo) => {
        const image = new Image()
        image.decoding = 'async'
        image.src = desktop.photoDisplayUrl(photo)
        image.decode().catch(() => undefined)
        return image
      })
      return { projectId, photos, images }
    })().finally(() => prefetching.delete(flightKey))
    prefetching.set(flightKey, started)
    return started
  }
  async function prefetchNextGroups() {
    const current = session.value
    const project = activeProject.value
    if (!current || !project || current.core.finished || typeof Image === 'undefined') {
      prefetched.clear()
      return
    }
    const size = Math.max(1, current.core.group_size)
    const groups = Array.from({ length: PREFETCH_GROUPS }, (_, index) =>
      idsOf(current.core.queue.slice(index * size, (index + 1) * size))
    ).filter(ids => ids.length > 0)
    const wanted = new Set(groups.map(ids => ids.join('\u0000')))
    for (const key of [...prefetched.keys()]) {
      if (!wanted.has(key) || prefetched.get(key)!.projectId !== project.id) prefetched.delete(key)
    }
    // 2 組は並べて読む。待っている間に組が進んだ・プロジェクトが変わったら、その結果は捨てる。
    await Promise.all(groups.map(async (ids) => {
      const key = ids.join('\u0000')
      if (prefetched.has(key)) return
      try {
        const entry = await readAhead(project.id, key, ids)
        if (activeProject.value?.id !== project.id || session.value !== current) return
        prefetched.set(key, entry)
      } catch {
        // 先読みは無くても困らない。
      }
    }))
  }

  /**
   * core に渡す全写真を取り直す。**その星に関係なく全件を 1 回で**。
   * 星は持たない（選別中に古くなる）。星が要るときは呼び出し側が行から読む。
   */
  async function loadCoreInputs(projectId = activeProject.value?.id) {
    if (!projectId) return null
    const inputs = buildCoreInputs(await desktop.getCoreInputs(projectId))
    if (activeProject.value?.id === projectId) coreInputs.value = inputs
    return inputs
  }

  /** 対応表が無ければ（リロード直後など）読む。 */
  async function ensureCoreInputs(): Promise<CoreInputs> {
    const known = coreInputs.value
    if (known) return known
    const loaded = await loadCoreInputs()
    if (!loaded) throw new Error('プロジェクトが開かれていません。')
    return loaded
  }

  async function loadPairOverrides() {
    pairOverrides = activeProject.value ? await desktop.getPairOverrides(activeProject.value.id) : []
    return pairOverrides
  }

  /** 連写の基準。学習した距離（この選別 → プロジェクト）→ 既定の順。 */
  const currentDistance = () =>
    session.value?.burstDistance ?? activeProject.value?.burstThreshold ?? DEFAULT_BURST_DISTANCE
  const thresholdFor = (distance: number): BurstThreshold => ({
    window_ms: BURST_WINDOW_MS, distance, d_hash_version: D_HASH_VERSION
  })

  /** core の Session を丸ごと置き換える。中身は変えない（返ってきた値をそのまま持つ）。 */
  function setCore(next: Session) {
    if (session.value) session.value.core = markRaw(next)
  }

  /** 判断（星・連写の手直し・学習した距離・やり直し）が変わった印。準備（ハッシュ値・画像）では呼ばない。 */
  function noteJudgementChanged(projectId = activeProject.value?.id) {
    if (projectId && projectId !== deletingProjectId) sidecar.markChanged(projectId)
  }

  /**
   * 前後の星を比べ、変わった写真の行だけ書く。`undo` も同じ。書き込みは直列キューに入れる。
   * `wait` が true（既定）なら、書き込みと集計の読み直しが終わるまで待つ。選別の 1 タップ
   * （`advanceWith`・`undoChoice`）だけ false で、待たずに次の組を出す。
   */
  async function writeRatings(changes: RatingChange[], wait = true) {
    const projectId = activeProject.value?.id
    if (!projectId || !changes.length) return
    const entries: SelectionResult[] = []
    for (const change of changes) {
      const id = idOf(change.relativePath)
      if (id) entries.push({ id, rating: change.rating })
    }
    if (!entries.length) return
    ratingsQueue.enqueue(async () => {
      await backend.saveSelectionResults(projectId, entries)
      noteJudgementChanged(projectId)
    })
    if (wait) await ratingsQueue.flush()
  }

  /** core の新しい Session を受け取り、星を行にも写す。`wait` は `writeRatings` と同じ。 */
  async function applyCore(next: Session, wait = true) {
    const previous = session.value?.core ?? null
    setCore(next)
    // Session を先に待ち行列へ入れる。行の星の書き込みの途中で終了しても、Session は新しい組になる
    // （逆順だと、行の星だけ進んで Session が前の組のまま残り、次に開いて同じ組を確定すると +1 が重なる）。
    saveSession()
    await writeRatings(syncRatings(previous, next), wait)
  }

  /**
   * 星を行の値から入れた Session。行が星の持ち主（結果・移動・書き出しが読む）なので、
   * 人が星を手直しするときはここを土台にする。Session が無ければ空の土台。
   */
  async function sessionWithRowRatings(): Promise<{ base: Session, inputs: CoreInputs, rows: Photo[] }> {
    const projectId = activeProject.value?.id
    if (!projectId) throw new Error('プロジェクトが開かれていません。')
    const rows = await desktop.getCoreInputs(projectId)
    const inputs = buildCoreInputs(rows)
    if (activeProject.value?.id === projectId) coreInputs.value = inputs
    const held = session.value?.core
      ?? core.startRound([], clampGroupSize(settings.groupSize, groupLimits), 0, false, thresholdFor(currentDistance()), [])
    const base = applyChanges(held, Object.fromEntries(rows.map(row => [row.relativePath, row.rating])))
    return { base: markRaw(base), inputs, rows }
  }

  /**
   * 開いたとき、行の星が Session とずれていたら Session に合わせる（自己修復）。
   * 組を確定した直後の強制終了で、行の星と封筒が食い違ったまま残るのを直す。
   */
  async function healRowRatings(projectId: string) {
    const current = session.value?.core
    if (!current) return
    try {
      const entries = healRatings(await desktop.getCoreInputs(projectId), current)
      if (!entries.length) return
      await desktop.saveSelectionResults(projectId, entries)
      noteJudgementChanged(projectId)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '星を Session に合わせられませんでした。'
    }
  }

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
    pairOverrides = []
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

  /** 開く処理の世代。新しい呼び出しが来たら古い呼び出しは、以降の結果を捨てて終わる（W6）。 */
  let openToken = 0
  async function openProject(project: Project) {
    const token = ++openToken
    const stale = () => token !== openToken
    activeProject.value = project
    previewPhotos.value = []
    previewTotal.value = 0
    analysisBacklog.value = 0
    prefetched.clear()
    coreInputs.value = null
    pairOverrides = []
    view.value = 'project'
    loading.value = true
    sidecarCheckPending = null
    try {
      // 前に開いていたプロジェクトの書き途中を、読む前に書き終える。
      await saveQueue.flush()
      if (stale()) return
      // プレビュー格子は星を出さないのでサイドカーの結果に依存しない。確認と並べて読み始める（W9）。
      // サイドカーが取り込んだときは reloadAfterSidecar がもう一度読むので、最後は新しい値になる。
      const preview = loadPreview(project.id)
      preview.catch(() => undefined) // 待つのは下。ここでは未処理の拒否にしない
      // 記録の取り込みは、選別の途中を読み込む前に済ませる（取り込んだ分が画面に出るように）。
      // 写真の行がまだ無いときは、走査のあとで確かめる（星を写す行が要る）。
      if (project.photoCount > 0) await runSidecarCheck(project)
      else {
        sidecarCheckPending = project.id
        await sidecar.refreshAccess(project.id)
      }
      if (stale()) return
      await Promise.all([
        preview,
        desktop.loadSession(project.id).then(value => {
          // 遅れて届いた前のプロジェクトの封筒で、今のプロジェクトの session を上書きしない。
          if (stale()) return
          if (value) value.core = markRaw(value.core)
          session.value = value
        })
      ])
      if (stale()) return
      await healRowRatings(project.id)
      if (stale()) return
      // 開いた時点から少しずつ解析を進めておく。「選別を開始」で待たされないように。
      // ただし**やることが無いなら起動しない**。以前は無条件に呼んでいたため、
      // 解析済みのプロジェクトを開くたびに進捗イベントだけが飛び、解析中の帯が
      // 一瞬表示されていた。
      // まだ一枚も読み込んでいないプロジェクトは、開いた時点で読み込みを始める。
      // 利用者がボタンを押すのを待つ理由が無い。
      //
      // **フォルダを走査できる環境だけ。** ブラウザには走査するフォルダが無く、
      // `startProjectScan` は何もしないので、進捗イベントも来ない。それを待つ
      // ダイアログが閉じられなくなり、リロードしないと戻れなくなっていた。
      // フォルダの許可が切れているときは、ここで求めても通らない（許可は利用者の操作の中でだけ）。
      // プロジェクトの画面の「フォルダへのアクセスを許可」を押してもらう。
      // リンクが消えた Amazon のプロジェクトは、開くたびに読みにいかない（「写真を再読み込み」で試す）。
      const linkGone = project.sourceKind === 'amazon' && project.status === 'missing'
      if (!project.photoCount && !importsByPicker.value && !scanRunning.value && project.folderAccess !== 'needs-permission' && !linkGone) {
        await startScan()
        return
      }
      if (linkGone) {
        await loadSummary()
        return
      }
      // 未解析数・表示用の状態・集計は互いに依存しないので、並べて読む（W9）。
      const [backlog] = await Promise.all([
        desktop.getAnalysisBacklog(project.id).catch(() => 0),
        refreshDisplayState(),
        loadSummary()
      ])
      if (stale()) return
      analysisBacklog.value = backlog
      if (backlog > 0) desktop.startBackgroundAnalysis(project.id).catch(cause => warnPrepareNotStarted('解析', cause))
      // 表示用画像は走査とは別に溜める。**走査に混ぜると解析が桁で遅くなる**
      // （EXIF サムネイル経路 1.72ms/枚 に対しフルデコード 132ms/枚）。
      if (displayBacklog.value > 0) {
        desktop.startDisplayGeneration(project.id).catch(cause => warnPrepareNotStarted('表示用画像の作成', cause))
      }
    } catch (cause) {
      if (!stale()) error.value = cause instanceof Error ? cause.message : 'プロジェクトを開けませんでした。'
    } finally {
      // 古い呼び出しが、新しい呼び出しの「読み込み中」を落とさない。
      if (!stale()) loading.value = false
    }
  }

  async function chooseFolder() {
    try {
      const selected = await desktop.chooseFolder()
      if (selected) folderPath.value = selected
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'フォルダを選択できませんでした。'
    }
  }

  /**
   * 写真ライブラリから取り込める環境か。ブラウザはフォルダを走査できないので、
   * 代わりに写真ピッカーから受け取る。
   */
  const canImportPhotos = computed(() => typeof desktop.importPhotos === 'function')
  /**
   * このプロジェクトは写真ピッカーで取り込むか。ブラウザでも、フォルダ（File System Access・
   * 開発用）のプロジェクトはフォルダから読むので、デスクトップと同じく「再読み込み」になる。
   */
  const importsByPicker = computed(() =>
    canImportPhotos.value && (activeProject.value?.source ?? 'picker') === 'picker')
  const photoInput = ref<HTMLInputElement | null>(null)

  function openPhotoPicker() {
    // 同じ写真を選び直せるよう、開く前に値を捨てる。
    if (photoInput.value) photoInput.value.value = ''
    photoInput.value?.click()
  }

  /** ピッカーで選ばれた写真を取り込み、そのまま解析まで進める。 */
  async function onPhotoPicked(event: Event) {
    const input = event.target as HTMLInputElement
    const files = Array.from(input.files ?? [])
    if (!files.length || !activeProject.value || !desktop.importPhotos) return
    taskWarning.value = null
    taskProgress.value = {
      projectId: activeProject.value.id, task: 'scan', phase: 'indexing',
      processed: 0, total: files.length, message: '写真を読み込んでいます…', warning: null, failed: 0
    }
    taskDialog.value = true
    try {
      await desktop.importPhotos(activeProject.value.id, files)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '写真を取り込めませんでした。'
    } finally {
      taskDialog.value = false
      await refreshProjects()
      activeProject.value = projects.value.find(item => item.id === activeProject.value?.id) ?? activeProject.value
      if (activeProject.value) {
        await loadPreview(activeProject.value.id)
        // 写真が増えたので core に渡す列も取り直す。
        await loadCoreInputs().catch(() => undefined)
      }
    }
  }

  /** リンクを読み込み、名前・枚数・見本を出す。 */
  async function loadAmazonPreview() {
    const url = amazonUrl.value.trim()
    if (!url || !desktop.amazonPreview || amazonLoading.value) return
    amazonLoading.value = true
    amazonError.value = ''
    amazonPreview.value = null
    try {
      const preview = await desktop.amazonPreview(url)
      amazonPreview.value = preview
      // 名前は共有の名前を初期値にする（あとで直せる）。
      if (!projectName.value.trim()) projectName.value = preview.name
    } catch (cause) {
      amazonError.value = cause instanceof Error ? cause.message : 'Amazon Photos のリンクを読めませんでした。'
    } finally {
      amazonLoading.value = false
    }
  }

  function resetAmazonDraft() {
    amazonUrl.value = ''
    amazonPreview.value = null
    amazonError.value = ''
    createTab.value = 'folder'
  }

  // リンクを書き換えたら、前の読み込みの結果は古い。
  watch(amazonUrl, () => {
    amazonPreview.value = null
    amazonError.value = ''
  })
  watch(createDialog, open => {
    if (!open) resetAmazonDraft()
    else {
      createPairRaw.value = true
      void loadCreateDisplay()
    }
  })

  // 作成ダイアログの「ファイル名が同じ JPEG と RAW を 1 枚の写真として扱う」。既定はオン（U46）。
  const createPairRaw = ref(true)

  // 作成ダイアログの「表示用画像の大きさ」。既定はアプリの設定の値。
  const createDisplayChoices = ref<number[]>([])
  const createDisplayEdge = ref(0)
  async function loadCreateDisplay() {
    try {
      const settings = await desktop.getDisplaySettings()
      createDisplayChoices.value = settings.choices
      createDisplayEdge.value = settings.edge
    } catch {
      // 読めなければ選択欄を出さず、あとから設定で変えられる。
      createDisplayChoices.value = []
    }
  }

  // アプリの設定の画面。表示用画像の既定（これから作るプロジェクトの分）。
  const appDisplayChoices = ref<number[]>([])
  const appDisplayEdge = ref(0)
  async function loadAppSettings() {
    try {
      const settings = await desktop.getDisplaySettings()
      appDisplayChoices.value = settings.choices
      appDisplayEdge.value = settings.edge
    } catch {
      appDisplayChoices.value = []
    }
  }
  /** 選んだらその場で既定に保存する。既存のプロジェクトの表示用画像は作り直さない。 */
  async function saveAppDisplayEdge(edge: number) {
    const previous = appDisplayEdge.value
    appDisplayEdge.value = edge
    try {
      appDisplayEdge.value = await desktop.saveDisplayEdge(edge)
    } catch (caught) {
      appDisplayEdge.value = previous
      notify(caught instanceof Error ? caught.message : '設定を保存できませんでした')
    }
  }
  watch(view, next => { if (next === 'app-settings') void loadAppSettings() })

  /**
   * 作成した直後、**準備（表示用画像づくり）を始める前**に、ダイアログで選んだ長辺を
   * このプロジェクトに書く（生成はこの値で行う）。**アプリの既定は変えない**
   * （既定はダイアログの初期値にだけ効く。変えるのはアプリの設定の画面）。
   */
  async function applyCreateDisplayEdge(projectId: string) {
    const edge = createDisplayEdge.value
    if (!edge || !createDisplayChoices.value.length) return
    try {
      await desktop.saveProjectDisplayEdge(projectId, edge)
    } catch (cause) {
      // 作成は続ける（既定の大きさで作られる）が、黙らずに知らせる。
      error.value = cause instanceof Error ? cause.message : '表示用画像の大きさを保存できませんでした。既定の大きさで作ります。'
    }
  }

  /** 作成した直後、走査を始める前に、ダイアログで選んだ「同名の JPEG と RAW」の設定をこのプロジェクトに書く。既定（オン）のときは書かない。 */
  async function applyCreatePairRaw(projectId: string) {
    if (createPairRaw.value) return
    try {
      await desktop.saveProjectPairRaw(projectId, false)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '設定を保存できませんでした。同名の JPEG と RAW をまとめる既定の設定で読み込みます。'
    }
  }

  /**
   * このプロジェクトの「同名の JPEG と RAW を 1 枚の写真として扱う」を切り替える。
   * 反映は次の走査から。変えたらその旨を知らせる（自動では走査しない）。
   * 切り替えた時刻も保存し（backend）、ほかの端末へ届くようサイドカーにも書く（U48。いつもの自動の書き込み
   * ＝区切りで書くのと同じ経路。選別状況が同じなら設定だけを書く。書けなければ次の区切りで書く）。
   */
  async function setPairRawJpeg(enabled: boolean) {
    const project = activeProject.value
    if (!project || project.pairRawJpeg === enabled) return
    try {
      await desktop.saveProjectPairRaw(project.id, enabled)
      await refreshProjects()
      activeProject.value = projects.value.find(item => item.id === project.id) ?? activeProject.value
      notify('設定を変えました。写真に反映するには「写真を再読み込み」を押してください。')
      void flushThenPush().catch(() => undefined)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '設定を保存できませんでした。'
    }
  }

  /** Amazon のプロジェクトを作る。作ると走査が始まる（`openProject` が読み込みを始める）。 */
  async function createAmazonProject() {
    const preview = amazonPreview.value
    if (!preview || !desktop.createAmazonProject) return
    loading.value = true
    try {
      const project = await desktop.createAmazonProject(projectName.value.trim() || preview.name, amazonUrl.value.trim())
      await applyCreateDisplayEdge(project.id)
      createDialog.value = false
      projectName.value = ''
      await refreshProjects()
      await openProject(project)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを作成できませんでした。'
    } finally {
      loading.value = false
    }
  }

  async function createProject() {
    if (createTab.value === 'amazon' && desktop.capabilities.amazon) {
      await createAmazonProject()
      return
    }
    // フォルダを選べないブラウザでは、名前だけ決めて作り、続けて写真を選ばせる。
    // フォルダ（選んだもの・開発用の絶対パス）があれば、そこから読む。
    const devPath = isDev ? devFolderPath.value.trim() : ''
    if (!canImportPhotos.value && !folderPath.value) return
    loading.value = true
    try {
      const fallbackName = devPath ? fileName(devPath) : folderPath.value ? fileName(folderPath.value) : '新しいプロジェクト'
      const project = await desktop.createProject(
        projectName.value.trim() || fallbackName, devPath ? `dev:${devPath}` : folderPath.value
      )
      await applyCreateDisplayEdge(project.id)
      await applyCreatePairRaw(project.id)
      createDialog.value = false
      projectName.value = ''
      folderPath.value = ''
      devFolderPath.value = ''
      await refreshProjects()
      await openProject(project)
      // ここで写真ピッカーを自動で開かない。iOS はファイル選択を
      // **利用者の操作の流れの中でしか**許さず、`await` を挟んだあとの
      // `click()` は黙って無視される。開いたつもりで何も起きない状態になるので、
      // プロジェクト画面の「写真を追加」を押してもらう形にしてある。
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを作成できませんでした。'
    } finally {
      loading.value = false
    }
  }

  async function startScan() {
    if (!activeProject.value || taskDialog.value) return
    // 走査するフォルダを持たないプロジェクト（ブラウザのピッカー）では、待つべき進捗が
    // 一度も発生しない。ダイアログを出すと閉じる手立てが無くなるので、始めない。
    // ピッカーでの取り込みは `openPhotoPicker` が入口。
    if (importsByPicker.value) return
    taskProgress.value = { projectId: activeProject.value.id, task: 'scan', phase: 'indexing', processed: 0, total: 0, message: '写真フォルダを確認しています…', warning: null, failed: 0 }
    taskWarning.value = null
    taskDialog.value = true
    try {
      await desktop.startProjectScan(activeProject.value.id)
    } catch (cause) {
      taskDialog.value = false
      error.value = cause instanceof Error ? cause.message : '写真の読み込みを開始できませんでした。'
    }
  }

  function enterMethod() {
    if (!activeProject.value?.photoCount || sidecarClash.value) return
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
    if (!activeProject.value || taskDialog.value || sidecarClash.value) return
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

  /** 確認の「星を全部消してやり直す」。開始の確認から開いていたら、閉じて開始へ進む。 */
  async function confirmRestartDialog() {
    if (!restartForStart.value) return restartFromScratch()
    restartDialog.value = false
    // 星を全部消して始め直すのも「やり直し」。ほかの端末に黙って埋め戻されないよう世代を変える。
    if (activeProject.value) await sidecar.markRestarted(activeProject.value.id)
    await startTournament()
  }

  async function startTournament() {
    if (!activeProject.value || taskDialog.value || sidecarClash.value) return
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
        thresholdFor(learned ?? DEFAULT_BURST_DISTANCE), pairOverrides
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
      const groups = core.groupBursts(inputs.refs, thresholdFor(distance), pairOverrides)
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
    setCore(core.regroup(current.core, inputs.refs, true, thresholdFor(distance), pairOverrides))
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

  /**
   * 選別の途中を保存する。**待たない**（待ち行列に入れて戻る）。入れる時点の写しを渡すので、
   * 書くまでの間に画面が封筒を書き換えても、この時点の状態が書かれる（core の Session は丸ごと置き換える決め）。
   */
  function saveSession() {
    const current = session.value
    const project = activeProject.value
    if (!current || !project || project.id === deletingProjectId) return
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
      pairOverrides = overrides

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
      setCore(core.regroup(current.core, inputs.refs, on, thresholdFor(currentDistance()), pairOverrides))
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
      pairOverrides = merged
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
        chosen.groupBursts, thresholdFor(distance), pairOverrides
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

  /**
   * 星を全部 0 に戻して最初からやり直す。解析結果（ハッシュ値・サムネイル）は消えない。
   * 消す判断の中身（星・連写の手直し・学習した距離・選別の途中）は、ここ 1 か所に集める。
   */
  async function restartFromScratch() {
    const project = activeProject.value
    if (!project) return
    restartBusy.value = true
    try {
      await desktop.resetSelectionResults(project.id)
      // 連写の手直しと学習した距離も消す（消さないと、やり直しても学習の質問から始まらない）。
      await desktop.savePairOverrides(project.id, [])
      pairOverrides = []
      await desktop.clearBurstThreshold(project.id)
      project.burstThreshold = null
      noteJudgementChanged()
      // やり直しの世代を変える（ほかの端末は、黙って空にせず確認する。設計書 §4.3 の #9）。
      await sidecar.markRestarted(project.id)
      session.value = null
      // 途中の選別も消す。残すと、リロードで星の無い写真に古い Session が戻る。
      saveQueue.enqueue(project.id, null)
      await refreshProjects()
      await loadSummary()
      restartDialog.value = false
      view.value = 'project'
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'やり直せませんでした。'
    } finally {
      restartBusy.value = false
    }
  }

  // ---- 選別結果の一覧と集計（`composables/curator/useResults.ts`） ----
  // 関数より前で使う名前（`loadSummary`・`selectionSummary` など）も、使うのは呼ばれたとき（作るときではない）。
  const {
    resultsPhotos, resultsTotal, resultsOffset, resultsBusy, resultsRating, resultsSort,
    selectionSummary, hasSelectionData, ratingCount,
    openResults, returnToResults, resultsTiles, loadMoreResults, loadSummary, selectResultsRating, loadResultsPage
  } = useResults({ desktop, activeProject, coreSession, view, error })

  // ---- 書き出し・共有・CSV・メタデータ（`composables/curator/useExport.ts`） ----
  const {
    exportDialog, exportMode, exportDestination, exportRatings, exportBusy, exportResult, exportError,
    metadataError, metadataDialog, metadataRatings, metadataBusy, metadataAcknowledged, metadataResult,
    shareDialog, shareRatings, shareBusy, shareError, shareMessage,
    chooseExportDestination, runExport, runMetadataWrite, exportResultsCsv, exportPreviewCount,
    exportMoveConfirm, requestExport, shareSelectedPhotos, exportZipByRating, exportCsvByRating, openShareDialog
  } = useExport({
    desktop, activeProject, coreSession, coreInputs, view, error, isAmazon, resultsRating, notify,
    refreshProjects, loadSummary, loadResultsPage
  })

  // ---- 連写の見直し --------------------------------------------------------

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

  // ---- レートの移動（`composables/curator/useMove.ts`） ----
  const {
    moveDialog, moveFrom, moveTo, moveBusy, movePhotos, moveTotal, moveOffset, moveError,
    moveSelectedCount, isMoveSelected, openMoveDialog, loadMovePage, toggleMoveSelection, setMoveSelectAll, runMove
  } = useMove({
    desktop, activeProject, session, view, error, notify, ensureCoreInputs, pathOf, setCore, saveSession,
    noteJudgementChanged, loadSummary, loadResultsPage
  })

  async function resumeSession() {
    // 別の端末の記録との食い違いを選ぶまで、選別は始めさせない。
    if (sidecarClash.value) return
    if (!session.value) return openSettings()
    // 別の環境で作られたセッションは、この端末の上限を超える枚数を持ちうる。
    const clamped = clampGroupSize(session.value.settings.groupSize, groupLimits)
    session.value.settings.groupSize = clamped
    if (session.value.core.group_size !== clamped) setCore(core.resize(session.value.core, clamped))
    await enterStage()
  }

  /** ホームの行の「次の一手」。プロジェクトを開いてから、状態に合う画面へ進む。 */
  async function openProjectAction(project: Project, state: CardState) {
    await openProject(project)
    if (error.value || activeProject.value?.id !== project.id) return
    if (state === 'error') await startScan()
    else if (state === 'culling') await resumeSession()
    else if (state === 'done') await openResults()
    else enterMethod()
  }

  function askDeleteProject(project: Project) {
    deleteTarget.value = project
    deleteDialog.value = true
  }

  async function confirmDeleteProject() {
    const target = deleteTarget.value
    if (!target) return
    deleteBusy.value = true
    deletingProjectId = target.id
    try {
      // 消す前に、そのプロジェクトの未書き込みの封筒を捨てる（消したあとに書かれないように）。
      await saveQueue.drop(target.id)
      await desktop.deleteProject(target.id)
      if (activeProject.value?.id === target.id) {
        activeProject.value = null
        coreInputs.value = null
        session.value = null
        previewPhotos.value = []
        tournamentPhotos.value = []
        view.value = 'home'
      }
      await refreshProjects()
      deleteDialog.value = false
      deleteTarget.value = null
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを削除できませんでした。'
    } finally {
      deletingProjectId = null
      deleteBusy.value = false
    }
  }

  function openGroupSizeDialog() {
    const size = clampGroupSize(session.value?.settings.groupSize ?? groupLimits.default, groupLimits)
    pendingSlideshow.value = isSlideshowSize(size)
    pendingGroupSize.value = tournamentGroupSize(size, groupLimits)
    groupSizeDialog.value = true
  }

  async function cancelTask() {
    const progress = taskProgress.value
    // ダイアログは**自分で閉じる**。以前は進捗イベントの `cancelled` が来るのを
    // 待っていたため、イベントを出さないバックエンドでは閉じる手段が無くなり、
    // リロードするしか戻れなかった。取り消しが届いたかに関係なく閉じてよい
    // （完了済みの読み込み結果は残る）。
    taskDialog.value = false
    if (!progress) return
    try {
      await desktop.cancelProjectTask(progress.projectId, progress.task)
    } catch {
      // 取り消せなくても画面は閉じる。走っているぶんはそのまま終わる。
    }
  }

  async function cancelAnalysis() {
    const progress = analysisProgress.value
    if (!progress) return
    try {
      await desktop.cancelProjectTask(progress.projectId, progress.task)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '中止できませんでした。'
    }
  }

  function onKeydown(event: KeyboardEvent) {
    // 拡大中は拡大のキーだけ。選択には流さない。
    if (zoomPhoto.value) {
      onZoomKeydown(event)
      return
    }
    if (event.target instanceof HTMLInputElement) return
    // ヘルプを開いている間は、選別のキー（数字・Enter・Backspace など）を後ろの画面へ流さない。
    if (helpDialog.value) return

    // 連写の見直し。**セッションが無くても開ける**画面なので、下の session 判定より
    // 手前で拾う。操作は選別画面と同じ（数字で選ぶ／Ctrl+数字で拡大／Enter で確定）。
    if (view.value === 'burst-review') {
      if (event.key === 'Enter') {
        event.preventDefault()
        if (burstReviewKept.value.length) void applyBurstReview()
        return
      }
      const digit = /^(Digit|Numpad)(\d)$/.exec(event.code)
      const index = digit ? (Number(digit[2]) === 0 ? 10 : Number(digit[2])) : 0
      const target = burstReviewPhotos.value[index - 1] ?? null
      if (!target) return
      event.preventDefault()
      if (event.ctrlKey || event.metaKey) openZoom(target, burstReviewPhotos.value)
      else toggleBurstReviewKeep(target.id)
      return
    }

    if (!session.value) return

    // 閾値の判定画面。テンポよく答えられるよう手を離さずに済ませる。
    if (view.value === 'burst-threshold' && currentPair.value) {
      // ボタンの並び（左から）と数字を一致させる。1=判断できない 2=別々 3=まとめる
      const key = event.key
      if (key === '1') { event.preventDefault(); void skipCurrentPair(); return }
      if (key === '2') { event.preventDefault(); void answerPair(false); return }
      if (key === '3') { event.preventDefault(); void answerPair(true); return }
      return
    }

    if (view.value !== 'tournament') return

    // 選び間違えたときに1手戻す。
    if (event.key === 'Backspace') {
      event.preventDefault()
      void undoChoice()
      return
    }
    // スライドショーは 1 枚ずつ。Enter・M・Space は使わず（Enter で「落とす」が走らないように）、
    // 1・3・5 と ←・→・↑ は画面（SlideshowView）が受ける（2・4・↓ は何もしない）。修飾キー付き（Ctrl＝拡大など）は下の共通処理へ。
    if (isSlideshow.value) {
      if (event.key === 'Enter' || event.key === ' ' || event.key.toLowerCase() === 'm') {
        event.preventDefault()
        return
      }
      if (!event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey && /^(Digit|Numpad)\d$/.test(event.code)) return
    }
    // Enter は「今の選択で確定」。1枚も選んでいなければ「どれも選ばない」になる。
    if (event.key === 'Enter') {
      event.preventDefault()
      void confirmChoices()
      return
    }
    // 複数枚選択のトグルは M でもスペースでも。
    if (event.key.toLowerCase() === 'm' || event.key === ' ') {
      event.preventDefault()
      session.value.multiSelect = !session.value.multiSelect
      session.value.selectedInGroup = []
      void saveSession()
      return
    }

    // 数字キーは修飾キーで役割を変える。
    //   そのまま … 選ぶ / Ctrl … 拡大 / Shift … 確定 / Alt … まとめを開く
    // Shift+数字 は event.key が記号になるため、物理キー(code)から番号を取る。
    const fromCode = /^(Digit|Numpad)(\d)$/.exec(event.code)
    const raw = fromCode ? Number(fromCode[2]) : Number(event.key)
    const number = raw === 0 ? 10 : raw
    if (!Number.isInteger(number) || number < 1 || number > tournamentPhotos.value.length) return
    const photo = tournamentPhotos.value[number - 1] ?? null
    if (!photo) return

    event.preventDefault()
    if (event.ctrlKey || event.metaKey) openZoom(photo, tournamentPhotos.value)
    else if (event.shiftKey) void confirmPhoto(photo.id)
    else if (event.altKey) void openBurst(photo)
    else void toggleChoice(photo.id)
  }

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

  // プロジェクトを閉じる（ホームへ戻る）とき、変更があれば書く。
  watch(view, (next, previous) => {
    if (next === 'home' && previous !== 'home') void flushThenPush()
    // 選別・結果からプロジェクトの画面へ戻ったら、保存を書き終えてから確かめる（設計書 §4.5）。
    // ラウンドの終わりの書き込みで食い違いが見つかっていれば、ここでダイアログになる。
    if (next === 'project' && (previous === 'tournament' || previous === 'result')) {
      const project = pushableProject()
      if (project && !sidecarClash.value) void syncAtBreak(project).catch(() => undefined)
    }
    // 選別のあとに戻ったとき、行の状態・点を今の値にする。
    // ただし home -> project（プロジェクトを開く）は読み直さない。開く処理が同じ DB を使っている最中で、
    // 点・見本は直前のホームの値のまま使える（W4）。
    if ((next === 'home' || next === 'project') && previous !== next && !(previous === 'home' && next === 'project')) {
      void refreshProjectCards()
    }
  })

  async function mount() {
    try {
      // 判断（core）は wasm。最初に 1 回だけ読み込む。
      await core.init()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '判断の部品を読み込めませんでした。'
    }
    try {
      // 保存領域を開けないとき（別の版が残っている・別のタブが使っている など）は、
      // その理由と次にすることの文がここに来る。ホームの上の赤い帯に出す。
      await refreshProjects()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを読み込めませんでした。'
    }
    try {
      stopProgressListener = await desktop.onProjectProgress(async (progress) => {
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
          if (sidecarCheckPending === progress.projectId && activeProject.value) {
            // 写真の行ができたので、開いたときの確認をここで行う（取り込んだ星を行へ写せる）。
            sidecarCheckPending = null
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
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '進み具合を受け取れませんでした。'
    }
    // 背面へ回る・窓を閉じるときに書く。ホームへ戻るときは `view` の監視で書く。
    stopAutoPush = registerAutoPush(
      () => sidecar.pushAuto(pushableProject()),
      async () => { await ratingsQueue.flush(); await saveQueue.flush() }
    )
    // Tauri の窓を閉じるとき: 保存とサイドカーの書き込みを待ってから閉じる（待ちすぎないよう 5 秒で切る）。
    if (desktop.kind === 'tauri') {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const win = getCurrentWindow()
        stopCloseListener = await win.onCloseRequested(async (event) => {
          event.preventDefault()
          try {
            await Promise.race([flushThenPush(), new Promise(resolve => setTimeout(resolve, 5000))])
          } finally {
            await win.destroy()
          }
        })
      } catch {
        // 窓の口が無い環境（テスト・Web）では登録しない。
      }
    }
    window.addEventListener('keydown', onKeydown)
    compactQuery = window.matchMedia(COMPACT_QUERY)
    syncCompact(compactQuery)
    compactQuery.addEventListener('change', syncCompact)
  }

  function unmount() {
    stopProgressListener?.()
    stopAutoPush?.()
    stopCloseListener?.()
    window.removeEventListener('keydown', onKeydown)
    compactQuery?.removeEventListener('change', syncCompact)
  }

  return {
    MAX_RATING,
    desktop,
    projects,
    projectCards,
    openProjectAction,
    prepareLines,
    activeProject,
    previewPhotos,
    previewTotal,
    loadMorePreview,
    tournamentPhotos,
    session,
    view,
    loading,
    error,
    createDialog,
    createDisplayChoices,
    createDisplayEdge,
    createPairRaw,
    appDisplayChoices,
    appDisplayEdge,
    saveAppDisplayEdge,
    createTab,
    amazonUrl,
    amazonPreview,
    amazonLoading,
    amazonError,
    loadAmazonPreview,
    isAmazon,
    zoomSrc,
    zoomError,
    zoomLoading,
    projectName,
    folderPath,
    taskProgress,
    taskWarning,
    taskDialog,
    analysisProgress,
    analysisFailures,
    displaySettings,
    displayEdge,
    displayBacklog,
    displayBusy,
    displayEdgeDialog,
    pendingDisplayEdge,
    requestDisplayEdge,
    confirmDisplayEdge,
    cancelDisplayEdge,
    largeDisplay,
    pendingTournamentSettings,
    groupLimits,
    settings,
    isTouchOnly,
    isCompact,
    drawerOpen,
    drawerRail,
    currentPair,
    pairPhotos,
    previewThreshold,
    previewBusy,
    groupSizeDialog,
    helpDialog,
    pendingGroupSize,
    pendingSlideshow,
    isSlideshow,
    decideSlide,
    deleteDialog,
    deleteTarget,
    deleteBusy,
    isSelecting,
    nextRoundDialog,
    nextRoundGroupSize,
    nextRoundRating,
    restartDialog,
    confirmRestartDialog,
    restartBusy,
    resultsPhotos,
    resultsTotal,
    resultsOffset,
    resultsBusy,
    resultsRating,
    resultsSort,
    selectionSummary,
    hasSelectionData,
    ratingCount,
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
    exportDialog,
    exportMode,
    exportDestination,
    exportRatings,
    exportBusy,
    exportResult,
    exportError,
    metadataError,
    metadataDialog,
    metadataRatings,
    metadataBusy,
    metadataAcknowledged,
    metadataResult,
    shareDialog,
    shareRatings,
    shareBusy,
    shareError,
    shareMessage,
    zoomPhoto,
    zoomList,
    densityOptions,
    previewDensity,
    resultsDensity,
    moveDensity,
    gridClass,
    gridStyle,
    burstDialog,
    burstPhotos,
    burstCuts,
    burstOriginal,
    burstPicked,
    burstBusy,
    burstReviewGroups,
    burstReviewIndex,
    burstReviewPhotos,
    burstReviewKept,
    burstReviewBusy,
    burstReviewLoaded,
    burstReviewColumns,
    burstReviewRows,
    isConfirmed,
    burstSizeOf,
    currentGroup,
    remainingGroups,
    remainingPhotos,
    selectedCount,
    askedCount,
    learningPosition,
    learningTotal,
    burstGroupCount,
    targetStar,
    roundNumber,
    canUndo,
    survivorCount,
    roundProgress,
    groupedPhotoCount,
    maxPairDistance,
    progressValue,
    scanRunning,
    analysisRunning,
    analysisValue,
    formatDate,
    shortPath,
    fileName,
    statusLabel,
    loadPreview,
    loadCurrentPhotos,
    openProject,
    chooseFolder,
    canImportPhotos,
    importsByPicker,
    devFolderPath,
    isDev,
    photoInput,
    openPhotoPicker,
    onPhotoPicked,
    createProject,
    startScan,
    enterMethod,
    openSettings,
    openSlideshowSettings,
    beginTournament,
    finishTournamentStart,
    answerPair,
    skipCurrentPair,
    refreshBurstPreview,
    askMorePairs,
    acceptBurstThreshold,
    relearnThreshold,
    saveSession,
    toggleChoice,
    confirmChoices,
    confirmPhoto,
    openZoom,
    zoomIndex,
    stepZoom,
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
    applyBurstShape,
    undoChoice,
    applyGroupSize,
    applyGroupBursts,
    groupSelectedAsBurst,
    openNextRoundDialog,
    startRatingSelection,
    chooseExportDestination,
    runExport,
    runMetadataWrite,
    openResults,
    loadSummary,
    selectResultsRating,
    loadResultsPage,
    openBurstReview,
    toggleBurstReviewKeep,
    applyBurstReview,
    skipBurstReview,
    applyDisplayEdge,
    setPairRawJpeg,
    regenerateDisplayImages,
    openMoveDialog,
    loadMovePage,
    toggleMoveSelection,
    setMoveSelectAll,
    runMove,
    shareSelectedPhotos,
    exportZipByRating,
    exportCsvByRating,
    exportResultsCsv,
    resultsTiles,
    loadMoreResults,
    returnToResults,
    exportPreviewCount,
    exportMoveConfirm,
    requestExport,
    openShareDialog,
    resumeSession,
    sidecarAccess,
    sidecarClash,
    sidecarBusy,
    sidecarMessage,
    sidecarSavedAt,
    sidecarNotice,
    sidecarDetached,
    saveSidecarNow,
    writeSidecarToNas,
    resolveSidecarClash,
    askDeleteProject,
    confirmDeleteProject,
    openGroupSizeDialog,
    cancelTask,
    cancelAnalysis,
    onKeydown,
    mount,
    unmount
  }
}

let instance: ReturnType<typeof createCurator> | null = null

export function useCurator() {
  return (instance ??= createCurator())
}
