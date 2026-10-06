import type { ComputedRef, Ref, ShallowRef } from 'vue'
import type { ExportReport, Photo, Project } from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { Session } from '~/lib/core'
import type { CoreInputs } from '~/utils/coreInputs'
import { exportTargetsForStars } from '~/utils/exportTargets'
import { resultsCsv } from '~/utils/amazonCsv'
import {
  SHARE_FILE_LIMIT, downloadBlob, shareFiles, zipEntriesByRating
} from '~/utils/shareExport'
import { createStoredZip } from '~/utils/zip'
import type { View } from './types'

/** 書き出し（フォルダ分け・メタデータ・CSV・共有・ZIP）が `useCurator` から受け取るもの。 */
export interface ExportDeps {
  desktop: PhotoBackend
  activeProject: Ref<Project | null>
  coreSession: ComputedRef<Session | null>
  coreInputs: ShallowRef<CoreInputs | null>
  view: Ref<View>
  error: Ref<string>
  isAmazon: ComputedRef<boolean>
  /** 結果の画面の絞り込み（「CSV を書き出す」の対象）。 */
  resultsRating: Ref<number | null>
  notify: (text: string) => void
  refreshProjects: () => Promise<void>
  loadSummary: () => Promise<void>
  loadResultsPage: (reset?: boolean) => Promise<void>
}

/**
 * 書き出し・共有・CSV・メタデータへの反映（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 * ほかの機能からは呼ばれない。状態はここで作り、`useCurator` が戻り値に並べ直す。
 */
export function useExport(deps: ExportDeps) {
  const {
    desktop, activeProject, coreSession, coreInputs, view, error, isAmazon, resultsRating,
    notify, refreshProjects, loadSummary, loadResultsPage
  } = deps

  // 書き出し
  const exportDialog = ref(false)
  // Amazon の写真は移動できない（原本は Amazon にある）。開くたびにコピーへ戻す。
  watch(exportDialog, open => { if (open && isAmazon.value) exportMode.value = 'copy' })
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

  // ---- 書き出し -------------------------------------------------------------

  async function chooseExportDestination() {
    try {
      const selected = await desktop.chooseFolder()
      if (selected) exportDestination.value = selected
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'フォルダを選択できませんでした。'
    }
  }

  /**
   * 書き出しの対象を決める（フォルダ分け・メタデータ・共有・ZIP・CSV の全部が使う）。
   *
   * 選んだ星の写真を、連写ごとに畳んだ行にして仲間まで広げ、**その 1 枚自身の星が選んだ星に合う
   * ものだけ**にする（`utils/exportTargets.ts`）。連写の中身を選別した組は、選んだものだけが出る。
   * 行は全部を読み直して使う（結果の格子はページ送りで、全部は持っていないので）。
   */
  async function exportPhotosFor(stars: readonly number[]): Promise<Photo[]> {
    const projectId = activeProject.value?.id
    if (!projectId || !stars.length) return []
    const rows = [...await desktop.getCoreInputs(projectId)]
      .sort((left, right) => (left.relativePath < right.relativePath ? -1 : left.relativePath > right.relativePath ? 1 : 0))
    const byPath = new Map(rows.map(row => [row.relativePath, row]))
    return exportTargetsForStars(rows, coreSession.value?.members, stars)
      .map(target => byPath.get(target.relativePath))
      .filter((photo): photo is Photo => !!photo)
  }

  /**
   * ダイアログに出す枚数のための、広げたあとの写真。ダイアログを開いたときと、星を選び直したときに
   * 読み直す（`exportPreviewFor`）。星ごとの枚数 `ratingCount` は広げる前の行の数。
   */
  const exportPreviewRows = shallowRef<Photo[]>([])
  const exportPreviewStars = ref<number[]>([])
  let exportPreviewToken = 0
  async function refreshExportPreview() {
    const token = ++exportPreviewToken
    const stars = [...exportPreviewStars.value]
    const rows = stars.length && activeProject.value ? await exportPhotosFor(stars).catch(() => []) : []
    if (token === exportPreviewToken) exportPreviewRows.value = rows
  }
  /** いま開いているダイアログが、どの星の一覧を見せるか。閉じたら空。 */
  const exportPreviewCount = computed(() => exportPreviewRows.value.length)
  watch(
    () => exportDialog.value ? [...exportRatings.value]
      : metadataDialog.value ? [...metadataRatings.value]
        : shareDialog.value ? [...shareRatings.value] : [],
    stars => { exportPreviewStars.value = stars; void refreshExportPreview() },
    { immediate: true }
  )

  /** 「移動」を押したときの確認。コピーは確認なしで実行する。 */
  const exportMoveConfirm = ref(false)
  function requestExport() {
    if (exportMode.value === 'move') exportMoveConfirm.value = true
    else void runExport()
  }

  async function runExport() {
    exportMoveConfirm.value = false
    if (!activeProject.value || !exportDestination.value) return
    exportBusy.value = true
    exportResult.value = null
    exportError.value = ''
    try {
      const photos = await exportPhotosFor(exportRatings.value)
      if (!photos.length) throw new Error('対象の写真がありません。')
      exportResult.value = await desktop.exportPhotos(
        activeProject.value.id, exportDestination.value,
        photos.map(photo => photo.id), exportMode.value === 'move'
      )
      if (exportMode.value === 'move') {
        // 移動した写真は、原本がそのフォルダに無いのでプロジェクトから外れる。数・一覧・対応表を読み直す。
        coreInputs.value = null
        await refreshProjects()
        await loadSummary()
        if (view.value === 'results') await loadResultsPage(true)
        const moved = exportResult.value?.processed ?? 0
        if (moved > 0) notify(`${moved.toLocaleString()} 枚を移動しました（このプロジェクトからは外れます）`)
      }
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
      const photos = await exportPhotosFor(metadataRatings.value)
      if (!photos.length) throw new Error('対象の写真がありません。')
      metadataResult.value = await desktop.writeRatingsToPhotos(
        activeProject.value.id, photos.map(photo => photo.id)
      )
    } catch (cause) {
      metadataError.value = cause instanceof Error ? cause.message : 'メタデータを書き込めませんでした。'
    } finally {
      metadataBusy.value = false
    }
  }

  /**
   * CSV を書き出す。全部の出所で同じ（先頭 3 列は `relative_path,rating,captured_at`、Amazon は 4 列目に `name`）。
   * PC は保存ダイアログ、ブラウザはダウンロード。**原本には触れない。**
   */
  async function saveResultsCsv(stars: readonly number[]): Promise<string> {
    const photos = await exportPhotosFor(stars)
    if (!photos.length) throw new Error('対象の写真がありません。')
    const stamp = new Date().toISOString().slice(0, 10)
    const text = resultsCsv(photos.map(photo => ({
      relativePath: photo.relativePath, rating: photo.rating, capturedAt: photo.capturedAt, name: photo.name
    })), isAmazon.value)
    const saved = await desktop.saveCsv(`photo-curator-${stamp}.csv`, text)
    return saved ? `${photos.length.toLocaleString()} 枚を CSV にしました。` : ''
  }

  /** 結果の画面の「CSV を書き出す」。いまの絞り込み（すべてなら全部の星）が対象。 */
  async function exportResultsCsv() {
    try {
      notify(await saveResultsCsv(resultsRating.value === null ? [5, 4, 3, 2, 1, 0] : [resultsRating.value]))
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'CSV を書き出せませんでした。'
    }
  }

  // ---- ライブラリへの反映（ブラウザ） --------------------------------------

  /**
   * 書き出しの対象を集める（共有・ZIP）。対象の決め方は `exportPhotosFor`（連写の仲間まで広げる）。
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
    for (const photo of await exportPhotosFor(shareRatings.value)) {
      const file = desktop.originalFile?.(photo.id) ?? null
      if (file) rows.push({ name: photo.name, rating: photo.rating, capturedAt: photo.capturedAt, file })
      else missing += 1
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

  /**
   * Amazon の結果の ZIP（原本を取る）。原本が 1 枚も取れないときは、その理由が `shareError` に出る
   * （CSV だけ書き出せます）。
   */
  async function exportAmazonZip() {
    const project = activeProject.value
    if (!project || !desktop.exportAmazon) return
    shareBusy.value = true
    shareError.value = ''
    shareMessage.value = ''
    try {
      const photos = await exportPhotosFor(shareRatings.value)
      const out = await desktop.exportAmazon(project.id, photos.map(photo => photo.id))
      downloadBlob(out.blob, out.fileName)
      shareMessage.value = `${out.count} 枚を ZIP にしました${out.skipped ? `（原本を取れなかった ${out.skipped} 枚は除いています）` : ''}。`
    } catch (cause) {
      shareError.value = cause instanceof Error ? cause.message : '書き出せませんでした。'
    } finally {
      shareBusy.value = false
    }
  }

  /** 書き出しダイアログの CSV。全部の出所で同じ。 */
  async function exportCsvByRating() {
    shareBusy.value = true
    shareError.value = ''
    shareMessage.value = ''
    try {
      shareMessage.value = await saveResultsCsv(shareRatings.value)
    } catch (cause) {
      shareError.value = cause instanceof Error ? cause.message : 'CSV を書き出せませんでした。'
    } finally {
      shareBusy.value = false
    }
  }

  /** 星ごとのフォルダに分けた ZIP を書き出す。 */
  async function exportZipByRating() {
    if (isAmazon.value) return exportAmazonZip()
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

  return {
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
    chooseExportDestination,
    runExport,
    runMetadataWrite,
    exportResultsCsv,
    exportPreviewCount,
    exportMoveConfirm,
    requestExport,
    shareSelectedPhotos,
    exportZipByRating,
    exportCsvByRating,
    openShareDialog
  }
}
