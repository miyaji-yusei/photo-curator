export type NoticeKind = 'success' | 'info' | 'warning'

export interface Notice {
  id: number
  text: string
  kind: NoticeKind
}

// 画面全体で 1 つだけ持つ（app.vue の v-snackbar が表示する）。
const notice = ref<Notice | null>(null)
let seq = 0

/** 成功・情報の通知を、画面の下に重ねて出す（4 秒で自動で消える）。本文は動かさない。 */
export function useNotice() {
  function notify(text: string, kind: NoticeKind = 'success') {
    if (!text) return
    notice.value = { id: ++seq, text, kind }
  }
  function dismiss() {
    notice.value = null
  }
  return { notice, notify, dismiss }
}
