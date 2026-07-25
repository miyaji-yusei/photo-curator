<script setup lang="ts">
import type {
  BurstPair, ExportReport, Photo, PhotoSort, Project, ProjectProgress,
  SelectionResult, SelectionSession, SelectionSummary, TournamentSettings
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import {
  collapseBursts,
  makeSession,
  prepareRound,
  regroupRemaining,
  setBurstRepresentative,
  undoLastStep
} from '~/utils/tournament'
import {
  DEFAULT_THRESHOLD_OPTIONS,
  MAX_HASH_DISTANCE,
  applyAnswer,
  inferThreshold,
  nextPair,
  shouldStop,
  skipPair
} from '~/utils/burstThreshold'

type View = 'home' | 'project' | 'method' | 'settings' | 'burst-threshold' | 'burst-preview' | 'tournament' | 'result' | 'results'

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
const settings = reactive<TournamentSettings>({ groupSize: 10, groupBursts: false })
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
/** まとめの中身も選別画面と同じ組み方にして、同じ大きさで見比べられるようにする。 */
const burstColumns = computed(() => columnsFor(burstPhotos.value.length))
const burstRows = computed(() =>
  Math.max(1, Math.ceil(burstPhotos.value.length / burstColumns.value))
)
/** 写真を見比べている画面かどうか。余白の詰め方を変える。 */
const isSelecting = computed(() => view.value === 'tournament' || view.value === 'burst-threshold')

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

// 拡大表示・まとめの展開
const zoomPhoto = ref<Photo | null>(null)
const burstDialog = ref(false)
const burstOwner = ref<Photo | null>(null)
const burstPhotos = ref<Photo[]>([])

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
  if (desktop.isDesktop()) projects.value = await desktop.listProjects()
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
    if (!project.photoCount && !scanRunning.value) {
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

async function createProject() {
  if (!folderPath.value) return
  loading.value = true
  try {
    const project = await desktop.createProject(projectName.value.trim() || fileName(folderPath.value), folderPath.value)
    createDialog.value = false
    projectName.value = ''
    folderPath.value = ''
    await refreshProjects()
    await openProject(project)
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : 'プロジェクトを作成できませんでした。'
  } finally {
    loading.value = false
  }
}

async function startScan() {
  if (!activeProject.value || taskDialog.value) return
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
  settings.groupSize = prior?.groupSize ?? 10
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
  const chosen = [...session.value.selectedInGroup]
  const group = [...currentGroup.value]
  session.value.history.push({ groupIndex: session.value.groupIndex, chosen })
  for (const id of chosen) {
    session.value.survivors.push(id)
    // 星は 1 ラウンド通過ごとに +1 で、MAX_RATING で頭打ち。
    session.value.ratings[id] = Math.min(MAX_RATING, (session.value.ratings[id] ?? 0) + 1)
  }
  await persistGroupResults(group)
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
 */
async function confirmPhoto(photoId: string) {
  if (!session.value) return
  // 星を一気に最大へ。★5 は以降どの星の選別にも出てこないので、
  // 「確定」という別状態を持たなくてよい。
  session.value.ratings[photoId] = MAX_RATING
  session.value.selectedInGroup = [photoId]
  await confirmChoices()
}

function openZoom(photo: Photo | null) {
  if (photo) zoomPhoto.value = photo
}

/** まとめられた連写の中身を開く。代表の差し替えもここから。 */
async function openBurst(photo: Photo | null) {
  if (!photo || !session.value || burstSizeOf(photo.id) < 2) return
  const ids = session.value.burstMembers[photo.id] ?? []
  burstOwner.value = photo
  burstPhotos.value = activeProject.value
    ? await desktop.getPhotosByIds(activeProject.value.id, ids)
    : []
  burstDialog.value = true
}

/** まとめの代表を差し替える。選別画面に出る1枚が変わる。 */
async function chooseRepresentative(photoId: string) {
  if (!session.value || !burstOwner.value) return
  setBurstRepresentative(session.value, burstOwner.value.id, photoId)
  burstDialog.value = false
  burstOwner.value = null
  await loadCurrentPhotos()
  await saveSession()
}

/** 直前の1グループぶんの判断を取り消してやり直す。 */
async function undoChoice() {
  if (!session.value || !session.value.history.length) return
  undoLastStep(session.value)
  // 戻したぶんの星と落選を DB からも取り消す。まだ判定していない状態に戻す。
  const group = session.value.groups[session.value.groupIndex] ?? []
  await persistGroupResults(group)
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
  nextRoundGroupSize.value = session.value?.settings.groupSize ?? 10
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

async function resumeSession() {
  if (!session.value) return openSettings()
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
  pendingGroupSize.value = session.value?.settings.groupSize ?? 10
  groupSizeDialog.value = true
}

async function cancelTask() {
  const progress = taskProgress.value
  if (!progress) return
  await desktop.cancelProjectTask(progress.projectId, progress.task)
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
  if (!session.value || event.target instanceof HTMLInputElement) return

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
  if (event.ctrlKey || event.metaKey) openZoom(photo)
  else if (event.shiftKey) void confirmPhoto(photo.id)
  else if (event.altKey) void openBurst(photo)
  else void toggleChoice(photo.id)
}

/** 拡大表示は、どのキーでも閉じる。 */
function onZoomKeydown(event: KeyboardEvent) {
  if (!zoomPhoto.value) return
  event.preventDefault()
  event.stopPropagation()
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
          <div class="d-flex align-center justify-space-between flex-wrap ga-4 mb-7"><div><v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0" @click="view = 'home'">ホーム</v-btn><h1 class="text-h4 font-weight-bold">{{ activeProject.name }}</h1><p class="text-body-2 text-medium-emphasis mt-1">{{ activeProject.folderPath }}</p></div><div class="d-flex flex-wrap ga-2"><v-btn variant="text" prepend-icon="mdi-delete-outline" @click="askDeleteProject(activeProject)">削除</v-btn><v-btn v-if="hasSelectionData" variant="text" prepend-icon="mdi-star-outline" @click="openResults">選別結果を見る</v-btn><v-btn variant="outlined" prepend-icon="mdi-refresh" :loading="scanRunning" @click="startScan">写真を再読み込み</v-btn><v-btn color="primary" prepend-icon="mdi-play" :disabled="!activeProject.photoCount" @click="session ? resumeSession() : enterMethod()">{{ session ? '選別を再開' : '選別を開始' }}</v-btn></div></div>
          <v-card class="mb-6"><v-card-text class="d-flex align-center ga-5"><v-avatar color="primary" size="50"><v-icon color="black" icon="mdi-image-multiple" /></v-avatar><div><div class="text-h6">{{ activeProject.photoCount.toLocaleString() }} 枚の写真</div><div class="text-body-2 text-medium-emphasis">サブフォルダも含めて参照します。写真ファイルは変更しません。</div></div></v-card-text></v-card>
          <div v-if="previewPhotos.length" class="photo-grid"><div v-for="photo in previewPhotos" :key="photo.id" class="photo-tile"><img :src="desktop.photoThumbnailUrl(photo)" :alt="photo.name" loading="lazy"><div class="photo-tile__caption">{{ photo.relativePath }}</div></div></div>
          <v-alert v-if="previewTotal > previewPhotos.length" type="info" variant="tonal" class="mt-5">表示負荷を抑えるため、最初の {{ previewPhotos.length }} 枚だけを表示しています。選別にはすべての写真が含まれます。</v-alert>
        </template>

        <template v-else-if="view === 'method'">
          <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'project'">プロジェクトへ戻る</v-btn><div class="text-overline text-primary">選別を開始</div><h1 class="text-h4 mb-6">方法を選んでください</h1>
          <v-row><v-col cols="12" md="6"><v-card class="choice-card pa-6 h-100" role="button" tabindex="0" @click="openSettings" @keydown.enter="openSettings"><v-icon color="primary" size="40" icon="mdi-tournament" /><div class="text-h5 mt-5">トーナメントで選別</div><p class="text-medium-emphasis mt-2">複数の写真から、次のラウンドに進める1枚を選びます。</p><v-btn color="primary" class="mt-4">設定へ進む</v-btn></v-card></v-col><v-col cols="12" md="6"><v-card class="choice-card choice-card--disabled pa-6 h-100" aria-disabled="true"><v-icon size="40" icon="mdi-play-box-outline" /><div class="text-h5 mt-5">スライドショーで選別</div><p class="text-medium-emphasis mt-2">1枚ずつ直感的に選べる方法です。</p><v-chip class="mt-4">準備中</v-chip></v-card></v-col></v-row>
        </template>

        <template v-else-if="view === 'settings'">
          <v-btn variant="text" prepend-icon="mdi-arrow-left" class="px-0 mb-4" @click="view = 'method'">方法の選択へ戻る</v-btn><div class="text-overline text-primary">Tournament setup</div><h1 class="text-h4 mb-6">トーナメントの設定</h1>
          <v-card max-width="720" class="pa-6 settings-card"><div class="text-subtitle-1 font-weight-medium mb-5">何枚から選びますか？</div><v-slider v-model="settings.groupSize" class="selection-slider" :min="2" :max="10" :step="1" thumb-label aria-label="何枚から選ぶか"><template #append><v-text-field v-model.number="settings.groupSize" density="compact" variant="outlined" style="width: 86px" hide-details suffix="枚" /></template></v-slider><p class="text-caption text-medium-emphasis mt-2">少ないほど比較は丁寧に、多いほどテンポよく進みます。</p><v-divider class="my-7" /><v-switch v-model="settings.groupBursts" color="primary" label="事前にバースト写真（連写）をまとめる" hint="最大 8 問だけ答えると、残りは同じ基準で自動的にまとまります。" persistent-hint />
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

              <!-- 選択のクリックと切り分けるため、拡大は右上に置く。 -->
              <div class="tournament-card__tools">
                <v-btn
                  icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                  :aria-label="`${photo.name} を拡大`"
                  @click.stop="openZoom(photo)"
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
            <div class="text-caption text-medium-emphasis">
              1〜0 選ぶ ・ Ctrl+数字 拡大 ・ Shift+数字 確定 ・ Alt+数字 まとめを開く ・ Enter 確定 ・ ⌫ 戻す
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
              <v-btn
                size="small" variant="outlined" prepend-icon="mdi-tournament"
                :disabled="ratingCount(rating) < 2"
                @click.stop="openNextRoundDialog(rating)"
              >この {{ ratingCount(rating).toLocaleString() }} 枚を選別</v-btn>
            </div>
          </div>

          <div class="d-flex flex-wrap align-center ga-3 mt-6 mb-4">
            <v-btn-toggle v-model="resultsSort" density="comfortable" variant="outlined" divided mandatory @update:model-value="loadResultsPage(true)">
              <v-btn value="rating">星の高い順</v-btn>
              <v-btn value="name">ファイル名順</v-btn>
            </v-btn-toggle>
            <v-chip v-if="resultsRating !== null" closable color="primary" variant="flat" @click:close="selectResultsRating(null)">
              ★{{ resultsRating }} だけ表示中
            </v-chip>
            <v-spacer />
            <v-btn variant="outlined" prepend-icon="mdi-folder-move-outline" @click="exportDialog = true">フォルダ分け</v-btn>
            <v-btn variant="outlined" prepend-icon="mdi-tag-text-outline" @click="metadataDialog = true">メタデータに反映</v-btn>
            <v-btn variant="text" prepend-icon="mdi-restart" @click="restartDialog = true">最初からやり直す</v-btn>
          </div>

          <div v-if="resultsPhotos.length" class="result-grid">
            <div
              v-for="photo in resultsPhotos" :key="photo.id" class="result-tile"
              :class="{ 'is-confirmed': photo.rating >= MAX_RATING, 'is-eliminated': photo.rating === 0 }"
              role="button" tabindex="0"
              @click="openZoom(photo)" @keydown.enter="openZoom(photo)"
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

    <v-dialog v-model="createDialog" max-width="620"><v-card title="プロジェクトを作成"><v-card-text class="pt-5"><v-text-field v-model="projectName" label="プロジェクト名" :placeholder="folderPath ? fileName(folderPath) : '任意のプロジェクト名'" hint="空欄ならフォルダ名を使います。" persistent-hint class="mb-5" /><v-text-field v-model="folderPath" label="写真フォルダ" readonly prepend-inner-icon="mdi-folder-image"><template #append-inner><v-btn variant="outlined" size="small" @click="chooseFolder">選択</v-btn></template></v-text-field></v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="createDialog = false">キャンセル</v-btn><v-btn variant="outlined" color="primary" :disabled="!folderPath" :loading="loading" @click="createProject">作成</v-btn></v-card-actions></v-card></v-dialog>

    <v-dialog v-model="taskDialog" persistent max-width="520"><v-card><v-card-title>写真を読み込み中</v-card-title><v-card-text class="pt-5"><div class="d-flex justify-space-between text-body-2 mb-3"><span>{{ taskProgress?.message }}</span><span v-if="taskProgress?.total">{{ taskProgress.processed.toLocaleString() }} / {{ taskProgress.total.toLocaleString() }}</span></div><v-progress-linear :model-value="progressValue" :indeterminate="!taskProgress?.total" color="primary" height="10" rounded /><p class="text-caption text-medium-emphasis mt-5 mb-0">読み込みのあと、連写の解析はバックグラウンドで少しずつ進みます。キャンセルしても、完了済みの読み込み結果は保持されます。</p></v-card-text><v-card-actions class="pa-5 pt-2"><v-spacer /><v-btn variant="outlined" @click="cancelTask">キャンセル</v-btn></v-card-actions></v-card></v-dialog>

    <!-- 選別の途中で1グループの表示枚数を変える。済んだぶんはそのまま残る。 -->
    <v-dialog v-model="groupSizeDialog" max-width="520">
      <v-card title="1グループの表示枚数">
        <v-card-text class="pt-5">
          <v-slider v-model="pendingGroupSize" class="selection-slider" :min="2" :max="10" :step="1" thumb-label aria-label="1グループの表示枚数">
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

    <!-- 拡大表示。写真だけを見せたいので余計な枠は置かない。どのキーでも閉じる。 -->
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
        <img :src="desktop.photoUrl(zoomPhoto.path)" :alt="zoomPhoto.name">
        <div class="zoom-overlay__caption text-caption">{{ zoomPhoto.name }} ・ クリックか任意のキーで閉じる</div>
      </div>
    </v-overlay>

    <!-- まとめられた連写の中身。ここで代表を差し替えられる。 -->
    <v-dialog v-model="burstDialog" fullscreen transition="dialog-bottom-transition" scrollable>
      <v-card>
        <v-toolbar color="surface" density="comfortable">
          <v-toolbar-title>まとめられた {{ burstPhotos.length }} 枚</v-toolbar-title>
          <v-spacer />
          <v-btn icon="mdi-close" aria-label="閉じる" @click="burstDialog = false" />
        </v-toolbar>
        <v-card-text class="pt-5">
          <p class="text-body-2 text-medium-emphasis mb-5">
            選別画面に出るのは「代表」の1枚です。写真をクリックすると代表が入れ替わります。
            右上の虫めがねで拡大できます。
          </p>
          <!-- 選別画面と同じ組み方・同じ大きさ。見比べる作業は同じなので、
               別のレイアウトにする理由がない。 -->
          <div
            class="tournament-grid burst-review-grid"
            :style="{ '--tournament-columns': burstColumns, '--tournament-rows': burstRows }"
          >
            <v-card
              v-for="(photo, index) in burstPhotos"
              :key="photo.id"
              class="tournament-card"
              :class="{ 'is-representative': index === 0 }"
              @click="chooseRepresentative(photo.id)"
            >
              <div class="tournament-card__tools">
                <v-btn
                  icon="mdi-magnify-plus-outline" size="x-small" variant="flat"
                  :aria-label="`${photo.name} を拡大`"
                  @click.stop="openZoom(photo)"
                />
              </div>
              <span v-if="index === 0" class="tournament-card__confirmed">代表</span>
              <img :src="desktop.photoUrl(photo.path)" :alt="photo.name">
            </v-card>
          </div>
        </v-card-text>
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
          <v-slider v-model="nextRoundGroupSize" class="selection-slider" :min="2" :max="10" :step="1" thumb-label aria-label="1グループの表示枚数">
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
