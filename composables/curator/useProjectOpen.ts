import type { ComputedRef, Ref, ShallowRef } from 'vue'
import type { AnalysisFailure, Photo, Project, ProjectProgress } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { PairOverride } from '~/lib/core'
import type { CoreInputs } from '~/utils/coreInputs'
import { prepareProgress, projectStatus } from '~/utils/projectStatus'
import type { CardState, CardStatus, PrepareLine } from '~/utils/projectStatus'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { View } from './types'

/** プロジェクトを開く・カード・準備の数が `useCurator` から受け取るもの。 */
export interface ProjectOpenDeps {
  desktop: PhotoBackend
  projects: Ref<Project[]>
  projectCards: Ref<Record<string, { status: CardStatus, thumbnailUrl: string | null }>>
  activeProject: Ref<Project | null>
  session: Ref<SavedSelection | null>
  view: Ref<View>
  loading: Ref<boolean>
  error: Ref<string>
  coreInputs: ShallowRef<CoreInputs | null>
  previewPhotos: Ref<Photo[]>
  previewTotal: Ref<number>
  tournamentPhotos: Ref<Photo[]>
  taskProgress: Ref<ProjectProgress | null>
  taskWarning: Ref<string | null>
  scanRunning: ComputedRef<boolean>
  analysisBacklog: Ref<number>
  displayBacklog: Ref<number>
  analysisFailures: Ref<number>
  analysisFailureList: Ref<AnalysisFailure[]>
  analysisFailuresDialog: Ref<boolean>
  analysisProgress: Ref<ProjectProgress | null>
  deleteTarget: Ref<Project | null>
  deleteDialog: Ref<boolean>
  deleteBusy: Ref<boolean>
  saveQueue: { flush: () => Promise<void>, drop: (projectId: string) => Promise<void> | void }
  sidecar: { refreshAccess: (projectId: string) => Promise<unknown> }
  // 再代入される `let` や、あとで作られるものは関数で渡す（呼ばれるのは作ったあと）。
  /** 開く処理の世代を 1 つ進めて返す（`openToken` は useCurator が持つ）。 */
  nextOpenToken: () => number
  currentOpenToken: () => number
  setSidecarCheckPending: (projectId: string | null) => void
  setDeletingProjectId: (projectId: string | null) => void
  setPairOverrides: (overrides: PairOverride[]) => void
  clearPrefetched: () => void
  importsByPicker: () => boolean
  loadPreview: (projectId: string) => Promise<void>
  loadSummary: () => Promise<void>
  refreshDisplayState: () => Promise<void>
  refreshAnalysisFailures: (projectId?: string) => Promise<void>
  runSidecarCheck: (project: Project) => Promise<unknown>
  healRowRatings: (projectId: string) => Promise<void>
  startScan: () => Promise<void>
  enterMethod: () => void
  resumeSession: () => Promise<void>
  openResults: () => Promise<void>
}

/** プロジェクトを開く・ホームのカード・準備の数・削除（R6。`useCurator.ts` から切り出した。中身は変えていない）。 */
export function useProjectOpen(deps: ProjectOpenDeps) {
  const {
    desktop, projects, projectCards, activeProject, session, view, loading, error, coreInputs, previewPhotos,
    previewTotal, tournamentPhotos, taskProgress, taskWarning, scanRunning, analysisBacklog, displayBacklog,
    analysisFailures, analysisFailureList, analysisFailuresDialog, analysisProgress, deleteTarget, deleteDialog,
    deleteBusy, saveQueue, sidecar, nextOpenToken, currentOpenToken, setSidecarCheckPending, setDeletingProjectId,
    setPairOverrides, clearPrefetched, importsByPicker, loadPreview, loadSummary, refreshDisplayState,
    refreshAnalysisFailures, runSidecarCheck, healRowRatings, startScan, enterMethod, resumeSession, openResults
  } = deps

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
        const [prepare, saved, summary, first] = await Promise.all([
          desktop.getPrepareState(project.id),
          desktop.loadSession(project.id),
          desktop.getSelectionSummary(project.id),
          desktop.getProjectPhotoPage(project.id, 0, 1).catch(() => null)
        ])
        const status = projectStatus({
          project, analysisBacklog: prepare.analysisBacklog, displayBacklog: prepare.displayBacklog,
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
    const state = await desktop.getPrepareState(projectId).catch(() => null)
    if (activeProject.value?.id !== projectId) return
    analysisBacklog.value = state?.analysisBacklog ?? 0
    displayBacklog.value = state?.displayBacklog ?? 0
  }

  /** 開く処理の世代。新しい呼び出しが来たら古い呼び出しは、以降の結果を捨てて終わる（W6）。 */
  let openToken = 0
  async function openProject(project: Project) {
    const token = nextOpenToken()
    const stale = () => token !== currentOpenToken()
    activeProject.value = project
    previewPhotos.value = []
    previewTotal.value = 0
    analysisBacklog.value = 0
    // 前のプロジェクトの警告・進みを持ち越さない（B6）。
    analysisFailures.value = 0
    analysisFailureList.value = []
    analysisFailuresDialog.value = false
    analysisProgress.value = null
    clearPrefetched()
    coreInputs.value = null
    setPairOverrides([])
    view.value = 'project'
    loading.value = true
    setSidecarCheckPending(null)
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
        setSidecarCheckPending(project.id)
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
      // 「0 枚なら走査」は状態で判定する。全部が非対応の形式だと枚数は 0 になるが、走査は済んでいる（U58）。
      const neverScanned = !project.photoCount && project.status !== 'ready'
      if (neverScanned && !importsByPicker() && !scanRunning.value && project.folderAccess !== 'needs-permission' && !linkGone) {
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
      void refreshAnalysisFailures(project.id)
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
    setDeletingProjectId(target.id)
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
      setDeletingProjectId(null)
      deleteBusy.value = false
    }
  }

  return {
    refreshProjects, refreshProjectCards, prepareLines, refreshPrepareCounts, openProject, openProjectAction,
    askDeleteProject, confirmDeleteProject
  }
}
