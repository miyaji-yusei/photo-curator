import type { Photo } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'

/**
 * 拡大表示（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 * 拡大中のキー（`onZoomKeydown`）もここ。選別画面のキー（`useCurator` の `onKeydown`）が、拡大中はこちらへ渡す。
 */
export function useZoom(desktop: PhotoBackend) {
  const zoomPhoto = ref<Photo | null>(null)
  /** 拡大中に ← → で辿れる一覧。開いた場所に並んでいた写真をそのまま入れる。 */
  const zoomList = ref<Photo[]>([])

  /**
   * 拡大表示を開く。`list` にその写真が並んでいた一覧を渡すと、
   * 拡大したまま ← → で前後の写真へ移れる。
   */
  function openZoom(photo: Photo | null, list: Photo[] = []) {
    if (!photo) return
    zoomPhoto.value = photo
    zoomList.value = list.length ? [...list] : [photo]
  }

  /**
   * 拡大に出す画像。**原本**（Amazon は取ってきて端末に置いたもの）。取れるまでの間だけ表示用を見せる。
   * 写真が変わったら、遅れて届いた前の写真の結果は捨てる。
   */
  const zoomSrc = ref('')
  const zoomError = ref('')
  const zoomLoading = ref(false)
  let zoomToken = 0
  watch(zoomPhoto, async (photo) => {
    const token = ++zoomToken
    zoomError.value = ''
    zoomLoading.value = false
    zoomSrc.value = ''
    if (!photo) return
    const pending = desktop.photoOriginalUrl(photo)
    let arrived = false
    // すぐ着く（フォルダの写真）ときは途中の絵を挟まない。時間がかかるときだけ表示用を先に見せる。
    const timer = setTimeout(() => {
      if (arrived || token !== zoomToken) return
      zoomLoading.value = true
      zoomSrc.value = desktop.photoDisplayUrl(photo)
    }, 80)
    try {
      const url = await pending
      arrived = true
      if (token === zoomToken) zoomSrc.value = url
    } catch (cause) {
      arrived = true
      if (token === zoomToken) zoomError.value = cause instanceof Error ? cause.message : '原本を読み込めませんでした。'
    } finally {
      clearTimeout(timer)
      if (token === zoomToken) zoomLoading.value = false
    }
  })

  const zoomIndex = computed(() =>
    zoomPhoto.value ? zoomList.value.findIndex(item => item.id === zoomPhoto.value!.id) : -1
  )

  /** 拡大中に前後へ移る。行き先が無ければ**動かないだけ**で、拡大は閉じない。 */
  function stepZoom(step: number) {
    const next = zoomList.value[zoomIndex.value + step]
    if (next) zoomPhoto.value = next
  }

  /**
   * 拡大表示のキー。← → は前後送り、Esc・Enter・Space は閉じる。
   * Ctrl・Shift・Alt・Meta の単独の押下や、ほかのキーでは閉じない（Ctrl+ホイールの前に Ctrl を押すだけで閉じない）。
   * どのキーでも、選別画面の操作には流さない。
   */
  function onZoomKeydown(event: KeyboardEvent) {
    if (!zoomPhoto.value) return
    event.stopPropagation()
    if (event.key === 'ArrowLeft') { event.preventDefault(); stepZoom(-1); return }
    if (event.key === 'ArrowRight') { event.preventDefault(); stepZoom(1); return }
    if (event.key === 'Escape' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      zoomPhoto.value = null
    }
  }

  return {
    zoomPhoto,
    zoomList,
    openZoom,
    zoomSrc,
    zoomError,
    zoomLoading,
    zoomIndex,
    stepZoom,
    onZoomKeydown
  }
}
