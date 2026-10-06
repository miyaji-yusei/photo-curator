import type { ComputedRef, Ref } from 'vue'
import type { AmazonPreview, Project } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { View } from './types'

/** 作成ダイアログ・アプリの設定が `useCurator` から受け取るもの。 */
export interface ProjectCreateDeps {
  desktop: PhotoBackend
  view: Ref<View>
  loading: Ref<boolean>
  error: Ref<string>
  notify: (text: string) => void
  canImportPhotos: ComputedRef<boolean>
  fileName: (path: string) => string
  refreshProjects: () => Promise<void>
  openProject: (project: Project) => Promise<void>
}

/**
 * プロジェクトの作成ダイアログ（フォルダ・Amazon のリンク・表示用画像の大きさ・同名の JPEG と RAW）と、
 * アプリの設定の画面（表示用画像の既定）（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 */
export function useProjectCreate(deps: ProjectCreateDeps) {
  const { desktop, view, loading, error, notify, canImportPhotos, fileName, refreshProjects, openProject } = deps

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

  async function chooseFolder() {
    try {
      const selected = await desktop.chooseFolder()
      if (selected) folderPath.value = selected
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'フォルダを選択できませんでした。'
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

  return {
    createDialog,
    createTab,
    amazonUrl,
    amazonPreview,
    amazonLoading,
    amazonError,
    projectName,
    folderPath,
    devFolderPath,
    isDev,
    chooseFolder,
    loadAmazonPreview,
    createPairRaw,
    createDisplayChoices,
    createDisplayEdge,
    appDisplayChoices,
    appDisplayEdge,
    saveAppDisplayEdge,
    createProject
  }
}
