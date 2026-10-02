import type { Ref } from 'vue'
import type { Photo, Project } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import { previewRefreshPlan, withNewThumbnails } from '~/utils/previewRefresh'

/**
 * プロジェクトの画面の格子（プレビュー）の読み込みと、その世代番号
 * （U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 */
export function usePreviewGrid(desktop: PhotoBackend, activeProject: Ref<Project | null>) {
  const previewPhotos = shallowRef<Photo[]>([])
  const previewTotal = ref(0)

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

  return {
    previewPhotos,
    previewTotal,
    loadPreview,
    refreshPreviewThumbnails,
    loadMorePreview
  }
}
