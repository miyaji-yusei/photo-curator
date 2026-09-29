import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoSort, Project, ProjectProgress,
  SelectionResult, SelectionSession, SelectionSummary, TournamentSettings
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { DisplaySettings } from '~/composables/photoBackend'
import type { MoveSelection } from '~/utils/ratingMove'
// `selectedCount` は選別画面側の computed と名前がぶつかるので別名にする。
import {
  createMoveSelection, isSelected as isMovePicked, selectedCount as countMoveSelection,
  setSelectAll, toMoveArgs, toggleSelection
} from '~/utils/ratingMove'
import {
  collapseBursts,
  insertIntoUpcoming,
  makeSession,
  prepareRound,
  regroupRemaining,
  resolveChosen,
  reviewBurstRatings,
  spreadBurstRatings,
  undoLastStep
} from '~/utils/tournament'
import {
  blocksFromCuts, cutAll, cutAroundSelection, cutsFromGroups, joinAt
} from '~/utils/burstEdit'
import { clampGroupSize, groupSizeLimits } from '~/utils/groupSize'
import {
  SHARE_FILE_LIMIT, downloadBlob, shareFiles, zipEntriesByRating
} from '~/utils/shareExport'
import { createStoredZip } from '~/utils/zip'
import {
  DEFAULT_THRESHOLD_OPTIONS,
  MAX_HASH_DISTANCE,
  applyAnswer,
  inferThreshold,
  nextPair,
  shouldStop,
  skipPair
} from '~/utils/burstThreshold'

type View = 'home' | 'project' | 'method' | 'settings' | 'burst-threshold' | 'burst-preview' | 'tournament' | 'result' | 'results' | 'burst-review'

function createCurator() {
  const desktop = useDesktop()
  const projects = ref<Project[]>([])
  const activeProject = ref<Project | null>(null)
  const previewPhotos = ref<Photo[]>([])
  const previewTotal = ref(0)
  const tournamentPhotos = ref<Photo[]>([])
  const session = ref<SelectionSession | null>(null)
  const view = ref<View>('home')
  const loading = ref(false)
  const error = ref('')
  const createDialog = ref(false)
  const projectName = ref('')
  const folderPath = ref('')
  const taskProgress = ref<ProjectProgress | null>(null)
  const taskWarning = ref<string | null>(null)
  const taskDialog = ref(false)
  // 連写解析は前面をブロックしない。ダイアログではなく帯で知らせるだけにする。
  const analysisProgress = ref<ProjectProgress | null>(null)
  const analysisFailures = ref(0)
  // 選別画面に出す表示用画像の設定。既定は全体、プロジェクトごとに上書きできる。
  const displaySettings = ref<DisplaySettings | null>(null)
  /** このプロジェクトで実際に使う長辺。 */
  const displayEdge = ref(0)
  const displayBacklog = ref(0)
  const displayBusy = ref(false)
  /** 「大きな画像で選別する」= 大きい方の長辺を選んでいるか。 */
  const largeDisplay = computed({
    get: () => displayEdge.value >= (displaySettings.value?.largeEdge ?? 1536),
    set: (on: boolean) => {
      const settings = displaySettings.value
      if (settings) void applyDisplayEdge(on ? settings.largeEdge : settings.defaultEdge)
    }
  })
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
  const currentPair = ref<BurstPair | null>(null)
  const pairPhotos = ref<Photo[]>([])
  const previewThreshold = ref(0)
  const previewBusy = ref(false)

  // 選別中の枚数変更とプロジェクト削除
  const groupSizeDialog = ref(false)
  const pendingGroupSize = ref(10)
  const deleteDialog = ref(false)
  const deleteTarget = ref<Project | null>(null)
  const deleteBusy = ref(false)

  /** 枚数に対する列数。1画面に収まりやすい並びを枚数ごとに決めてある。 */
  function columnsFor(count: number) {
    const byCount: Record<number, number> = { 1: 1, 2: 2, 3: 3, 4: 2, 5: 3, 6: 3, 7: 4, 8: 4, 9: 3, 10: 5 }
    return byCount[count] ?? Math.min(5, Math.max(1, Math.ceil(Math.sqrt(count))))
  }
  const tournamentColumns = computed(() => columnsFor(tournamentPhotos.value.length))
  const tournamentRows = computed(() =>
    Math.max(1, Math.ceil(tournamentPhotos.value.length / tournamentColumns.value))
  )
  /** 写真を見比べている画面かどうか。余白の詰め方を変える。 */
  const isSelecting = computed(() =>
    view.value === 'tournament' || view.value === 'burst-threshold' || view.value === 'burst-review'
  )

  // 次ラウンドと選別結果ビュー
  const nextRoundDialog = ref(false)
  const nextRoundGroupSize = ref(10)
  const nextRoundRating = ref(0)
  const restartDialog = ref(false)
  const restartBusy = ref(false)
  const resultsPhotos = ref<Photo[]>([])
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

  // レートの移動
  const moveDialog = ref(false)
  const moveFrom = ref(0)
  const moveTo = ref(0)
  const moveBusy = ref(false)
  const movePhotos = ref<Photo[]>([])
  const moveTotal = ref(0)
  const moveOffset = ref(0)
  /** ダイアログ内に出すエラー。画面上部に出すとモーダルに隠れて気づけない。 */
  const moveError = ref('')
  /** 移動が終わったことを画面上部で知らせる。 */
  const moveReport = ref('')
  /**
   * 既定は「全選択」。個別のチェックは**ここからの差分**だけを持つ。
   * 5,000 枚の id を並べて持たないための形。詳細は `utils/ratingMove.ts`。
   */
  const moveSelection = ref<MoveSelection>(createMoveSelection())
  const moveSelectedCount = computed(() => countMoveSelection(moveSelection.value, moveTotal.value))
  const isMoveSelected = (photoId: string) => isMovePicked(moveSelection.value, photoId)

  // 書き出し
  const exportDialog = ref(false)
  const exportMode = ref<'copy' | 'move'>('copy')
  const exportDestination = ref('')
  const exportRatings = ref<number[]>([5, 4, 3, 2, 1])
  const exportBusy = ref(false)
  const exportResult = ref<ExportReport | null>(null)
  /** ダイアログ内に出すエラー。画面上部に出すとモーダルに隠れて気づけない。 */
  const exportError = ref('')
  const metadataError = ref('')
  const metadataDialog = ref(false)
  const metadataRatings = ref<number[]>([5, 4, 3, 2, 1, 0])
  const metadataBusy = ref(false)
  const metadataAcknowledged = ref(false)
  const metadataResult = ref<ExportReport | null>(null)

  // ブラウザからライブラリへ渡す出口（共有シート / 星ごとの ZIP）。
  //
  // Shortcuts でアルバムに入れる経路も試したが、写真ライブラリを名前で辿る手立てが
  // 実機に無く（「写真を検索」に相当するアクションが見当たらず、写真アプリの
  // 「検索」はファイル名で検索できない）、成立しないので取り下げた。
  // 星はこのアプリが持ち続け、写真そのものは共有シートか ZIP で渡す。
  const shareDialog = ref(false)
  const shareRatings = ref<number[]>([MAX_RATING])
  const shareBusy = ref(false)
  const shareError = ref('')
  const shareMessage = ref('')

  // 拡大表示・まとめの展開
  const zoomPhoto = ref<Photo | null>(null)
  /** 拡大中に ← → で辿れる一覧。開いた場所に並んでいた写真をそのまま入れる。 */
  const zoomList = ref<Photo[]>([])

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

  /** 星が最大なら「確定」扱い。別のフラグは持たない。 */
  const isConfirmed = (photoId: string) => (session.value?.ratings[photoId] ?? 0) >= MAX_RATING
  /** その写真が代表しているまとめの枚数。1 なら単独。 */
  const burstSizeOf = (photoId: string) => session.value?.burstMembers[photoId]?.length ?? 1

  const currentGroup = computed(() => session.value?.groups[session.value.groupIndex] ?? [])
  const remainingGroups = computed(() => Math.max(0, (session.value?.groups.length ?? 0) - (session.value?.groupIndex ?? 0)))
  const remainingPhotos = computed(() => session.value ? session.value.groups.slice(session.value.groupIndex).reduce((total, group) => total + group.length, 0) : 0)
  const selectedCount = computed(() => session.value?.survivors.length ?? 0)
  const askedCount = computed(() => session.value?.burstThresholdState.answers.length ?? 0)
  const maxQuestions = DEFAULT_THRESHOLD_OPTIONS.maxQuestions
  const groupedPhotoCount = computed(() =>
    (session.value?.burstGroups ?? []).reduce((total, group) => total + group.photoIds.length, 0)
  )
  const maxPairDistance = computed(() =>
    session.value?.burstPairs.length
      ? Math.max(...session.value.burstPairs.map(pair => pair.distance))
      : MAX_HASH_DISTANCE
  )
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
    return '未読み込み'
  }

  async function refreshProjects() {
    // デスクトップは PC の DB、ブラウザは端末内の DB。どちらも一覧を返す。
    projects.value = await desktop.listProjects()
  }

  async function loadPreview(projectId: string) {
    const page = await desktop.getProjectPhotoPage(projectId, 0, 80)
    previewPhotos.value = page.photos
    previewTotal.value = page.total
  }

  async function loadCurrentPhotos(ids = currentGroup.value) {
    if (!activeProject.value || !ids.length) {
      tournamentPhotos.value = []
      return
    }
    tournamentPhotos.value = await desktop.getPhotosByIds(activeProject.value.id, ids)
  }

  async function openProject(project: Project) {
    activeProject.value = project
    view.value = 'project'
    loading.value = true
    try {
      await Promise.all([loadPreview(project.id), desktop.loadSession(project.id).then(value => { session.value = value })])
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
      if (!project.photoCount && !canImportPhotos.value && !scanRunning.value) {
        await startScan()
        return
      }
      const backlog = await desktop.getAnalysisBacklog(project.id).catch(() => 0)
      if (backlog > 0) desktop.startBackgroundAnalysis(project.id).catch(() => undefined)
      // 表示用画像は走査とは別に溜める。**走査に混ぜると解析が桁で遅くなる**
      // （EXIF サムネイル経路 1.72ms/枚 に対しフルデコード 132ms/枚）。
      await refreshDisplayState()
      if (displayBacklog.value > 0) {
        desktop.startDisplayGeneration(project.id).catch(() => undefined)
      }
      await loadSummary()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを開けませんでした。'
    } finally {
      loading.value = false
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
      if (activeProject.value) await loadPreview(activeProject.value.id)
    }
  }

  async function createProject() {
    // ブラウザではフォルダを選べない。名前だけ決めて作り、続けて写真を選ばせる。
    if (!canImportPhotos.value && !folderPath.value) return
    loading.value = true
    try {
      const fallbackName = folderPath.value ? fileName(folderPath.value) : '新しいプロジェクト'
      const project = await desktop.createProject(projectName.value.trim() || fallbackName, folderPath.value)
      createDialog.value = false
      projectName.value = ''
      folderPath.value = ''
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
    // 走査するフォルダを持たない環境では、待つべき進捗が一度も発生しない。
    // ダイアログを出すと閉じる手立てが無くなるので、始めない。
    // ブラウザでの取り込みは `openPhotoPicker` が入口。
    if (canImportPhotos.value) return
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
    if (!activeProject.value?.photoCount) return
    view.value = 'method'
  }

  function openSettings() {
    const prior = session.value?.settings
    settings.groupSize = clampGroupSize(prior?.groupSize ?? groupLimits.default, groupLimits)
    settings.groupBursts = prior?.groupBursts ?? false
    view.value = 'settings'
  }

  // 解析の完了を待たない。scan 後の事前生成で出来ているぶんをそのまま使い、
  // 未解析が残っていても選別画面へ進む。残りはバックグラウンドで進み続ける。
  async function beginTournament() {
    if (!activeProject.value || taskDialog.value) return
    pendingTournamentSettings.value = { ...settings }
    if (settings.groupBursts) {
      taskWarning.value = null
      // 起動できなくても選別自体は始められる。ここで止めない。
      desktop.startBurstAnalysis(activeProject.value.id).catch(() => undefined)
    }
    await finishTournamentStart()
  }

  async function finishTournamentStart() {
    if (!activeProject.value || !pendingTournamentSettings.value) return
    loading.value = true
    try {
      // 最初から選び直すので、前回の星と落選は消す。数字を膨らませない。
      await desktop.resetSelectionResults(activeProject.value.id).catch(() => undefined)
      const [seed, burstPairs] = await Promise.all([
        desktop.getSelectionSeed(activeProject.value.id),
        pendingTournamentSettings.value.groupBursts
          ? desktop.getBurstPairs(activeProject.value.id)
          : Promise.resolve<BurstPair[]>([])
      ])
      if (!seed.length) throw new Error('選別できる写真がありません。')
      session.value = makeSession(
        activeProject.value.id,
        seed,
        pendingTournamentSettings.value,
        burstPairs,
        activeProject.value.burstThreshold
      )
      await enterStage()
      await saveSession()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別を準備できませんでした。'
    } finally {
      pendingTournamentSettings.value = null
      loading.value = false
    }
  }

  /** セッションの stage に合わせて画面と必要なデータを揃える。 */
  async function enterStage() {
    if (!session.value) return
    if (session.value.stage === 'burst-threshold') {
      view.value = 'burst-threshold'
      await showNextPair()
      return
    }
    if (session.value.stage === 'burst-preview') {
      view.value = 'burst-preview'
      await refreshBurstPreview(session.value.burstThreshold ?? inferCurrentThreshold())
      return
    }
    if (session.value.stage === 'result') {
      view.value = 'result'
      return
    }
    if (!session.value.groups.length) prepareRound(session.value)
    view.value = 'tournament'
    await loadCurrentPhotos()
  }

  function inferCurrentThreshold() {
    if (!session.value) return 0
    return inferThreshold(session.value.burstThresholdState.answers, maxPairDistance.value)
  }

  /** 次の出題を用意する。出し尽くしたら確認画面へ進む。 */
  async function showNextPair() {
    if (!session.value) return
    const state = session.value.burstThresholdState
    if (shouldStop(session.value.burstPairs, state, DEFAULT_THRESHOLD_OPTIONS)) {
      await finishThresholdLearning()
      return
    }
    const pair = nextPair(session.value.burstPairs, state)
    if (!pair) {
      await finishThresholdLearning()
      return
    }
    currentPair.value = pair
    pairPhotos.value = activeProject.value
      ? await desktop.getPhotosByIds(activeProject.value.id, [pair.leftPhotoId, pair.rightPhotoId])
      : []
  }

  async function answerPair(grouped: boolean) {
    if (!session.value || !currentPair.value) return
    session.value.burstThresholdState = applyAnswer(
      session.value.burstPairs,
      session.value.burstThresholdState,
      currentPair.value,
      grouped
    )
    await showNextPair()
    await saveSession()
  }

  /** 判断できないペアは学習に使わない。区間は動かさず次を出す。 */
  async function skipCurrentPair() {
    if (!session.value || !currentPair.value) return
    session.value.burstThresholdState = skipPair(session.value.burstThresholdState, currentPair.value)
    await showNextPair()
    await saveSession()
  }

  async function finishThresholdLearning() {
    if (!session.value) return
    currentPair.value = null
    session.value.stage = 'burst-preview'
    view.value = 'burst-preview'
    await refreshBurstPreview(inferCurrentThreshold())
  }

  /** 閾値を当てはめた結果を取り直す。スライダー操作からも呼ぶ。 */
  async function refreshBurstPreview(threshold: number) {
    if (!session.value || !activeProject.value) return
    previewThreshold.value = threshold
    previewBusy.value = true
    try {
      session.value.burstGroups = await desktop.getBurstGroups(activeProject.value.id, threshold)
      session.value.burstThreshold = threshold
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '連写のまとめ結果を取得できませんでした。'
    } finally {
      previewBusy.value = false
    }
  }

  /** さらに質問して閾値を詰める。 */
  async function askMorePairs() {
    if (!session.value) return
    session.value.stage = 'burst-threshold'
    view.value = 'burst-threshold'
    await showNextPair()
    await saveSession()
  }

  /** 確認した閾値を保存し、選別へ進む。 */
  async function acceptBurstThreshold() {
    if (!session.value || !activeProject.value) return
    try {
      await desktop.saveBurstThreshold(activeProject.value.id, previewThreshold.value)
      activeProject.value.burstThreshold = previewThreshold.value
      await refreshProjects()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '閾値を保存できませんでした。'
    }
    // まとめた連写は代表1枚に畳み込む。以降は1カードとして扱う。
    collapseBursts(session.value)
    prepareRound(session.value)
    view.value = 'tournament'
    await loadCurrentPhotos()
    await saveSession()
  }

  /** 設定画面から学習をやり直す。 */
  async function relearnThreshold() {
    if (!activeProject.value) return
    try {
      await desktop.clearBurstThreshold(activeProject.value.id)
      activeProject.value.burstThreshold = null
      await refreshProjects()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '閾値を消去できませんでした。'
    }
  }

  async function saveSession() {
    if (!session.value) return
    session.value.updatedAt = Date.now()
    await desktop.saveSession(session.value)
  }

  async function toggleChoice(photoId: string) {
    if (!session.value) return
    if (!session.value.multiSelect) {
      session.value.selectedInGroup = [photoId]
      await confirmChoices()
      return
    }
    const index = session.value.selectedInGroup.indexOf(photoId)
    if (index >= 0) session.value.selectedInGroup.splice(index, 1)
    else session.value.selectedInGroup.push(photoId)
    await saveSession()
  }

  /**
   * このグループの判断を確定して次へ進む。
   * `selectedInGroup` が空でも進める。良い写真が1枚も無いグループはあるので、
   * その場合は「1枚も通さない」という判断として記録する。
   */
  async function confirmChoices() {
    if (!session.value) return
    const group = [...currentGroup.value]
    // 選択した写真に加え、このグループで★5に確定した写真も通す。
    const chosen = resolveChosen(session.value.selectedInGroup, group, session.value.ratings, MAX_RATING)
    for (const id of chosen) {
      session.value.survivors.push(id)
      // 星は 1 ラウンド通過ごとに +1 で、MAX_RATING で頭打ち。
      session.value.ratings[id] = Math.min(MAX_RATING, (session.value.ratings[id] ?? 0) + 1)
    }
    // 代表に付いた星を、まとめられた仲間にも配る。**survivors には入れない。**
    // 入れると同じラウンドの次の回にまとめの全員が出てきて、畳んだ意味が消える。
    // 星が揃うので、次に「その星を選別」したときには自然に一緒に出てくる。
    const spread = spreadBurstRatings(session.value, chosen)
    // 戻すときは仲間の星も一緒に戻す。survivors に居ない id は undo 側で読み飛ばされる。
    session.value.history.push({ groupIndex: session.value.groupIndex, chosen: [...chosen, ...spread] })
    await persistGroupResults([...group, ...spread])
    session.value.groupIndex += 1
    session.value.selectedInGroup = []
    session.value.multiSelect = false
    if (session.value.groupIndex >= session.value.groups.length) {
      session.value.candidates = [...new Set(session.value.survivors)]
      session.value.stage = 'result'
      view.value = 'result'
    } else {
      await loadCurrentPhotos()
    }
    await saveSession()
  }

  /**
   * 判定したグループぶんだけ DB へ書く。最大10行なので毎回書いても軽い。
   * 全件を書き出す方式にすると 5,000 行の IPC が毎クリック発生する。
   */
  async function persistGroupResults(group: string[]) {
    if (!session.value || !activeProject.value || !group.length) return
    // 選ばれなかった写真の星は据え置き。下げない。
    const entries: SelectionResult[] = group.map(id => ({
      id,
      rating: Math.min(MAX_RATING, session.value!.ratings[id] ?? 0)
    }))
    try {
      await desktop.saveSelectionResults(activeProject.value.id, entries)
      await loadSummary()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別結果を保存できませんでした。'
    }
  }

  /** このグループからは1枚も通さない。 */
  async function skipGroup() {
    if (!session.value) return
    session.value.selectedInGroup = []
    await confirmChoices()
  }

  /**
   * 迷う必要のない1枚を「確定」にする。最高レーティングを付けて通し、
   * 以降のラウンドでは判定に出さない。
   *
   * 複数枚選択中は**トグル**として振る舞う。★5 を付け外しするだけで
   * 次の選別へは進まないので、同じグループの他の写真もそのまま選び続けられる。
   * 決定（Enter）を押すまでグループは確定しない。
   * 単数選択のときは、その1枚を確定して即座に次へ進む（従来どおり）。
   */
  async function confirmPhoto(photoId: string) {
    if (!session.value) return
    if (session.value.multiSelect) {
      // ラウンド開始時、表示中の写真の星はすべて targetRating。だから確定を
      // 外したら targetRating に戻す。★5 は以降どの星の選別にも出てこないので、
      // 「確定」という別状態を持たずに星だけで表せる。
      session.value.ratings[photoId] = isConfirmed(photoId)
        ? session.value.targetRating
        : MAX_RATING
      await saveSession()
      return
    }
    session.value.ratings[photoId] = MAX_RATING
    session.value.selectedInGroup = [photoId]
    await confirmChoices()
  }

  /**
   * 拡大表示を開く。`list` にその写真が並んでいた一覧を渡すと、
   * 拡大したまま ← → で前後の写真へ移れる。
   */
  function openZoom(photo: Photo | null, list: Photo[] = []) {
    if (!photo) return
    zoomPhoto.value = photo
    zoomList.value = list.length ? [...list] : [photo]
  }

  const zoomIndex = computed(() =>
    zoomPhoto.value ? zoomList.value.findIndex(item => item.id === zoomPhoto.value!.id) : -1
  )

  /** 拡大中に前後へ移る。行き先が無ければ**動かないだけ**で、拡大は閉じない。 */
  function stepZoom(step: number) {
    const next = zoomList.value[zoomIndex.value + step]
    if (next) zoomPhoto.value = next
  }

  /**
   * まとめの中身を開く。**ここが「まとまりの形」を直す唯一の場所。**
   *
   * 表示するのは代表のまとめだけではなく、**撮影順で前後 4 秒に入る1続きの写真**。
   * まとめの中身も、まとめに入れられる近くの写真も、同じ1本の並びの上にあるので、
   * 「切る」「繋ぐ」の 2 つだけで分割・切り離し・追加・全解除がすべて表せる。
   */
  async function openBurst(photo: Photo | null) {
    if (!photo || !session.value || !activeProject.value || burstSizeOf(photo.id) < 2) return
    const members = session.value.burstMembers[photo.id] ?? []
    burstOwner.value = photo
    burstDialog.value = true
    burstBusy.value = true
    burstPicked.value = []
    burstReps.value = []
    try {
      const run = await desktop.getBurstNeighborhood(activeProject.value.id, members)
      // 近くに何も無ければ、まとめの中身だけで組む。
      burstPhotos.value = run.length ? run : await desktop.getPhotosByIds(activeProject.value.id, members)
      const ids = burstPhotos.value.map(item => item.id)
      // いまのまとまり方を境目に起こす。まとめに属さない近くの写真は、
      // それぞれ1枚のまとまりとして並ぶ。
      burstCuts.value = cutsFromGroups(ids, Object.values(session.value.burstMembers))
      burstOriginal.value = ids.filter(id => members.includes(id))
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
  /**
   * まだまとめの外にある塊か。**1枚でも元のまとめの写真を含んでいれば「中」**。
   * 外の写真を繋いで取り込んだ塊を「外」と呼び続けないため。
   */
  const isOutsideBurst = (block: string[]) =>
    !block.some(id => burstOriginal.value.includes(id))

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

  /** 隣り合うまとまりを繋ぐ。近くの写真を取り込むのもこれ。 */
  function joinBurstAt(boundaryIndex: number) {
    burstCuts.value = joinAt(burstCuts.value, boundaryIndex)
  }

  function scatterBurst() {
    burstCuts.value = cutAll(burstCuts.value)
    burstPicked.value = []
  }

  /** ある写真の直前の境目。まとまりの先頭以外は必ずある。 */
  function boundaryBefore(photoId: string) {
    return burstPhotos.value.findIndex(photo => photo.id === photoId) - 1
  }

  /**
   * まとめの中で1枚の星を決める。★5 の確定と、明らかな脱落（−1）。
   *
   * 決めた写真は `burstSettled` に入れる。**これが無いとグループを確定した
   * 瞬間に `spreadBurstRatings` が代表の星で塗り潰してしまう。**
   */
  async function settleBurstPhoto(photoId: string, rating: number) {
    if (!session.value || !activeProject.value) return
    const current = session.value.ratings[photoId] ?? 0
    // もう一度押したら取り消し。まとめの外に出すわけではないので星だけ戻す。
    const settled = session.value.burstSettled.includes(photoId)
    const next = settled && current === rating ? session.value.targetRating : rating
    session.value.ratings[photoId] = next
    session.value.burstSettled = next === session.value.targetRating
      ? session.value.burstSettled.filter(id => id !== photoId)
      : [...new Set([...session.value.burstSettled, photoId])]
    const photo = burstPhotoOf(photoId)
    if (photo) photo.rating = next
    try {
      await desktop.saveSelectionResults(activeProject.value.id, [{ id: photoId, rating: next }])
      await loadSummary()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '星を保存できませんでした。'
    }
    await saveSession()
  }

  const confirmBurstPhoto = (photoId: string) => settleBurstPhoto(photoId, MAX_RATING)
  const dropBurstPhoto = (photoId: string) =>
    settleBurstPhoto(photoId, Math.max(0, (session.value?.targetRating ?? 0) - 1))

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
   * まとまりを代表1枚に畳む。並びの中で**最初に出会った位置に代表を置き**、
   * 残りのメンバーは取り除く。位置が動かないので撮影順の意味が保たれる。
   */
  function collapseTo(ids: string[], members: Set<string>, representative: string): string[] {
    let placed = false
    const out: string[] = []
    for (const id of ids) {
      if (!members.has(id)) {
        out.push(id)
        continue
      }
      if (!placed) {
        out.push(representative)
        placed = true
      }
    }
    return out
  }

  /**
   * 直した形を確定して選別画面へ戻す。
   *
   * 保存するのは**例外そのものではなく「こう分かれていてほしい」という形**で、
   * 閾値との食い違いだけがバックエンドで例外として残る。何度押しても結果は同じ。
   */
  async function applyBurstShape() {
    if (!session.value || !activeProject.value) return
    burstBusy.value = true
    try {
      const ids = burstPhotos.value.map(photo => photo.id)
      await desktop.saveBurstShape(activeProject.value.id, ids, burstBlocks.value)

      // セッション側のまとめを組み直す。2枚以上の塊だけが「まとめ」になる。
      // この並びに関わる古いまとめは、いったん全部落としてから作り直す。
      const owned = new Set(burstOriginal.value)
      for (const id of ids) delete session.value.burstMembers[id]

      const released: string[] = []
      for (const block of burstBlocks.value) {
        if (block.length < 2) {
          // 元のまとめから外れて1枚になった写真は、選別に出し直す必要がある。
          if (owned.has(block[0]!)) released.push(block[0]!)
          continue
        }
        const representative = representativeOf(block)
        session.value.burstMembers[representative] = [
          representative, ...block.filter(id => id !== representative)
        ]
        // 代表以外を畳む。**代表が入れ替わってもカードの位置は動かない。**
        const members = new Set(block)
        session.value.candidates = collapseTo(session.value.candidates, members, representative)
        session.value.groups = session.value.groups.map(
          group => collapseTo(group, members, representative)
        )
        session.value.survivors = collapseTo(session.value.survivors, members, representative)
        session.value.selectedInGroup = collapseTo(
          session.value.selectedInGroup, members, representative
        )
      }

      // 外した写真は**今見ているグループを崩さず**、次に見る分の先頭へ。
      insertIntoUpcoming(session.value, released)

      burstDialog.value = false
      burstOwner.value = null
      await loadCurrentPhotos()
      await saveSession()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'まとめの形を保存できませんでした。'
    } finally {
      burstBusy.value = false
    }
  }

  /** 直前の1グループぶんの判断を取り消してやり直す。 */
  async function undoChoice() {
    if (!session.value || !session.value.history.length) return
    // 戻す対象を**先に**控える。`undoLastStep` が履歴から取り出してしまうため。
    // まとめの仲間は表示中のグループに居ないので、これが無いと画面の星だけ戻って
    // DB には上がったままの星が残る。
    const restored = [...(session.value.history[session.value.history.length - 1]?.chosen ?? [])]
    undoLastStep(session.value)
    // 戻したぶんの星と落選を DB からも取り消す。まだ判定していない状態に戻す。
    const group = session.value.groups[session.value.groupIndex] ?? []
    await persistGroupResults([...new Set([...group, ...restored])])
    view.value = 'tournament'
    await loadCurrentPhotos()
    await saveSession()
  }

  /** 選別の途中で1グループの表示枚数を変える。済んだぶんはそのまま。 */
  async function applyGroupSize(size: number) {
    if (!session.value) return
    regroupRemaining(session.value, size)
    groupSizeDialog.value = false
    if (session.value.groupIndex >= session.value.groups.length) {
      session.value.candidates = [...new Set(session.value.survivors)]
      session.value.stage = 'result'
      view.value = 'result'
    } else {
      await loadCurrentPhotos()
    }
    await saveSession()
  }

  function openNextRoundDialog(rating: number) {
    nextRoundGroupSize.value = clampGroupSize(session.value?.settings.groupSize ?? groupLimits.default, groupLimits)
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
    if (!activeProject.value) return
    nextRoundDialog.value = false
    loading.value = true
    try {
      const settingsToUse = session.value?.settings ?? { ...settings }
      if (nextRoundGroupSize.value >= 2) settingsToUse.groupSize = nextRoundGroupSize.value

      const [seed, burstPairs] = await Promise.all([
        desktop.getSelectionSeed(activeProject.value.id, rating),
        settingsToUse.groupBursts
          ? desktop.getBurstPairs(activeProject.value.id)
          : Promise.resolve<BurstPair[]>([])
      ])
      if (seed.length < 2) {
        error.value = `★${rating} の写真が ${seed.length} 枚しかないため、選別できません。`
        return
      }
      session.value = makeSession(
        activeProject.value.id, seed, settingsToUse, burstPairs,
        activeProject.value.burstThreshold, rating
      )
      await enterStage()
      await saveSession()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別を準備できませんでした。'
    } finally {
      loading.value = false
    }
  }

  /** 星を全部 0 に戻して最初からやり直す。解析結果は消えない。 */
  async function restartFromScratch() {
    if (!activeProject.value) return
    restartBusy.value = true
    try {
      await desktop.resetSelectionResults(activeProject.value.id)
      session.value = null
      await desktop.saveSession({ ...(session.value ?? {}) } as SelectionSession).catch(() => undefined)
      await loadSummary()
      restartDialog.value = false
      view.value = 'project'
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'やり直せませんでした。'
    } finally {
      restartBusy.value = false
    }
  }

  // ---- 書き出し -------------------------------------------------------------

  async function chooseExportDestination() {
    try {
      const selected = await desktop.chooseFolder()
      if (selected) exportDestination.value = selected
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'フォルダを選択できませんでした。'
    }
  }

  async function runExport() {
    if (!activeProject.value || !exportDestination.value) return
    exportBusy.value = true
    exportResult.value = null
    exportError.value = ''
    try {
      exportResult.value = await desktop.exportByRating(
        activeProject.value.id, exportDestination.value,
        [...exportRatings.value], exportMode.value === 'move'
      )
      if (exportMode.value === 'move') await refreshProjects()
    } catch (cause) {
      // ダイアログの外に出すと、モーダルに隠れて気づけない。中に出す。
      exportError.value = cause instanceof Error ? cause.message : '書き出しに失敗しました。'
    } finally {
      exportBusy.value = false
    }
  }

  async function runMetadataWrite() {
    if (!activeProject.value || !metadataAcknowledged.value) return
    metadataBusy.value = true
    metadataResult.value = null
    metadataError.value = ''
    try {
      metadataResult.value = await desktop.writeRatingsToFiles(
        activeProject.value.id, [...metadataRatings.value]
      )
    } catch (cause) {
      metadataError.value = cause instanceof Error ? cause.message : 'メタデータを書き込めませんでした。'
    } finally {
      metadataBusy.value = false
    }
  }

  /** 選別結果の一覧。中断中でも開ける。 */
  async function openResults() {
    if (!activeProject.value) return
    view.value = 'results'
    resultsOffset.value = 0
    resultsPhotos.value = []
    await Promise.all([loadResultsPage(true), loadSummary()])
  }

  async function loadSummary() {
    if (!activeProject.value) return
    try {
      selectionSummary.value = await desktop.getSelectionSummary(activeProject.value.id)
    } catch {
      selectionSummary.value = null
    }
  }

  async function selectResultsRating(rating: number | null) {
    resultsRating.value = rating
    await loadResultsPage(true)
  }

  async function loadResultsPage(reset = false) {
    if (!activeProject.value) return
    resultsBusy.value = true
    try {
      if (reset) {
        resultsOffset.value = 0
        resultsPhotos.value = []
      }
      const page = await desktop.getProjectPhotoPage(
        activeProject.value.id, resultsOffset.value, 80, resultsRating.value, resultsSort.value
      )
      resultsPhotos.value = [...resultsPhotos.value, ...page.photos]
      resultsTotal.value = page.total
      resultsOffset.value += page.photos.length
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '選別結果を読み込めませんでした。'
    } finally {
      resultsBusy.value = false
    }
  }

  // ---- 連写の見直し --------------------------------------------------------

  /** いま見ているまとめ。 */
  const burstReviewGroup = computed(() => burstReviewGroups.value[burstReviewIndex.value] ?? null)

  /**
   * 連写の見直しを開く。**まとめは保存していない**ので、学習済みの閾値から
   * その場で引き直す。2枚以上のものだけが対象。
   */
  async function openBurstReview() {
    if (!activeProject.value) return
    view.value = 'burst-review'
    burstReviewLoaded.value = false
    burstReviewBusy.value = true
    burstReviewGroups.value = []
    burstReviewIndex.value = 0
    burstReviewPhotos.value = []
    try {
      const groups = await desktop.getBurstGroups(activeProject.value.id)
      burstReviewGroups.value = groups.filter(group => group.photoIds.length > 1)
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
      await openResults()
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
      const entries = reviewBurstRatings(
        burstReviewPhotos.value.map(photo => ({ id: photo.id, rating: photo.rating })),
        burstReviewKept.value,
        MAX_RATING
      )
      await desktop.saveSelectionResults(activeProject.value.id, entries)
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

  // ---- 表示用画像 ----------------------------------------------------------

  /**
   * 表示用画像の設定と、残っている生成量を読む。
   * プロジェクトを開くたびに呼ぶので、外で作られた分もここで拾える。
   */
  async function refreshDisplayState() {
    if (!activeProject.value) return
    try {
      displaySettings.value = await desktop.getDisplaySettings()
      // プロジェクトの上書きを反映した実効値。null を渡すと現状のまま返る。
      displayEdge.value = await desktop.saveProjectDisplayEdge(activeProject.value.id, null)
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
    } catch {
      // 設定が読めなくても選別は続けられる。表示用が無ければ原本に落ちるだけ。
      displaySettings.value = null
    }
  }

  /**
   * 長辺を変えて作り直す。
   *
   * **下げるときは原本を読み直さない**（保存済みを縮めるだけ）。上げるときは
   * 原本が要るので通信量が増える。UI にその違いを出しておく。
   */
  async function applyDisplayEdge(edge: number) {
    if (!activeProject.value || displayBusy.value) return
    displayBusy.value = true
    try {
      displayEdge.value = await desktop.saveProjectDisplayEdge(activeProject.value.id, edge)
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
      await desktop.startDisplayGeneration(activeProject.value.id)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '表示用の設定を変えられませんでした。'
    } finally {
      displayBusy.value = false
    }
  }

  /** 明示的に作り直す。壊れたときや、途中で止まったときの逃げ道。 */
  async function regenerateDisplayImages() {
    if (!activeProject.value || displayBusy.value) return
    displayBusy.value = true
    try {
      await desktop.resetDisplayImages(activeProject.value.id)
      displayBacklog.value = await desktop.getDisplayBacklog(activeProject.value.id)
      await desktop.startDisplayGeneration(activeProject.value.id)
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '作り直しを始められませんでした。'
    } finally {
      displayBusy.value = false
    }
  }

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

  async function loadMovePage(reset = false) {
    if (!activeProject.value) return
    moveBusy.value = true
    try {
      if (reset) {
        moveOffset.value = 0
        movePhotos.value = []
      }
      const page = await desktop.getProjectPhotoPage(
        activeProject.value.id, moveOffset.value, 80, moveFrom.value, 'name'
      )
      movePhotos.value = [...movePhotos.value, ...page.photos]
      moveTotal.value = page.total
      moveOffset.value += page.photos.length
    } catch (cause) {
      moveError.value = cause instanceof Error ? cause.message : '写真を読み込めませんでした。'
    } finally {
      moveBusy.value = false
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
      const moved = await desktop.moveRating(
        activeProject.value.id, moveFrom.value, moveTo.value, includeIds, excludeIds
      )
      // 進行中のセッションが持つ星も合わせる。移した写真が分かるとき（明示指定）
      // だけメモリ上を直し、それ以外は件数を読み直すことで整合を取る。
      if (session.value && includeIds) {
        for (const id of includeIds) session.value.ratings[id] = moveTo.value
        await saveSession()
      }
      moveDialog.value = false
      await loadSummary()
      if (view.value === 'results') await loadResultsPage(true)
      error.value = ''
      moveReport.value = `${moved.toLocaleString()} 枚を ★${moveFrom.value} から ★${moveTo.value} へ移しました。`
    } catch (cause) {
      moveError.value = cause instanceof Error ? cause.message : 'レートを移動できませんでした。'
    } finally {
      moveBusy.value = false
    }
  }

  // ---- ライブラリへの反映（ブラウザ） --------------------------------------

  /**
   * 書き出しの対象を集める。
   *
   * **原本はこのセッションで取り込んだぶんしか手元に無い。** iOS には永続的な
   * ファイルハンドルが無いため、リロードすると参照が切れる。書き出せる枚数と
   * 全体の枚数を分けて返し、画面で差を伝える。
   */
  async function collectShareCandidates() {
    const project = activeProject.value
    if (!project) return { rows: [], files: [], missing: 0 }
    const rows: { name: string, rating: number, capturedAt: number | null, file: File }[] = []
    let missing = 0
    for (const rating of [...shareRatings.value].sort((left, right) => right - left)) {
      let offset = 0
      for (;;) {
        const page = await desktop.getProjectPhotoPage(project.id, offset, 200, rating, 'name')
        for (const photo of page.photos) {
          const file = desktop.originalFile?.(photo.id) ?? null
          if (file) rows.push({ name: photo.name, rating: photo.rating, capturedAt: photo.capturedAt, file })
          else missing += 1
        }
        offset += page.photos.length
        if (!page.photos.length || offset >= page.total) break
      }
    }
    return { rows, files: rows.map(row => row.file), missing }
  }

  /** 選んだ写真を共有シートに渡す。写真アプリには重複として入る。 */
  async function shareSelectedPhotos() {
    shareBusy.value = true
    shareError.value = ''
    shareMessage.value = ''
    try {
      const { files, missing } = await collectShareCandidates()
      if (!files.length) {
        shareError.value = missing
          ? 'この端末に原本が残っていません。写真を選び直してから書き出してください。'
          : '対象の写真がありません。'
        return
      }
      if (files.length > SHARE_FILE_LIMIT) {
        shareError.value = `一度に共有できるのは ${SHARE_FILE_LIMIT} 枚までです。ZIP で書き出してください。`
        return
      }
      const outcome = await shareFiles(files, `★${shareRatings.value.join('・')} の写真`)
      if (outcome === 'unsupported') shareError.value = 'この端末では共有シートを開けませんでした。ZIP で書き出してください。'
      else if (outcome === 'shared') shareMessage.value = `${files.length} 枚を共有シートに渡しました。`
    } catch (cause) {
      shareError.value = cause instanceof Error ? cause.message : '共有できませんでした。'
    } finally {
      shareBusy.value = false
    }
  }

  /** 星ごとのフォルダに分けた ZIP を書き出す。 */
  async function exportZipByRating() {
    shareBusy.value = true
    shareError.value = ''
    shareMessage.value = ''
    try {
      const { rows, missing } = await collectShareCandidates()
      if (!rows.length) {
        shareError.value = missing
          ? 'この端末に原本が残っていません。写真を選び直してから書き出してください。'
          : '対象の写真がありません。'
        return
      }
      const zip = await createStoredZip(zipEntriesByRating(rows.map(row => ({
        name: row.name, rating: row.rating, blob: row.file, modifiedAt: row.file.lastModified
      }))))
      const stamp = new Date().toISOString().slice(0, 10)
      downloadBlob(zip, `photo-curator-${stamp}.zip`)
      shareMessage.value = `${rows.length} 枚を ZIP にしました${missing ? `（原本の無い ${missing} 枚は除いています）` : ''}。`
    } catch (cause) {
      shareError.value = cause instanceof Error ? cause.message : 'ZIP を作れませんでした。'
    } finally {
      shareBusy.value = false
    }
  }

  function openShareDialog() {
    shareError.value = ''
    shareMessage.value = ''
    shareDialog.value = true
  }

  async function resumeSession() {
    if (!session.value) return openSettings()
    // 別の環境で作られたセッションは、この端末の上限を超える枚数を持ちうる。
    session.value.settings.groupSize = clampGroupSize(session.value.settings.groupSize, groupLimits)
    await enterStage()
  }

  function askDeleteProject(project: Project) {
    deleteTarget.value = project
    deleteDialog.value = true
  }

  async function confirmDeleteProject() {
    const target = deleteTarget.value
    if (!target) return
    deleteBusy.value = true
    try {
      await desktop.deleteProject(target.id)
      if (activeProject.value?.id === target.id) {
        activeProject.value = null
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
      deleteBusy.value = false
    }
  }

  function openGroupSizeDialog() {
    pendingGroupSize.value = clampGroupSize(session.value?.settings.groupSize ?? groupLimits.default, groupLimits)
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
    await desktop.cancelProjectTask(progress.projectId, progress.task)
  }

  function onKeydown(event: KeyboardEvent) {
    // 拡大中はどのキーでも閉じるだけ。選択には流さない。
    if (zoomPhoto.value) {
      onZoomKeydown(event)
      return
    }
    if (event.target instanceof HTMLInputElement) return

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

  /** 拡大表示は、左右キーだけ前後送りに使い、それ以外のキーでは閉じる。 */
  function onZoomKeydown(event: KeyboardEvent) {
    if (!zoomPhoto.value) return
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'ArrowLeft') { stepZoom(-1); return }
    if (event.key === 'ArrowRight') { stepZoom(1); return }
    zoomPhoto.value = null
  }

  async function mount() {
    try {
      await refreshProjects()
      stopProgressListener = await desktop.onProjectProgress(async (progress) => {
        if (!activeProject.value || progress.projectId !== activeProject.value.id) return
        // 連写解析（前面・事前生成とも）は待たせない。帯で状況だけ伝える。
        if (progress.task !== 'scan') {
          analysisProgress.value = progress
          analysisFailures.value = progress.failed
          // 警告は解析中の1イベントにしか乗らないので、別に保持して出し続ける。
          if (progress.warning) taskWarning.value = progress.warning
          if (progress.phase === 'error') error.value = progress.message
          return
        }
        taskProgress.value = progress
        if (progress.warning) taskWarning.value = progress.warning
        if (progress.phase === 'complete') {
          taskDialog.value = false
          await refreshProjects()
          activeProject.value = projects.value.find(project => project.id === activeProject.value?.id) ?? activeProject.value
          await loadPreview(progress.projectId)
        }
        if (progress.phase === 'cancelled' || progress.phase === 'error') {
          taskDialog.value = false
          if (progress.phase === 'error') error.value = progress.message
        }
      })
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'プロジェクトを読み込めませんでした。'
    }
    window.addEventListener('keydown', onKeydown)
    compactQuery = window.matchMedia(COMPACT_QUERY)
    syncCompact(compactQuery)
    compactQuery.addEventListener('change', syncCompact)
  }

  function unmount() {
    stopProgressListener?.()
    window.removeEventListener('keydown', onKeydown)
    compactQuery?.removeEventListener('change', syncCompact)
  }

  return {
    MAX_RATING,
    desktop,
    projects,
    activeProject,
    previewPhotos,
    previewTotal,
    tournamentPhotos,
    session,
    view,
    loading,
    error,
    createDialog,
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
    largeDisplay,
    pendingTournamentSettings,
    groupLimits,
    settings,
    isTouchOnly,
    COMPACT_QUERY,
    isCompact,
    drawerOpen,
    drawerRail,
    syncCompact,
    currentPair,
    pairPhotos,
    previewThreshold,
    previewBusy,
    groupSizeDialog,
    pendingGroupSize,
    deleteDialog,
    deleteTarget,
    deleteBusy,
    columnsFor,
    tournamentColumns,
    tournamentRows,
    isSelecting,
    nextRoundDialog,
    nextRoundGroupSize,
    nextRoundRating,
    restartDialog,
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
    moveReport,
    moveSelection,
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
    burstOwner,
    burstPhotos,
    burstCuts,
    burstOriginal,
    burstPicked,
    burstReps,
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
    maxQuestions,
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
    refreshProjects,
    loadPreview,
    loadCurrentPhotos,
    openProject,
    chooseFolder,
    canImportPhotos,
    photoInput,
    openPhotoPicker,
    onPhotoPicked,
    createProject,
    startScan,
    enterMethod,
    openSettings,
    beginTournament,
    finishTournamentStart,
    enterStage,
    inferCurrentThreshold,
    showNextPair,
    answerPair,
    skipCurrentPair,
    finishThresholdLearning,
    refreshBurstPreview,
    askMorePairs,
    acceptBurstThreshold,
    relearnThreshold,
    saveSession,
    toggleChoice,
    confirmChoices,
    persistGroupResults,
    skipGroup,
    confirmPhoto,
    openZoom,
    zoomIndex,
    stepZoom,
    openBurst,
    burstBlocks,
    burstPhotoOf,
    isOutsideBurst,
    representativeOf,
    toggleBurstPick,
    splitBurstSelection,
    joinBurstAt,
    scatterBurst,
    boundaryBefore,
    settleBurstPhoto,
    confirmBurstPhoto,
    dropBurstPhoto,
    makeBurstRepresentative,
    collapseTo,
    applyBurstShape,
    undoChoice,
    applyGroupSize,
    openNextRoundDialog,
    startRatingSelection,
    restartFromScratch,
    chooseExportDestination,
    runExport,
    runMetadataWrite,
    openResults,
    loadSummary,
    selectResultsRating,
    loadResultsPage,
    burstReviewGroup,
    openBurstReview,
    loadBurstReviewPhotos,
    toggleBurstReviewKeep,
    advanceBurstReview,
    applyBurstReview,
    skipBurstReview,
    refreshDisplayState,
    applyDisplayEdge,
    regenerateDisplayImages,
    openMoveDialog,
    loadMovePage,
    toggleMoveSelection,
    setMoveSelectAll,
    runMove,
    collectShareCandidates,
    shareSelectedPhotos,
    exportZipByRating,
    openShareDialog,
    resumeSession,
    askDeleteProject,
    confirmDeleteProject,
    openGroupSizeDialog,
    cancelTask,
    cancelAnalysis,
    onKeydown,
    onZoomKeydown,
    mount,
    unmount
  }
}

let instance: ReturnType<typeof createCurator> | null = null

export function useCurator() {
  return (instance ??= createCurator())
}
