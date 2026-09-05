<script setup lang="ts">
import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoSort, Project, ProjectProgress,
  SelectionResult, SelectionSession, SelectionSummary, TournamentSettings
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
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

onMounted(async () => {
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
})

onBeforeUnmount(() => {
  stopProgressListener?.()
  window.removeEventListener('keydown', onKeydown)
  compactQuery?.removeEventListener('change', syncCompact)
})
</script>

<template>
  <v-app>
    <!-- 狭い画面では常設をやめ、ヘッダーのボタンで開閉する。 -->
    <v-app-bar v-if="isCompact" flat color="surface" density="comfortable">
      <v-app-bar-nav-icon aria-label="メニューを開く" @click="drawerOpen = !drawerOpen" />
      <v-app-bar-title class="text-body-1">Photo Curator</v-app-bar-title>
    </v-app-bar>

    <!-- 選別中は写真に幅を使いたい。rail でアイコンだけの細い状態に畳める。 -->
    <v-navigation-drawer
      v-model="drawerOpen"
      :permanent="!isCompact"
      :temporary="isCompact"
      :rail="!isCompact && drawerRail"
      :width="280"
      rail-width="60"
      color="surface"
    >
      <v-list-item class="py-4" :title="drawerRail ? undefined : 'Photo Curator'" :subtitle="drawerRail ? undefined : '人の目で、素早く選ぶ'">
        <template #prepend><v-avatar color="primary" size="34"><v-icon color="black" icon="mdi-image-multiple-outline" /></v-avatar></template>
        <template v-if="!drawerRail && !isCompact" #append>
          <v-btn icon="mdi-chevron-left" variant="text" size="small" aria-label="サイドバーをたたむ" @click.stop="drawerRail = true" />
        </template>
      </v-list-item>
      <v-divider />
      <v-list nav class="pt-3">
        <v-list-item prepend-icon="mdi-home-outline" title="ホーム" :active="view === 'home'" @click="view = 'home'" />
        <!-- rail では入れ子のリストが開けないので、畳んだときは1項目にまとめる。 -->
        <v-list-item
          v-if="drawerRail && !isCompact"
          prepend-icon="mdi-folder-multiple-image" title="プロジェクト"
          @click="drawerRail = false"
        />
        <v-list-group v-else value="projects">
          <template #activator="{ props }"><v-list-item v-bind="props" prepend-icon="mdi-folder-multiple-image" title="プロジェクト" /></template>
          <v-list-item v-for="project in projects" :key="project.id" class="project-nav-item" :title="project.name" :subtitle="statusLabel(project)" @click="openProject(project)">
            <template #prepend><v-icon size="18" icon="mdi-folder-outline" /></template>
          </v-list-item>
          <v-list-item v-if="!projects.length" title="まだありません" subtitle="ホームから作成できます" disabled />
        </v-list-group>
      </v-list>
      <template #append>
        <v-list nav class="pb-2">
          <v-list-item
            v-if="!isCompact"
            :prepend-icon="drawerRail ? 'mdi-chevron-right' : 'mdi-chevron-left'"
            :title="drawerRail ? '広げる' : 'サイドバーをたたむ'"
            @click="drawerRail = !drawerRail"
          />
        </v-list>
        <v-card v-if="!drawerRail" flat class="ma-3 pa-3 drop-placeholder" title="作業フォルダへ送る" subtitle="ドラッグ＆ドロップは準備中" prepend-icon="mdi-folder-move-outline" />
      </template>
    </v-navigation-drawer>

    <v-main class="app-shell">
      <!-- 選別中は写真の面積を優先し、余白と横幅の上限をゆるめる。 -->
      <v-container
        fluid
        :class="isSelecting ? 'pa-3 pa-md-5' : 'pa-7 pa-md-10'"
        :style="{ maxWidth: isSelecting ? '100%' : '1680px' }"
      >
        <v-alert v-if="error" type="error" closable class="mb-5" @click:close="error = ''">{{ error }}</v-alert>
        <v-progress-linear v-if="loading" indeterminate color="primary" class="mb-6" />

        <!-- 連写解析はバックグラウンドで進む。操作を止めずに状況だけ見せる。 -->
        <v-alert v-if="analysisRunning" type="info" variant="tonal" density="compact" class="mb-5">
          <div class="d-flex align-center justify-space-between flex-wrap ga-3">
            <span class="text-body-2">{{ analysisProgress?.message }}<template v-if="analysisProgress?.total"> （{{ analysisProgress.processed.toLocaleString() }} / {{ analysisProgress.total.toLocaleString() }}）</template></span>
            <v-btn variant="text" size="small" @click="cancelAnalysis">解析を止める</v-btn>
          </div>
          <v-progress-linear :model-value="analysisValue" :indeterminate="!analysisProgress?.total" color="primary" height="6" rounded class="mt-2" />
        </v-alert>
        <v-alert v-if="taskWarning" type="info" variant="tonal" density="compact" closable class="mb-5" @click:close="taskWarning = null">{{ taskWarning }}</v-alert>
        <v-alert v-if="moveReport" type="success" variant="tonal" density="compact" closable class="mb-5" @click:close="moveReport = ''">{{ moveReport }}</v-alert>
        <!-- 1枚も解析できなくても選別は続けられる。件数だけ伝えて先へ進ませる。 -->
        <v-alert v-if="analysisFailures" type="warning" variant="tonal" density="compact" closable class="mb-5" @click:close="analysisFailures = 0">
          {{ analysisFailures.toLocaleString() }} 件を解析できませんでした。該当の写真は連写のまとめ対象から外れますが、選別はこのまま続けられます。
        </v-alert>

        <template v-if="view === 'home'">
          <div class="d-flex align-start justify-space-between flex-wrap ga-4 mb-8">
            <div><div class="text-overline text-primary">Photo selection workspace</div><h1 class="text-h3 font-weight-bold">写真を、選びやすい形へ。</h1><p class="text-medium-emphasis mt-2">プロジェクトを作成して、直感的な選択を始めましょう。</p></div>
            <v-btn color="primary" size="large" prepend-icon="mdi-plus" @click="createDialog = true">プロジェクトを作成</v-btn>
          </div>
          <v-row>
            <v-col cols="12"><v-card><v-card-title class="pt-5 px-5">プロジェクト</v-card-title><v-card-subtitle class="px-5">選別の状況をここから確認できます。</v-card-subtitle><v-list v-if="projects.length" lines="two" class="mt-3"><v-list-item v-for="project in projects" :key="project.id" :title="project.name" :subtitle="shortPath(project.folderPath)" @click="openProject(project)"><template #prepend><v-avatar color="surface-variant"><v-icon icon="mdi-folder-image" /></v-avatar></template><template #append><div class="d-flex align-center ga-4"><div class="text-right"><div class="text-body-2">{{ statusLabel(project) }}</div><div class="text-caption text-medium-emphasis">更新 {{ formatDate(project.updatedAt) }}</div></div><v-btn icon="mdi-delete-outline" variant="text" size="small" :aria-label="`${project.name} を削除`" @click.stop="askDeleteProject(project)" /></div></template></v-list-item></v-list><v-card-text v-else class="py-12 text-center text-medium-emphasis">まだプロジェクトがありません。</v-card-text></v-card></v-col>
          </v-row>
        </template>

        <template v-else-if="view === 'project' && activeProject">
          <div class="d-flex align-center justify-space-between flex-wrap ga-4 mb-7"><div><v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="view = 'home'">ホーム</v-btn><h1 class="text-h4 font-weight-bold">{{ activeProject.name }}</h1><p class="text-body-2 text-medium-emphasis mt-1">{{ activeProject.folderPath }}</p></div><div class="d-flex flex-wrap ga-2"><v-btn variant="text" prepend-icon="mdi-delete-outline" @click="askDeleteProject(activeProject)">削除</v-btn><v-btn v-if="hasSelectionData" variant="text" prepend-icon="mdi-star-outline" @click="openResults">選別結果を見る</v-btn><v-btn v-if="canImportPhotos" variant="outlined" prepend-icon="mdi-image-plus" :loading="scanRunning" @click="openPhotoPicker">写真を追加</v-btn><v-btn v-else variant="outlined" prepend-icon="mdi-refresh" :loading="scanRunning" @click="startScan">写真を再読み込み</v-btn><v-btn color="primary" prepend-icon="mdi-play" :disabled="!activeProject.photoCount" @click="session ? resumeSession() : enterMethod()">{{ session ? '選別を再開' : '選別を開始' }}</v-btn></div></div>
          <v-card class="mb-6"><v-card-text class="d-flex align-center ga-5"><v-avatar color="primary" size="50"><v-icon color="black" icon="mdi-image-multiple" /></v-avatar><div><div class="text-h6">{{ activeProject.photoCount.toLocaleString() }} 枚の写真</div><div class="text-body-2 text-medium-emphasis">{{ canImportPhotos ? '星とサムネイルはこの端末に保存されます。写真ライブラリは変更しません。' : 'サブフォルダも含めて参照します。写真ファイルは変更しません。' }}</div></div></v-card-text></v-card>
          <div v-if="previewPhotos.length" class="d-flex align-center justify-end ga-3 mb-4">
            <v-btn-toggle v-model="previewDensity" density="comfortable" variant="outlined" divided mandatory>
              <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
            </v-btn-toggle>
          </div>
          <div v-if="previewPhotos.length" class="photo-grid" :class="gridClass(previewDensity)" :style="gridStyle(previewDensity)"><div v-for="photo in previewPhotos" :key="photo.id" class="photo-tile" role="button" tabindex="0" @click="openZoom(photo, previewPhotos)" @keydown.enter="openZoom(photo, previewPhotos)"><img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy"><div class="photo-tile__caption">{{ photo.relativePath }}</div></div></div>
          <v-alert v-if="previewTotal > previewPhotos.length" type="info" variant="tonal" class="mt-5">表示負荷を抑えるため、最初の {{ previewPhotos.length }} 枚だけを表示しています。選別にはすべての写真が含まれます。</v-alert>
          <!-- まだ 1 枚も無いプロジェクト。次にやることを 1 つだけ置く。 -->
          <v-card v-if="canImportPhotos && !previewPhotos.length && !scanRunning" class="pa-10 text-center">
            <v-icon icon="mdi-image-plus" size="44" class="text-medium-emphasis" />
            <div class="text-h6 mt-4">まだ写真がありません</div>
            <p class="text-body-2 text-medium-emphasis mt-2 mb-6">
              この端末の写真から選びます。写真そのものは端末の外に出ません。
            </p>
            <v-btn color="primary" size="large" prepend-icon="mdi-image-plus" @click="openPhotoPicker">写真を追加</v-btn>
          </v-card>
        </template>

        <template v-else-if="view === 'method'">
          <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'project'">プロジェクトへ戻る</v-btn><div class="text-overline text-primary">選別を開始</div><h1 class="text-h4 mb-6">方法を選んでください</h1>
          <v-row><v-col cols="12" md="6"><v-card class="choice-card pa-6 h-100" role="button" tabindex="0" @click="openSettings" @keydown.enter="openSettings"><v-icon color="primary" size="40" icon="mdi-tournament" /><div class="text-h5 mt-5">トーナメントで選別</div><p class="text-medium-emphasis mt-2">複数の写真から、次のラウンドに進める1枚を選びます。</p><v-btn color="primary" class="mt-4">設定へ進む</v-btn></v-card></v-col><v-col cols="12" md="6"><v-card class="choice-card choice-card--disabled pa-6 h-100" aria-disabled="true"><v-icon size="40" icon="mdi-play-box-outline" /><div class="text-h5 mt-5">スライドショーで選別</div><p class="text-medium-emphasis mt-2">1枚ずつ直感的に選べる方法です。</p><v-chip class="mt-4">準備中</v-chip></v-card></v-col></v-row>
        </template>

        <template v-else-if="view === 'settings'">
          <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'method'">方法の選択へ戻る</v-btn><div class="text-overline text-primary">Tournament setup</div><h1 class="text-h4 mb-6">トーナメントの設定</h1>
          <v-card max-width="720" class="pa-6 settings-card"><div class="text-subtitle-1 font-weight-medium mb-5">何枚から選びますか？</div><v-slider v-model="settings.groupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="何枚から選ぶか"><template #append><v-text-field v-model.number="settings.groupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template></v-slider><p class="text-caption text-medium-emphasis mt-2">少ないほど比較は丁寧に、多いほどテンポよく進みます。</p><v-divider class="my-7" /><v-switch v-model="settings.groupBursts" color="primary" label="事前にバースト写真（連写）をまとめる" hint="最大 8 問だけ答えると、残りは同じ基準で自動的にまとまります。" persistent-hint />
            <v-alert v-if="settings.groupBursts && activeProject?.burstThreshold !== null && activeProject?.burstThreshold !== undefined" type="info" variant="tonal" density="comfortable" class="mt-4">
              <div class="d-flex align-center justify-space-between flex-wrap ga-3">
                <span>このプロジェクトは学習済みです（基準 {{ activeProject.burstThreshold }}）。質問は出ません。</span>
                <v-btn size="small" variant="outlined" @click="relearnThreshold">学習し直す</v-btn>
              </div>
            </v-alert>
            <div class="d-flex justify-end mt-8"><v-btn color="primary" size="large" prepend-icon="mdi-play" @click="beginTournament">選別を開始</v-btn></div></v-card>
        </template>

        <template v-else-if="view === 'burst-threshold' && session">
          <div class="d-flex align-center justify-space-between flex-wrap ga-3 mb-2">
            <div>
              <div class="text-overline text-primary">連写のまとめ方を学習中 &middot; {{ askedCount + 1 }} / 最大 {{ maxQuestions }} 問</div>
              <h1 class="text-h5 text-md-h4">この2枚は同じ連写ですか？</h1>
            </div>
            <v-btn variant="text" prepend-icon="mdi-pause-circle-outline" @click="view = 'project'">中断して戻る</v-btn>
          </div>
          <p class="text-body-2 text-medium-emphasis mb-5">
            数問だけ答えると、残りは同じ基準で自動的にまとまります。全部を確認する必要はありません。
          </p>
          <v-progress-linear :model-value="(askedCount / maxQuestions) * 100" color="primary" height="6" rounded class="mb-6" />

          <template v-if="currentPair && pairPhotos.length === 2">
            <div class="compare-pair">
              <figure v-for="photo in pairPhotos" :key="photo.id" class="compare-pair__item">
                <img :src="desktop.photoUrl(photo.path)" :alt="photo.name">
                <figcaption class="text-caption text-medium-emphasis mt-2 text-truncate">{{ photo.name }}</figcaption>
              </figure>
            </div>
            <div class="text-caption text-medium-emphasis mt-4 text-center">
              撮影間隔 {{ (currentPair.gapMs / 1000).toFixed(1) }} 秒
            </div>
            <!-- 主操作を右端（縦並びでは最下部）に置く。まとめる／別々は二分探索の
                 性質上ほぼ半々で押されるので、隣り合わせにしておく。 -->
            <div class="compare-actions mt-5">
              <v-btn size="large" variant="text" @click="skipCurrentPair">判断できない<span class="ms-2 text-caption">1</span></v-btn>
              <v-btn size="large" variant="outlined" @click="answerPair(false)">別々に扱う<span class="ms-2 text-caption">2</span></v-btn>
              <v-btn size="large" color="primary" prepend-icon="mdi-image-album" @click="answerPair(true)">まとめる<span class="ms-2 text-caption">3</span></v-btn>
            </div>
          </template>

          <v-card v-else class="pa-8 text-center">
            <v-progress-circular indeterminate color="primary" class="mb-4" />
            <div class="text-body-1">連写の候補を準備しています…</div>
            <p class="text-caption text-medium-emphasis mt-2 mb-0">
              解析が進むと候補が増えます。そのまま少しお待ちください。
            </p>
          </v-card>
        </template>

        <template v-else-if="view === 'burst-preview' && session">
          <div class="text-overline text-primary">連写のまとめ方</div>
          <h1 class="text-h5 text-md-h4">{{ session.burstGroups.length.toLocaleString() }} グループ / {{ groupedPhotoCount.toLocaleString() }} 枚にまとまりました</h1>
          <p class="text-body-2 text-medium-emphasis mt-2">
            {{ askedCount }} 問の回答から、まとめる基準を {{ previewThreshold }} と判断しました。ここで微調整できます。
          </p>

          <v-card max-width="760" class="mt-6 pa-6 settings-card">
            <div class="text-subtitle-2 mb-4">まとめる基準（小さいほど厳しく、似た写真だけをまとめます）</div>
            <v-slider
              class="selection-slider"
              :model-value="previewThreshold"
              :min="0"
              :max="maxPairDistance"
              :step="1"
              thumb-label
              :disabled="previewBusy"
              aria-label="まとめる基準"
              @update:model-value="refreshBurstPreview($event as number)"
            />
            <div class="d-flex justify-space-between text-caption text-medium-emphasis">
              <span>厳しく（まとまりにくい）</span><span>ゆるく（まとまりやすい）</span>
            </div>
            <v-divider class="my-6" />
            <div class="d-flex flex-wrap ga-3 justify-end">
              <v-btn variant="outlined" :disabled="previewBusy" @click="askMorePairs">もう少し質問する</v-btn>
              <v-btn color="primary" size="large" prepend-icon="mdi-play" :loading="previewBusy" @click="acceptBurstThreshold">この設定で選別を始める</v-btn>
            </div>
          </v-card>
        </template>

        <template v-else-if="view === 'tournament' && session">
          <div class="d-flex align-center justify-space-between flex-wrap ga-3 mb-3">
            <div>
              <div class="text-overline text-primary">★{{ session.targetRating }} を選別中 &middot; Round {{ session.round }}</div>
              <h1 class="text-h6 text-md-h5">残り {{ remainingPhotos.toLocaleString() }} 枚 / {{ remainingGroups.toLocaleString() }} グループ</h1>
            </div>
            <div class="d-flex flex-wrap ga-2">
              <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!session.history.length" @click="undoChoice">1つ戻す<span class="ms-1 text-caption">⌫</span></v-btn>
              <v-btn variant="text" prepend-icon="mdi-view-grid-outline" @click="openGroupSizeDialog">表示枚数</v-btn>
              <v-btn variant="text" prepend-icon="mdi-pause-circle-outline" @click="view = 'project'">中断して戻る</v-btn>
            </div>
          </div>
          <v-progress-linear :model-value="session.groups.length ? (session.groupIndex / session.groups.length) * 100 : 100" color="primary" height="6" rounded class="mb-2" />
          <div class="d-flex justify-space-between text-caption text-medium-emphasis mb-4"><span>選択済み {{ selectedCount }} 枚</span><span>このグループ {{ tournamentPhotos.length }} 枚</span></div>

          <div
            class="tournament-grid"
            :style="{ '--tournament-columns': tournamentColumns, '--tournament-rows': tournamentRows }"
          >
            <v-card
              v-for="(photo, index) in tournamentPhotos"
              :key="photo.id"
              class="tournament-card"
              :class="{
                'is-selected': session.selectedInGroup.includes(photo.id),
                'is-confirmed': isConfirmed(photo.id),
                'is-burst': burstSizeOf(photo.id) > 1
              }"
              @click="toggleChoice(photo.id)"
            >
              <span class="tournament-card__number">{{ index + 1 === 10 ? 0 : index + 1 }}</span>

              <!-- 選択のクリックと切り分けるため、拡大と確定は右上に置く。 -->
              <div class="tournament-card__tools">
                <!-- 迷う必要のない1枚を、その場で★5にして以降の判定から外す。
                     複数枚選択中はトグルなので、確定済みでも押せるように出し続ける
                     （もう一度押すと確定を外せる）。単数選択では確定した時点で次へ
                     進むため、確定済みの表示は残らない。 -->
                <v-btn
                  v-if="!isConfirmed(photo.id) || session.multiSelect"
                  :icon="isConfirmed(photo.id) ? 'mdi-star' : 'mdi-star-outline'"
                  size="x-small" variant="flat"
                  :color="isConfirmed(photo.id) ? 'secondary' : undefined"
                  :aria-label="isConfirmed(photo.id) ? `${photo.name} の★${MAX_RATING}確定を外す` : `${photo.name} を★${MAX_RATING} で確定`"
                  @click.stop="confirmPhoto(photo.id)"
                />
                <v-btn
                  icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                  :aria-label="`${photo.name} を拡大`"
                  @click.stop="openZoom(photo, tournamentPhotos)"
                />
              </div>

              <!-- 連写の表示は左下の1か所だけ。ここ自体がまとめを開くボタン。
                   以前は右上にも同じ操作があり、左下は押しても開かず選択されていた。 -->
              <button
                v-if="burstSizeOf(photo.id) > 1"
                type="button" class="tournament-card__stackmark"
                :aria-label="`まとめられた ${burstSizeOf(photo.id)} 枚を開く`"
                @click.stop="openBurst(photo)"
              >
                <v-icon icon="mdi-layers-triple-outline" size="16" />
                連写 {{ burstSizeOf(photo.id) }} 枚
                <v-icon icon="mdi-chevron-right" size="16" />
              </button>

              <span v-if="session.selectedInGroup.includes(photo.id)" class="tournament-card__check">
                <v-icon icon="mdi-check-bold" size="20" />
              </span>
              <span v-if="isConfirmed(photo.id)" class="tournament-card__confirmed">確定</span>

              <img :src="desktop.photoUrl(photo.path)" :alt="photo.name">
            </v-card>
          </div>

          <v-sheet class="d-flex align-center justify-space-between flex-wrap ga-3 mt-4 pa-3" color="surface-variant" rounded>
            <v-checkbox v-model="session.multiSelect" density="compact" hide-details label="複数枚選択（M）" @update:model-value="session.selectedInGroup = []; saveSession()" />
            <!-- キーボードが無い環境では案内しない。操作はカード上のボタンで完結する。 -->
            <div v-if="!isTouchOnly" class="text-caption text-medium-emphasis">
              1〜0 選ぶ ・ Ctrl+数字 拡大 ・ Shift+数字 ★{{ MAX_RATING }}で確定 ・ Alt+数字 まとめを開く ・ Enter 決定 ・ ⌫ 戻す
            </div>
            <div class="d-flex flex-wrap ga-2">
              <v-btn variant="outlined" @click="skipGroup">どれも選ばない</v-btn>
              <v-btn color="primary" @click="confirmChoices">
                {{ session.selectedInGroup.length ? `${session.selectedInGroup.length} 枚を選択` : '選択なしで次へ' }}（Enter）
              </v-btn>
            </div>
          </v-sheet>
        </template>

        <template v-else-if="view === 'results'">
          <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-3" @click="view = 'project'">プロジェクトへ戻る</v-btn>
          <h1 class="text-h5 text-md-h4">レーティング</h1>
          <p class="text-body-2 text-medium-emphasis mt-2">
            星を選ぶと、その星の写真だけを選別できます。選ばれた写真は星が1つ上がり、選ばれなかった写真はそのままです。
          </p>

          <div class="rating-board mt-6">
            <div
              v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
              class="rating-row" :class="{ 'is-active': resultsRating === rating }"
              role="button" tabindex="0"
              @click="selectResultsRating(resultsRating === rating ? null : rating)"
              @keydown.enter="selectResultsRating(resultsRating === rating ? null : rating)"
            >
              <span class="rating-row__stars">
                <v-icon v-for="star in MAX_RATING" :key="star" size="17" :icon="star <= rating ? 'mdi-star' : 'mdi-star-outline'" :class="star <= rating ? 'text-primary' : 'text-medium-emphasis'" />
              </span>
              <span class="rating-row__count">{{ ratingCount(rating).toLocaleString() }} 枚</span>
              <v-progress-linear
                class="rating-row__bar"
                :model-value="selectionSummary?.total ? (ratingCount(rating) / selectionSummary.total) * 100 : 0"
                :color="rating >= 4 ? 'primary' : 'secondary'" height="6" rounded
              />
              <span class="rating-row__actions">
                <v-btn
                  size="small" variant="outlined" prepend-icon="mdi-tournament"
                  :disabled="ratingCount(rating) < 2"
                  @click.stop="openNextRoundDialog(rating)"
                >この {{ ratingCount(rating).toLocaleString() }} 枚を選別</v-btn>
                <v-btn
                  size="small" variant="text" prepend-icon="mdi-swap-horizontal"
                  :disabled="!ratingCount(rating)"
                  @click.stop="openMoveDialog(rating)"
                >レートを移動</v-btn>
              </span>
            </div>
          </div>

          <div class="d-flex flex-wrap align-center ga-3 mt-6 mb-4">
            <v-btn-toggle v-model="resultsSort" density="comfortable" variant="outlined" divided mandatory @update:model-value="loadResultsPage(true)">
              <v-btn value="rating">星の高い順</v-btn>
              <v-btn value="name">ファイル名順</v-btn>
            </v-btn-toggle>
            <v-btn-toggle v-model="resultsDensity" density="comfortable" variant="outlined" divided mandatory>
              <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
            </v-btn-toggle>
            <v-chip v-if="resultsRating !== null" closable color="primary" variant="flat" @click:close="selectResultsRating(null)">
              ★{{ resultsRating }} だけ表示中
            </v-chip>
            <v-spacer />
            <!-- 選別中は「まとめの中から1枚」を決めていない。その1手をここで引き受ける。 -->
            <v-btn variant="outlined" prepend-icon="mdi-layers-triple-outline" @click="openBurstReview">連写を見直す</v-btn>
            <!-- デスクトップは原本のフォルダを直接操作できる。ブラウザはできないので、
                 共有シートか星ごとの ZIP を通して渡す。 -->
            <template v-if="canImportPhotos">
              <v-btn variant="outlined" prepend-icon="mdi-export-variant" @click="openShareDialog">書き出す</v-btn>
            </template>
            <template v-else>
              <v-btn variant="outlined" prepend-icon="mdi-folder-move-outline" @click="exportDialog = true">フォルダ分け</v-btn>
              <v-btn variant="outlined" prepend-icon="mdi-tag-text-outline" @click="metadataDialog = true">メタデータに反映</v-btn>
            </template>
            <v-btn variant="text" prepend-icon="mdi-restart" @click="restartDialog = true">最初からやり直す</v-btn>
          </div>

          <div v-if="resultsPhotos.length" class="result-grid" :class="gridClass(resultsDensity)" :style="gridStyle(resultsDensity)">
            <div
              v-for="photo in resultsPhotos" :key="photo.id" class="result-tile"
              :class="{ 'is-confirmed': photo.rating >= MAX_RATING, 'is-eliminated': photo.rating === 0 }"
              role="button" tabindex="0"
              @click="openZoom(photo, resultsPhotos)" @keydown.enter="openZoom(photo, resultsPhotos)"
            >
              <img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy">
              <div class="result-tile__meta">
                <span class="result-tile__stars">
                  <v-icon v-for="star in MAX_RATING" :key="star" size="13" :icon="star <= photo.rating ? 'mdi-star' : 'mdi-star-outline'" :class="star <= photo.rating ? 'text-primary' : 'text-medium-emphasis'" />
                </span>
              </div>
              <div class="result-tile__name text-caption">{{ photo.name }}</div>
            </div>
          </div>
          <v-card v-else-if="!resultsBusy" class="pa-10 text-center text-medium-emphasis">
            この条件に当てはまる写真はありません。
          </v-card>

          <div class="d-flex justify-center mt-6">
            <v-btn v-if="resultsPhotos.length < resultsTotal" variant="outlined" :loading="resultsBusy" @click="loadResultsPage()">
              さらに読み込む（{{ resultsPhotos.length.toLocaleString() }} / {{ resultsTotal.toLocaleString() }}）
            </v-btn>
            <span v-else-if="resultsPhotos.length" class="text-caption text-medium-emphasis">{{ resultsTotal.toLocaleString() }} 枚すべて表示しました</span>
          </div>
        </template>

        <!-- 連写の見直し。まとめ単位で「残す写真」を決めて星を上げ下げする。 -->
        <template v-else-if="view === 'burst-review'">
          <div class="d-flex flex-wrap align-center justify-space-between ga-3 mb-3">
            <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="openResults">レーティングへ戻る</v-btn>
            <span v-if="burstReviewGroups.length" class="text-caption text-medium-emphasis">
              {{ (burstReviewIndex + 1).toLocaleString() }} / {{ burstReviewGroups.length.toLocaleString() }} グループ
            </span>
          </div>

          <template v-if="burstReviewGroups.length && burstReviewPhotos.length">
            <h1 class="text-h6 text-md-h5">まとめられた {{ burstReviewPhotos.length }} 枚から残す写真を選ぶ</h1>
            <p class="text-body-2 text-medium-emphasis mt-1 mb-3">
              残した写真は星が1つ上がり、外した写真は1つ下がります。
            </p>
            <v-progress-linear
              :model-value="burstReviewGroups.length ? (burstReviewIndex / burstReviewGroups.length) * 100 : 100"
              color="primary" height="6" rounded class="mb-4"
            />

            <div
              class="tournament-grid"
              :style="{ '--tournament-columns': burstReviewColumns, '--tournament-rows': burstReviewRows }"
            >
              <v-card
                v-for="(photo, index) in burstReviewPhotos"
                :key="photo.id"
                class="tournament-card"
                :class="{ 'is-selected': burstReviewKept.includes(photo.id) }"
                @click="toggleBurstReviewKeep(photo.id)"
              >
                <span class="tournament-card__number">{{ index + 1 === 10 ? 0 : index + 1 }}</span>
                <div class="tournament-card__tools">
                  <v-btn
                    icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                    :aria-label="`${photo.name} を拡大`"
                    @click.stop="openZoom(photo, burstReviewPhotos)"
                  />
                </div>
                <span v-if="burstReviewKept.includes(photo.id)" class="tournament-card__check">
                  <v-icon icon="mdi-check-bold" size="20" />
                </span>
                <span class="tournament-card__confirmed">★{{ photo.rating }}</span>
                <img :src="desktop.photoUrl(photo.path)" :alt="photo.name">
              </v-card>
            </div>

            <v-sheet class="d-flex align-center justify-space-between flex-wrap ga-3 mt-4 pa-3" color="surface-variant" rounded>
              <span class="text-caption text-medium-emphasis">
                残す {{ burstReviewKept.length }} 枚 ・ 下げる {{ burstReviewPhotos.length - burstReviewKept.length }} 枚
              </span>
              <div class="d-flex flex-wrap ga-2">
                <v-btn variant="outlined" :disabled="burstReviewBusy" @click="skipBurstReview">変更しない</v-btn>
                <v-btn
                  color="primary" :loading="burstReviewBusy" :disabled="!burstReviewKept.length"
                  @click="applyBurstReview"
                >この {{ burstReviewKept.length }} 枚を残す</v-btn>
              </div>
            </v-sheet>
          </template>

          <v-card v-else-if="burstReviewLoaded" class="pa-10 text-center text-medium-emphasis">
            まとめられた連写はありません。
          </v-card>
        </template>

        <template v-else-if="view === 'result' && session">
          <div class="text-overline text-primary">★{{ session.targetRating }} の選別が終わりました</div>
          <h1 class="text-h5 text-md-h4">{{ session.survivors.length.toLocaleString() }} 枚が ★{{ Math.min(MAX_RATING, session.targetRating + 1) }} に上がりました</h1>
          <p class="text-medium-emphasis mt-2">
            選ばれなかった写真は ★{{ session.targetRating }} のまま残っています。次はどの星を選別するか、レーティング画面から選べます。
          </p>
          <v-alert v-if="!session.survivors.length" type="warning" variant="tonal" class="mt-5" max-width="680">
            1枚も選ばれませんでした。厳しく見すぎた場合は「1つ戻す」で直前のグループからやり直せます。
          </v-alert>
          <v-card max-width="720" class="mt-6 pa-6">
            <div class="text-body-1 mb-5">
              ★{{ Math.min(MAX_RATING, session.targetRating + 1) }}: <strong>{{ ratingCount(Math.min(MAX_RATING, session.targetRating + 1)).toLocaleString() }} 枚</strong>
              ／ ★{{ session.targetRating }}: <strong>{{ ratingCount(session.targetRating).toLocaleString() }} 枚</strong>
            </div>
            <div class="d-flex flex-wrap ga-3">
              <v-btn
                color="primary" size="large" prepend-icon="mdi-tournament"
                :disabled="ratingCount(Math.min(MAX_RATING, session.targetRating + 1)) < 2"
                @click="openNextRoundDialog(Math.min(MAX_RATING, session.targetRating + 1))"
              >★{{ Math.min(MAX_RATING, session.targetRating + 1) }} をさらに選別</v-btn>
              <v-btn
                variant="outlined" size="large" prepend-icon="mdi-refresh"
                :disabled="ratingCount(session.targetRating) < 2"
                @click="openNextRoundDialog(session.targetRating)"
              >★{{ session.targetRating }} をもう一度見直す</v-btn>
            </div>
            <div class="d-flex flex-wrap ga-3 mt-4">
              <v-btn variant="text" prepend-icon="mdi-star-outline" @click="openResults">レーティングを見る</v-btn>
              <v-btn variant="text" prepend-icon="mdi-undo" :disabled="!session.history.length" @click="undoChoice">1つ戻す</v-btn>
              <v-btn variant="text" prepend-icon="mdi-check" @click="view = 'project'">選別を終了する</v-btn>
            </div>
          </v-card>
        </template>
      </v-container>
    </v-main>

    <!-- 写真ライブラリから受け取る口。ブラウザはフォルダを走査できないので、
         これが取り込みの唯一の入口になる。 -->
    <input
      ref="photoInput" type="file" accept="image/*" multiple
      class="d-none" aria-hidden="true" tabindex="-1"
      @change="onPhotoPicked"
    >

    <v-dialog v-model="createDialog" max-width="620"><v-card title="プロジェクトを作成"><v-card-text class="pt-5">
      <v-text-field v-model="projectName" label="プロジェクト名" :placeholder="folderPath ? fileName(folderPath) : '任意のプロジェクト名'" :hint="canImportPhotos ? '作成したあと「写真を追加」から選びます。' : '空欄ならフォルダ名を使います。'" persistent-hint class="mb-5" />
      <!-- デスクトップはフォルダを参照する。ブラウザは作成後にピッカーで選ぶ。 -->
      <v-text-field v-if="!canImportPhotos" v-model="folderPath" label="写真フォルダ" readonly prepend-inner-icon="mdi-folder-image"><template #append-inner><v-btn variant="outlined" size="small" @click="chooseFolder">選択</v-btn></template></v-text-field>
      <v-alert v-else type="info" variant="tonal" density="comfortable">
        この端末の写真から選びます。写真そのものは端末の外に出ません。
      </v-alert>
    </v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="createDialog = false">キャンセル</v-btn><v-btn variant="outlined" color="primary" :disabled="!canImportPhotos && !folderPath" :loading="loading" @click="createProject">作成</v-btn></v-card-actions></v-card></v-dialog>

    <v-dialog v-model="taskDialog" persistent max-width="520"><v-card><v-card-title>写真を読み込み中</v-card-title><v-card-text class="pt-5"><div class="d-flex justify-space-between text-body-2 mb-3"><span>{{ taskProgress?.message }}</span><span v-if="taskProgress?.total">{{ taskProgress.processed.toLocaleString() }} / {{ taskProgress.total.toLocaleString() }}</span></div><v-progress-linear :model-value="progressValue" :indeterminate="!taskProgress?.total" color="primary" height="10" rounded /><p class="text-caption text-medium-emphasis mt-5 mb-0">読み込みのあと、連写の解析はバックグラウンドで少しずつ進みます。キャンセルしても、完了済みの読み込み結果は保持されます。</p></v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="cancelTask">キャンセル</v-btn></v-card-actions></v-card></v-dialog>

    <!-- 選別の途中で1グループの表示枚数を変える。済んだぶんはそのまま残る。 -->
    <v-dialog v-model="groupSizeDialog" max-width="520">
      <v-card title="1グループの表示枚数">
        <v-card-text class="pt-5">
          <v-slider v-model="pendingGroupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="1グループの表示枚数">
            <template #append><v-text-field v-model.number="pendingGroupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template>
          </v-slider>
          <p class="text-caption text-medium-emphasis mt-3 mb-0">
            まだ見ていない写真だけを詰め直します。ここまでの選択と「1つ戻す」の履歴はそのまま残ります。
          </p>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" @click="groupSizeDialog = false">キャンセル</v-btn>
          <v-btn color="primary" @click="applyGroupSize(pendingGroupSize)">この枚数にする</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- 拡大表示。写真だけを見せたいので余計な枠は置かない。
         ← → だけ前後送りに使い、それ以外のキーでは閉じる。 -->
    <v-overlay
      :model-value="!!zoomPhoto"
      class="zoom-overlay align-center justify-center"
      scrim="#000000"
      opacity="0.94"
      :z-index="3000"
      @click="zoomPhoto = null"
      @update:model-value="value => { if (!value) zoomPhoto = null }"
    >
      <div v-if="zoomPhoto" class="zoom-overlay__inner">
        <!-- 送りボタンは写真の外に置く。写真の上に重ねると、閉じるつもりの
             クリックが送りに化ける。 -->
        <div class="zoom-overlay__stage">
          <v-btn
            class="zoom-overlay__step" icon="mdi-chevron-left" variant="text" size="large"
            aria-label="前の写真" :disabled="zoomIndex <= 0"
            @click.stop="stepZoom(-1)"
          />
          <img :src="desktop.photoUrl(zoomPhoto.path)" :alt="zoomPhoto.name">
          <v-btn
            class="zoom-overlay__step" icon="mdi-chevron-right" variant="text" size="large"
            aria-label="次の写真" :disabled="zoomIndex < 0 || zoomIndex >= zoomList.length - 1"
            @click.stop="stepZoom(1)"
          />
        </div>
        <div class="zoom-overlay__caption text-caption">
          <span v-if="zoomList.length > 1" class="zoom-overlay__position">{{ zoomIndex + 1 }} / {{ zoomList.length }}</span>
          {{ zoomPhoto.name }}
          <template v-if="zoomList.length > 1"> ・ ← → で前後</template>
          ・ クリックか他のキーで閉じる
        </div>
      </div>
    </v-overlay>

    <!-- 選別結果をライブラリ側へ渡す。ブラウザからは写真ライブラリを
         直接書き換えられないので、どれも利用者の操作を経由する。 -->
    <v-dialog v-model="shareDialog" max-width="640" scrollable>
      <v-card title="選別結果を書き出す">
        <v-card-text class="pt-5">
          <v-alert v-if="shareError" type="error" density="compact" class="mb-4">{{ shareError }}</v-alert>
          <v-alert v-if="shareMessage" type="success" density="compact" class="mb-4">{{ shareMessage }}</v-alert>

          <div class="text-caption text-medium-emphasis mb-1">どの星を書き出しますか？</div>
          <v-btn-toggle v-model="shareRatings" multiple density="comfortable" variant="outlined" divided class="mb-5">
            <v-btn v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating" :value="rating">★{{ rating }}</v-btn>
          </v-btn-toggle>

          <v-alert type="info" variant="tonal" density="comfortable" class="mb-5">
            写真アプリに星はなく「お気に入り(♡)」だけです。星そのものはこのアプリが持ち続けます。
            <strong>原本はこの端末で取り込んだ回のあいだだけ手元にあります。</strong>
            読み込み直したあとは、写真を選び直すと書き出せます。
          </v-alert>

          <v-list class="bg-transparent">
            <v-list-item class="px-0">
              <v-list-item-title>共有シートで渡す</v-list-item-title>
              <v-list-item-subtitle class="text-wrap">
                「画像を保存」で写真アプリへ、「ファイルに保存」でファイルアプリへ。
                写真アプリには<strong>重複として</strong>入り、星は付きません（{{ SHARE_FILE_LIMIT }} 枚まで）。
              </v-list-item-subtitle>
              <template #append>
                <v-btn variant="outlined" :loading="shareBusy" :disabled="!shareRatings.length" @click="shareSelectedPhotos">共有</v-btn>
              </template>
            </v-list-item>
            <v-divider />
            <v-list-item class="px-0">
              <v-list-item-title>星ごとに ZIP で書き出す</v-list-item-title>
              <v-list-item-subtitle class="text-wrap">
                <code>star-5/</code> のように星ごとのフォルダに分けます。枚数が多いときや PC に渡すときはこちら。
              </v-list-item-subtitle>
              <template #append>
                <v-btn variant="outlined" :loading="shareBusy" :disabled="!shareRatings.length" @click="exportZipByRating">ZIP</v-btn>
              </template>
            </v-list-item>
          </v-list>
        </v-card-text>
        <v-divider />
        <v-card-actions class="pa-4">
          <v-spacer />
          <v-btn variant="text" @click="shareDialog = false">閉じる</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- レートの移動。既定は全選択で、外したい写真だけチェックを解く。 -->
    <v-dialog v-model="moveDialog" fullscreen transition="dialog-bottom-transition" scrollable>
      <v-card>
        <v-toolbar color="surface" density="comfortable">
          <v-toolbar-title>★{{ moveFrom }} の写真をどのレートへ移しますか？</v-toolbar-title>
          <v-spacer />
          <v-btn icon="mdi-close" aria-label="閉じる" @click="moveDialog = false" />
        </v-toolbar>
        <v-card-text class="pt-5">
          <v-alert v-if="moveError" type="error" density="compact" class="mb-4">{{ moveError }}</v-alert>
          <!-- 選別中に星を動かすと、進行中のラウンドの前提が変わる。 -->
          <v-alert v-if="session" type="warning" variant="tonal" density="compact" class="mb-4">
            選別が進行中です。星を動かすと、いま選別している対象と食い違うことがあります。
          </v-alert>

          <div class="d-flex flex-wrap align-center ga-4 mb-5">
            <div>
              <div class="text-caption text-medium-emphasis mb-1">移動先のレート</div>
              <v-btn-toggle v-model="moveTo" density="comfortable" variant="outlined" divided mandatory>
                <v-btn v-for="rating in [0, 1, 2, 3, 4, 5]" :key="rating" :value="rating" :disabled="rating === moveFrom">★{{ rating }}</v-btn>
              </v-btn-toggle>
            </div>
            <v-spacer />
            <div class="d-flex align-center ga-2">
              <v-btn size="small" variant="text" :disabled="moveSelectedCount >= moveTotal" @click="setMoveSelectAll(true)">全選択</v-btn>
              <v-btn size="small" variant="text" :disabled="!moveSelectedCount" @click="setMoveSelectAll(false)">全解除</v-btn>
              <v-btn-toggle v-model="moveDensity" density="comfortable" variant="outlined" divided mandatory>
                <v-btn v-for="option in densityOptions" :key="option.label" :value="option.value" :icon="option.icon" :aria-label="`一覧を${option.label}で表示`" />
              </v-btn-toggle>
            </div>
          </div>

          <div v-if="movePhotos.length" class="result-grid" :class="gridClass(moveDensity)" :style="gridStyle(moveDensity)">
            <div
              v-for="photo in movePhotos" :key="photo.id" class="result-tile"
              :class="{ 'is-unpicked': !isMoveSelected(photo.id) }"
              role="button" tabindex="0"
              @click="toggleMoveSelection(photo.id)" @keydown.enter="toggleMoveSelection(photo.id)"
            >
              <v-checkbox-btn class="result-tile__pick" :model-value="isMoveSelected(photo.id)" density="compact" :aria-label="`${photo.name} を移動対象にする`" @click.stop="toggleMoveSelection(photo.id)" />
              <img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy">
              <div class="result-tile__name text-caption">{{ photo.name }}</div>
            </div>
          </div>
          <v-card v-else-if="!moveBusy" class="pa-10 text-center text-medium-emphasis">★{{ moveFrom }} の写真はありません。</v-card>

          <div class="d-flex justify-center mt-5">
            <v-btn v-if="movePhotos.length < moveTotal" variant="outlined" :loading="moveBusy" @click="loadMovePage()">
              さらに読み込む（{{ movePhotos.length.toLocaleString() }} / {{ moveTotal.toLocaleString() }}）
            </v-btn>
            <span v-else-if="movePhotos.length" class="text-caption text-medium-emphasis">{{ moveTotal.toLocaleString() }} 枚すべて表示しました</span>
          </div>
          <!-- まだ読み込んでいない写真も移動の対象に入る。件数は総数から数える。 -->
          <p v-if="movePhotos.length < moveTotal" class="text-caption text-medium-emphasis text-center mt-2">
            表示していない写真も対象に含まれます。外したい写真だけを読み込んでチェックを外してください。
          </p>
        </v-card-text>
        <v-divider />
        <v-card-actions class="pa-4">
          <div class="text-body-2">
            <strong>{{ moveSelectedCount.toLocaleString() }} 枚</strong> を ★{{ moveFrom }} → ★{{ moveTo }} へ移します
          </div>
          <v-spacer />
          <v-btn variant="text" @click="moveDialog = false">やめる</v-btn>
          <v-btn color="primary" :loading="moveBusy" :disabled="!moveSelectedCount || moveFrom === moveTo" @click="runMove">移動する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- まとめられた連写の中身。ここで代表を差し替えられる。 -->
    <v-dialog v-model="burstDialog" fullscreen transition="dialog-bottom-transition" scrollable>
      <v-card>
        <v-toolbar color="surface" density="comfortable">
          <v-toolbar-title>連写 {{ burstOriginal.length }} 枚 — {{ burstBlocks.length }} つのまとまり</v-toolbar-title>
          <v-spacer />
          <v-btn icon="mdi-close" aria-label="閉じる" @click="burstDialog = false" />
        </v-toolbar>
        <v-card-text class="pt-5">
          <p class="text-body-2 text-medium-emphasis mb-4">
            写真を選んで「切り離す」と、選んだぶんが別のまとまりになります。
            まとまりの境目の「つなぐ」で元に戻せます。薄い写真はまとめの外にある近くの写真で、
            つなぐと取り込めます。
          </p>

          <v-progress-linear v-if="burstBusy" indeterminate color="primary" class="mb-4" />

          <!-- まとまりごとに枠で囲む。境目そのものが操作の対象なので、
               間に「つなぐ」を置いて、切れているのが見えるようにする。 -->
          <template v-for="(block, blockIndex) in burstBlocks" :key="block[0]">
            <div v-if="blockIndex > 0" class="burst-seam">
              <span class="burst-seam__line" />
              <v-btn
                size="small" variant="outlined" prepend-icon="mdi-link-variant"
                @click="joinBurstAt(boundaryBefore(block[0]!))"
              >つなぐ</v-btn>
              <span class="burst-seam__line" />
            </div>

            <div class="burst-block" :class="{ 'is-outside': isOutsideBurst(block) }">
              <div class="text-caption text-medium-emphasis mb-2">
                {{ isOutsideBurst(block) ? 'まとめの外' : `まとまり ${blockIndex + 1}` }}
                ・ {{ block.length }} 枚
              </div>
              <div
                class="tournament-grid burst-review-grid"
                :style="{ '--tournament-columns': columnsFor(block.length), '--tournament-rows': 1 }"
              >
                <v-card
                  v-for="photoId in block"
                  :key="photoId"
                  class="tournament-card"
                  :class="{
                    'is-selected': burstPicked.includes(photoId),
                    'is-representative': block.length > 1 && representativeOf(block) === photoId
                  }"
                  @click="toggleBurstPick(photoId)"
                >
                  <div class="tournament-card__tools">
                    <v-btn
                      :icon="burstPhotoOf(photoId)?.rating === MAX_RATING ? 'mdi-star' : 'mdi-star-outline'"
                      size="x-small" variant="flat"
                      :color="burstPhotoOf(photoId)?.rating === MAX_RATING ? 'secondary' : undefined"
                      :aria-label="`${burstPhotoOf(photoId)?.name} を★${MAX_RATING} で確定`"
                      @click.stop="confirmBurstPhoto(photoId)"
                    />
                    <v-btn
                      icon="mdi-thumb-down-outline" size="x-small" variant="flat"
                      :aria-label="`${burstPhotoOf(photoId)?.name} を脱落させる`"
                      @click.stop="dropBurstPhoto(photoId)"
                    />
                    <v-btn
                      icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                      :aria-label="`${burstPhotoOf(photoId)?.name} を拡大`"
                      @click.stop="openZoom(burstPhotoOf(photoId), burstPhotos)"
                    />
                  </div>
                  <span v-if="block.length > 1 && representativeOf(block) === photoId" class="tournament-card__confirmed">代表</span>
                  <span v-if="burstPicked.includes(photoId)" class="tournament-card__check">
                    <v-icon icon="mdi-check-bold" size="20" />
                  </span>
                  <span class="tournament-card__number">★{{ burstPhotoOf(photoId)?.rating ?? 0 }}</span>
                  <img :src="desktop.photoUrl(burstPhotoOf(photoId)?.path ?? '')" :alt="burstPhotoOf(photoId)?.name">
                </v-card>
              </div>
            </div>
          </template>

          <v-card v-if="!burstBusy && !burstPhotos.length" class="pa-10 text-center text-medium-emphasis">
            この連写を読み込めませんでした。
          </v-card>
        </v-card-text>

        <v-card-actions class="pa-5 flex-wrap ga-2">
          <span class="text-caption text-medium-emphasis">{{ burstPicked.length }} 枚を選択中</span>
          <v-spacer />
          <v-btn
            variant="text" :disabled="burstPicked.length !== 1"
            @click="makeBurstRepresentative(burstPicked[0]!)"
          >代表にする</v-btn>
          <v-btn variant="text" :disabled="burstBusy" @click="scatterBurst">全部バラバラに</v-btn>
          <v-btn
            variant="outlined" prepend-icon="mdi-arrow-split-vertical"
            :disabled="!burstPicked.length" @click="splitBurstSelection"
          >切り離す</v-btn>
          <v-btn color="primary" :loading="burstBusy" @click="applyBurstShape">この形で戻る</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- 選別対象は星で決まる。枚数だけここで決める。 -->
    <v-dialog v-model="nextRoundDialog" max-width="520">
      <v-card :title="`★${nextRoundRating} を選別`">
        <v-card-text class="pt-5">
          <p class="text-body-2 mb-5">
            ★{{ nextRoundRating }} の <strong>{{ ratingCount(nextRoundRating).toLocaleString() }} 枚</strong>が対象です。
            選ばれた写真は ★{{ Math.min(MAX_RATING, nextRoundRating + 1) }} に上がり、選ばれなかった写真は ★{{ nextRoundRating }} のまま残ります。
          </p>
          <v-slider v-model="nextRoundGroupSize" class="selection-slider" :min="groupLimits.min" :max="groupLimits.max" :step="1" thumb-label aria-label="1グループの表示枚数">
            <template #append><v-text-field v-model.number="nextRoundGroupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template>
          </v-slider>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" @click="nextRoundDialog = false">キャンセル</v-btn>
          <v-btn color="primary" prepend-icon="mdi-play" @click="startRatingSelection(nextRoundRating)">始める</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog v-model="restartDialog" max-width="520">
      <v-card title="最初からやり直しますか？">
        <v-card-text class="pt-5">
          <v-alert type="warning" variant="tonal" density="comfortable">
            すべての写真の星が <strong>0 に戻ります</strong>。選別のやり直しは元に戻せません。
          </v-alert>
          <p class="text-caption text-medium-emphasis mt-4 mb-0">
            解析結果（連写の判定やサムネイル）は消えないので、次の選別もすぐ始められます。
            すでに書き出したフォルダやメタデータには影響しません。
          </p>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" :disabled="restartBusy" @click="restartDialog = false">キャンセル</v-btn>
          <v-btn color="error" :loading="restartBusy" @click="restartFromScratch">星を全部消してやり直す</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- フォルダ分け。移動は原本フォルダから写真が消えるので警告を強くする。 -->
    <v-dialog v-model="exportDialog" max-width="640">
      <v-card title="レーティングごとにフォルダ分け">
        <v-card-text class="pt-5">
          <v-text-field v-model="exportDestination" label="出力先フォルダ" readonly prepend-inner-icon="mdi-folder-outline" class="mb-4">
            <template #append-inner><v-btn variant="outlined" size="small" @click="chooseExportDestination">選択</v-btn></template>
          </v-text-field>

          <div class="text-subtitle-2 mb-2">書き出す星</div>
          <div class="d-flex flex-wrap ga-2 mb-5">
            <v-chip
              v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
              :color="exportRatings.includes(rating) ? 'primary' : undefined"
              :variant="exportRatings.includes(rating) ? 'flat' : 'outlined'"
              @click="exportRatings.includes(rating) ? exportRatings.splice(exportRatings.indexOf(rating), 1) : exportRatings.push(rating)"
            >★{{ rating }}（{{ ratingCount(rating).toLocaleString() }}）</v-chip>
          </div>

          <v-radio-group v-model="exportMode" hide-details class="mb-2">
            <v-radio value="copy" label="コピーする（原本はそのまま残る）" />
            <v-radio value="move" label="移動する（原本フォルダから写真が無くなる）" />
          </v-radio-group>
          <v-alert v-if="exportMode === 'move'" type="warning" variant="tonal" density="comfortable" class="mt-3">
            移動すると<strong>元のフォルダから写真が無くなります</strong>。移動後はプロジェクトの索引が古くなるため、
            「写真を再読み込み」が必要になります。
          </v-alert>

          <v-alert v-if="exportError" type="error" variant="tonal" class="mt-4" closable @click:close="exportError = ''">
            {{ exportError }}
          </v-alert>
          <v-alert v-if="exportResult" :type="exportResult.failed ? 'warning' : 'success'" variant="tonal" class="mt-4">
            {{ exportResult.processed.toLocaleString() }} 枚を{{ exportMode === 'move' ? '移動' : 'コピー' }}しました。
            <template v-if="exportResult.skipped">見つからず飛ばした写真 {{ exportResult.skipped }} 枚。</template>
            <template v-if="exportResult.failed">失敗 {{ exportResult.failed }} 枚。</template>
            <ul v-if="exportResult.errors.length" class="mt-2 text-caption">
              <li v-for="line in exportResult.errors" :key="line">{{ line }}</li>
            </ul>
          </v-alert>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" :disabled="exportBusy" @click="exportDialog = false">閉じる</v-btn>
          <v-btn
            :color="exportMode === 'move' ? 'error' : 'primary'"
            :loading="exportBusy" :disabled="!exportDestination || !exportRatings.length"
            @click="runExport"
          >{{ exportMode === 'move' ? '移動する' : 'コピーする' }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- メタデータ書き込み。原本を書き換えるので、明示的な同意を必須にする。 -->
    <v-dialog v-model="metadataDialog" max-width="640">
      <v-card title="レーティングをメタデータに反映">
        <v-card-text class="pt-5">
          <v-alert type="warning" variant="tonal" density="comfortable" class="mb-4">
            <strong>写真の原本を書き換えます。</strong>
            星は XMP（<code>xmp:Rating</code>）として写真の中に書き込まれ、Lightroom や Bridge などが読み取れます。
            撮影情報（EXIF）と画像そのものには手を加えません。
          </v-alert>
          <p class="text-caption text-medium-emphasis mb-4">
            書き込みは、いったん別ファイルを作って画像として開けるか確かめてから置き換えます。
            途中で失敗しても原本はそのまま残ります。対象は JPEG のみで、PNG と WebP は飛ばします。
          </p>

          <div class="text-subtitle-2 mb-2">書き込む星</div>
          <div class="d-flex flex-wrap ga-2 mb-5">
            <v-chip
              v-for="rating in [5, 4, 3, 2, 1, 0]" :key="rating"
              :color="metadataRatings.includes(rating) ? 'primary' : undefined"
              :variant="metadataRatings.includes(rating) ? 'flat' : 'outlined'"
              @click="metadataRatings.includes(rating) ? metadataRatings.splice(metadataRatings.indexOf(rating), 1) : metadataRatings.push(rating)"
            >★{{ rating }}（{{ ratingCount(rating).toLocaleString() }}）</v-chip>
          </div>

          <v-checkbox
            v-model="metadataAcknowledged" hide-details density="comfortable"
            label="原本が書き換わることを理解しました"
          />

          <v-alert v-if="metadataError" type="error" variant="tonal" class="mt-4" closable @click:close="metadataError = ''">
            {{ metadataError }}
          </v-alert>
          <v-alert v-if="metadataResult" :type="metadataResult.failed ? 'warning' : 'success'" variant="tonal" class="mt-4">
            {{ metadataResult.processed.toLocaleString() }} 枚に書き込みました。
            <template v-if="metadataResult.skipped">対象外で飛ばした写真 {{ metadataResult.skipped }} 枚。</template>
            <template v-if="metadataResult.failed">失敗 {{ metadataResult.failed }} 枚（原本は変更していません）。</template>
            <ul v-if="metadataResult.errors.length" class="mt-2 text-caption">
              <li v-for="line in metadataResult.errors" :key="line">{{ line }}</li>
            </ul>
          </v-alert>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" :disabled="metadataBusy" @click="metadataDialog = false">閉じる</v-btn>
          <v-btn color="error" :loading="metadataBusy" :disabled="!metadataAcknowledged || !metadataRatings.length" @click="runMetadataWrite">
            書き込む
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-dialog v-model="deleteDialog" max-width="560">
      <v-card title="プロジェクトを削除しますか？">
        <v-card-text class="pt-5">
          <div class="text-body-1 mb-3">{{ deleteTarget?.name }}</div>
          <v-alert type="info" variant="tonal" density="comfortable">
            <strong>写真の原本は削除されません。</strong>
            消えるのは、このアプリが持っている写真の索引・選別の履歴・サムネイルだけです。
            フォルダ <code>{{ deleteTarget?.folderPath }}</code> はそのまま残ります。
          </v-alert>
          <p class="text-caption text-medium-emphasis mt-4 mb-0">選別の途中経過は元に戻せません。</p>
        </v-card-text>
        <v-card-actions class="pa-5 pt-2">
          <v-spacer />
          <v-btn variant="outlined" :disabled="deleteBusy" @click="deleteDialog = false">キャンセル</v-btn>
          <v-btn color="error" :loading="deleteBusy" @click="confirmDeleteProject">削除する</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </v-app>
</template>
