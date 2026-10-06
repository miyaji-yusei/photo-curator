import type {
  AnalysisFailure, BurstGroup, Photo, Project, ProjectProgress,
  SelectionResult, TournamentSettings
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import * as core from '~/lib/core'
import type { BurstThreshold, PairOverride, PhotoRef, Session } from '~/lib/core'
import { createSaveQueue } from '~/utils/saveQueue'
import { createSerialQueue } from '~/utils/serialQueue'
import {
  BURST_WINDOW_MS, D_HASH_VERSION, DEFAULT_BURST_DISTANCE,
  buildCoreInputs
} from '~/utils/coreInputs'
import type { CoreInputs } from '~/utils/coreInputs'
import { applyChanges } from '~/utils/ratingEdit'
import { healRatings, syncRatings } from '~/utils/selectionFlow'
import type { CardStatus } from '~/utils/projectStatus'
import type { RatingChange, SavedSelection } from '~/utils/selectionFlow'
import { clampGroupSize, groupSizeLimits, isSlideshowSize, tournamentGroupSize } from '~/utils/groupSize'
import { registerAutoPush, useSidecarSync } from '~/composables/useSidecarSync'
import type { View } from '~/composables/curator/types'
import { useExport } from '~/composables/curator/useExport'
import { useResults } from '~/composables/curator/useResults'
import { useMove } from '~/composables/curator/useMove'
import { useDisplayImages } from '~/composables/curator/useDisplayImages'
import { useZoom } from '~/composables/curator/useZoom'
import { useProjectCreate } from '~/composables/curator/useProjectCreate'
import { useBurstReview } from '~/composables/curator/useBurstReview'
import { useBurstEdit } from '~/composables/curator/useBurstEdit'
import { usePreviewGrid } from '~/composables/curator/usePreviewGrid'
import { useSidecarActions } from '~/composables/curator/useSidecarActions'
import { useKeyboard } from '~/composables/curator/useKeyboard'
import { useSelectionStart } from '~/composables/curator/useSelectionStart'
import { useBurstLearning } from '~/composables/curator/useBurstLearning'
import { useProjectOpen } from '~/composables/curator/useProjectOpen'
import { useTournamentActions } from '~/composables/curator/useTournamentActions'
import { useProgressEvents } from '~/composables/curator/useProgressEvents'

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
    access: sidecarAccess, clash: sidecarClash, busy: sidecarBusy, checking: sidecarChecking,
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
  // プロジェクトの画面の格子（`composables/curator/usePreviewGrid.ts`）
  const { previewPhotos, previewTotal, loadPreview, refreshPreviewThumbnails, loadMorePreview } = usePreviewGrid(desktop, activeProject)
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
  const taskProgress = ref<ProjectProgress | null>(null)
  const taskWarning = ref<string | null>(null)
  const taskDialog = ref(false)
  // 連写解析は前面をブロックしない。ダイアログではなく帯で知らせるだけにする。
  const analysisProgress = ref<ProjectProgress | null>(null)
  const analysisFailures = ref(0)
  /** 解析できなかった写真の一覧（名前・理由・種類）。警告の「一覧を見る」で出す（U58）。 */
  const analysisFailureList = ref<AnalysisFailure[]>([])
  const analysisFailuresDialog = ref(false)

  /**
   * 解析できなかった写真を DB から読み直す。**件数はここで決める**（進みのイベントだけに頼ると、
   * プロジェクトを替えても前の件数が残った。B6）。
   */
  async function refreshAnalysisFailures(projectId = activeProject.value?.id) {
    if (!projectId) return
    try {
      const list = (await desktop.getAnalysisFailures(projectId)) ?? []
      if (activeProject.value?.id !== projectId) return
      analysisFailureList.value = list
      analysisFailures.value = list.length
    } catch {
      // 一覧が読めなくても、選別も解析も止めない。
    }
  }
  async function openAnalysisFailures() {
    await refreshAnalysisFailures()
    analysisFailuresDialog.value = true
  }
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

  // ---- 開く・カード・準備の数・削除（`composables/curator/useProjectOpen.ts`） ----
  // 再代入される `let`（`openToken`・`sidecarCheckPending`・`deletingProjectId`・`pairOverrides`）は関数で渡す。
  // あとで作られるもの（`loadSummary`・`runSidecarCheck`・`enterMethod` など）は呼ぶ関数として渡す（呼ばれるのは作ったあと）。
  /** 開く処理の世代。新しい呼び出しが来たら古い呼び出しは、以降の結果を捨てて終わる（W6）。 */
  let openToken = 0
  const {
    refreshProjects, refreshProjectCards, prepareLines, refreshPrepareCounts, openProject, openProjectAction,
    askDeleteProject, confirmDeleteProject
  } = useProjectOpen({
    desktop, projects, projectCards, activeProject, session, view, loading, error, coreInputs, previewPhotos,
    previewTotal, tournamentPhotos, taskProgress, taskWarning, scanRunning, analysisBacklog, displayBacklog,
    analysisFailures, analysisFailureList, analysisFailuresDialog, analysisProgress, deleteTarget, deleteDialog,
    deleteBusy, saveQueue, sidecar,
    nextOpenToken: () => ++openToken,
    currentOpenToken: () => openToken,
    setSidecarCheckPending: (projectId) => { sidecarCheckPending = projectId },
    setDeletingProjectId: (projectId) => { deletingProjectId = projectId },
    setPairOverrides: (overrides) => { pairOverrides = overrides },
    clearPrefetched: () => prefetched.clear(),
    importsByPicker: () => importsByPicker.value,
    loadPreview, loadSummary: () => loadSummary(), refreshDisplayState, refreshAnalysisFailures,
    runSidecarCheck: (project) => runSidecarCheck(project), healRowRatings,
    startScan: () => startScan(), enterMethod: () => enterMethod(), resumeSession: () => resumeSession(),
    openResults: () => openResults()
  })

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

  // ---- サイドカーを呼ぶ側の薄い層（`composables/curator/useSidecarActions.ts`） ----
  // `loadSummary` は下（useResults）で作るので、呼ぶ関数として渡す（呼ばれるのは作ったあと）。
  const {
    runSidecarCheck, syncAtBreak, reloadAfterSidecar, resolveSidecarClash, saveSidecarNow, writeSidecarToNas
  } = useSidecarActions({
    desktop, sidecar, sidecarClash, ratingsQueue, saveQueue, projects, activeProject, session, coreInputs,
    setPairOverrides: (overrides) => { pairOverrides = overrides },
    refreshProjects, loadPreview, loadSummary: () => loadSummary()
  })

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

  // ---- 作成ダイアログ・Amazon のリンク・アプリの設定（`composables/curator/useProjectCreate.ts`） ----
  const {
    createDialog, createTab, amazonUrl, amazonPreview, amazonLoading, amazonError, projectName, folderPath,
    devFolderPath, isDev, chooseFolder, loadAmazonPreview, createPairRaw, createDisplayChoices, createDisplayEdge,
    appDisplayChoices, appDisplayEdge, saveAppDisplayEdge, createProject
  } = useProjectCreate({
    desktop, view, loading, error, notify, canImportPhotos, fileName, refreshProjects, openProject
  })

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

  /** 確認の「星を全部消してやり直す」。開始の確認から開いていたら、閉じて開始へ進む。 */
  async function confirmRestartDialog() {
    if (!restartForStart.value) return restartFromScratch()
    restartDialog.value = false
    // 星を全部消して始め直すのも「やり直し」。ほかの端末に黙って埋め戻されないよう世代を変える。
    if (activeProject.value) await sidecar.markRestarted(activeProject.value.id)
    await startTournament()
  }

  // ---- 選別中の操作（`composables/curator/useTournamentActions.ts`） ----
  // あとで作られるもの（`enterTournamentAfterRebuild`・`openSelection`・`enterStage`）は呼ぶ関数として渡す（呼ばれるのは作ったあと）。
  // 再代入される `let`（`deletingProjectId`・`pairOverrides`）は関数で渡す。
  const {
    saveSession, toggleChoice, confirmChoices, decideSlide, confirmPhoto, undoChoice, applyGroupSize,
    applyGroupBursts, groupSelectedAsBurst, openNextRoundDialog, startRatingSelection
  } = useTournamentActions({
    desktop, activeProject, session, view, loading, error, settings, groupLimits, groupSizeDialog,
    nextRoundDialog, nextRoundGroupSize, nextRoundRating, saveQueue, ratingsQueue, pathOf,
    getDeletingProjectId: () => deletingProjectId,
    getPairOverrides: () => pairOverrides,
    setPairOverrides: (overrides) => { pairOverrides = overrides },
    applyCore, setCore, loadCurrentPhotos, loadCoreInputs, ensureCoreInputs, loadPairOverrides,
    sessionWithRowRatings, thresholdFor, currentDistance, noteJudgementChanged, flushThenPush,
    enterTournamentAfterRebuild: () => enterTournamentAfterRebuild(),
    openSelection: (initial, chosen, inputs) => openSelection(initial, chosen, inputs),
    enterStage: () => enterStage()
  })

  // ---- 連写の学習・確認（`composables/curator/useBurstLearning.ts`） ----
  const {
    showNextPair, answerPair, skipCurrentPair, finishThresholdLearning, refreshBurstPreview, toViewGroup,
    askMorePairs, acceptBurstThreshold, enterTournamentAfterRebuild, relearnThreshold
  } = useBurstLearning({
    desktop, activeProject, session, view, error, pairPhotos, previewThreshold, previewBusy, previewMaxDistance,
    burstPreviewGroups, currentQuestion, idsOf, thresholdFor, ensureCoreInputs, loadPairOverrides,
    getPairOverrides: () => pairOverrides,
    noteJudgementChanged, refreshProjects, flushThenPush, setCore, saveSession, loadCurrentPhotos
  })

  // ---- まとめの中身を直す画面（`composables/curator/useBurstEdit.ts`） ----
  const {
    burstDialog, burstPhotos, burstCuts, burstOriginal, burstPicked, burstBusy, openBurst, burstBlocks, burstPhotoOf,
    representativeOf, toggleBurstPick, splitBurstSelection, toggleBurstCut, moveBurstCut, scatterBurst,
    confirmBurstPhoto, dropBurstPhoto, makeBurstRepresentative, applyBurstShape
  } = useBurstEdit({
    desktop, activeProject, session, error, burstSizeOf, idsOf, pathOf, thresholdFor, currentDistance,
    ensureCoreInputs, loadPairOverrides, setPairOverrides: (overrides) => { pairOverrides = overrides },
    setCore, applyCore, saveSession, noteJudgementChanged, enterTournamentAfterRebuild
  })

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

  // ---- 連写の見直し（`composables/curator/useBurstReview.ts`） ----
  const {
    burstReviewGroups, burstReviewIndex, burstReviewPhotos, burstReviewKept, burstReviewBusy, burstReviewLoaded,
    burstReviewColumns, burstReviewRows, openBurstReview, toggleBurstReviewKeep, applyBurstReview, skipBurstReview
  } = useBurstReview({
    desktop, activeProject, session, view, error, idOf, thresholdFor, currentDistance, toViewGroup,
    loadCoreInputs, loadPairOverrides, sessionWithRowRatings, setCore, saveSession, writeRatings,
    loadSummary, returnToResults
  })

  // ---- レートの移動（`composables/curator/useMove.ts`） ----
  const {
    moveDialog, moveFrom, moveTo, moveBusy, movePhotos, moveTotal, moveOffset, moveError,
    moveSelectedCount, isMoveSelected, openMoveDialog, loadMovePage, toggleMoveSelection, setMoveSelectAll, runMove
  } = useMove({
    desktop, activeProject, session, view, error, notify, ensureCoreInputs, pathOf, setCore, saveSession,
    noteJudgementChanged, loadSummary, loadResultsPage
  })

  // ---- 選別の開始（`composables/curator/useSelectionStart.ts`） ----
  // `sidecarClash` などは上で作った状態、`loadSummary`・`hasSelectionData` は useResults が作る。
  const {
    enterMethod, openSlideshowSettings, openSettings, beginTournament, startTournament,
    finishTournamentStart, openSelection, enterStage, resumeSession
  } = useSelectionStart({
    desktop, activeProject, session, view, loading, error, settings, groupLimits, taskDialog, taskWarning,
    restartDialog, restartForStart, pendingTournamentSettings, sidecarClash, sidecarChecking, hasSelectionData,
    syncAtBreak, loadSummary, loadCoreInputs, ensureCoreInputs, loadPairOverrides,
    getPairOverrides: () => pairOverrides,
    thresholdFor, currentDistance, noteJudgementChanged, setCore, saveSession, loadCurrentPhotos,
    showNextPair, refreshBurstPreview
  })

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

  // ---- 選別画面のキー操作（`composables/curator/useKeyboard.ts`） ----
  const { onKeydown } = useKeyboard({
    view, session, helpDialog, zoomPhoto, tournamentPhotos, burstReviewPhotos, burstReviewKept, currentPair,
    isSlideshow, onZoomKeydown, openZoom, openBurst, applyBurstReview, toggleBurstReviewKeep, skipCurrentPair,
    answerPair, undoChoice, confirmChoices, confirmPhoto, toggleChoice, saveSession
  })

  // ---- 進みのイベントの受け取り（`composables/curator/useProgressEvents.ts`） ----
  // `sidecarCheckPending` は開く処理（useProjectOpen）と共有するので、ここ（useCurator）に置いて関数で渡す。
  const { listenProgress } = useProgressEvents({
    desktop, projects, activeProject, view, error, taskProgress, taskWarning, taskDialog, analysisProgress,
    analysisFailures, sidecar,
    getSidecarCheckPending: () => sidecarCheckPending,
    setSidecarCheckPending: (projectId) => { sidecarCheckPending = projectId },
    refreshProjects, refreshAnalysisFailures, refreshPrepareCounts, loadPreview, refreshPreviewThumbnails,
    loadCoreInputs, startDisplayAfterScan, runSidecarCheck, reloadAfterSidecar
  })

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
      stopProgressListener = await listenProgress()
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
    analysisFailureList,
    analysisFailuresDialog,
    openAnalysisFailures,
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
    sidecarChecking,
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
