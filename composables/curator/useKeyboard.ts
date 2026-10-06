import type { ComputedRef, Ref } from 'vue'
import type { Photo } from '~/types/photo'
import type { SavedSelection } from '~/utils/selectionFlow'
import type { View } from './types'

/** 選別画面のキー操作が `useCurator` から受け取るもの（状態と、キーで呼ぶ操作）。 */
export interface KeyboardDeps {
  view: Ref<View>
  session: Ref<SavedSelection | null>
  helpDialog: Ref<boolean>
  zoomPhoto: Ref<Photo | null>
  tournamentPhotos: Ref<Photo[]>
  burstReviewPhotos: Ref<Photo[]>
  burstReviewKept: Ref<string[]>
  currentPair: ComputedRef<{ gapMs: number } | null>
  isSlideshow: ComputedRef<boolean>
  onZoomKeydown: (event: KeyboardEvent) => void
  openZoom: (photo: Photo | null, list?: Photo[]) => void
  openBurst: (photo: Photo | null) => Promise<void>
  applyBurstReview: () => Promise<void>
  toggleBurstReviewKeep: (photoId: string) => void
  skipCurrentPair: () => Promise<void>
  answerPair: (grouped: boolean) => Promise<void>
  undoChoice: () => Promise<void>
  confirmChoices: () => Promise<void>
  confirmPhoto: (photoId: string) => Promise<void>
  toggleChoice: (photoId: string) => Promise<void>
  saveSession: () => void
}

/**
 * 選別画面のキー操作（U53 で `useCurator.ts` から切り出した。中身は変えていない）。
 * 判断はしない。キーを、`useCurator` の操作（選ぶ・確定・1 つ戻す・拡大など）に振り分けるだけ。
 * `window` への登録・解除は `useCurator` の `mount`／`unmount`（同じ関数を渡す）。
 */
export function useKeyboard(deps: KeyboardDeps) {
  const {
    view, session, helpDialog, zoomPhoto, tournamentPhotos, burstReviewPhotos, burstReviewKept, currentPair,
    isSlideshow, onZoomKeydown, openZoom, openBurst, applyBurstReview, toggleBurstReviewKeep, skipCurrentPair,
    answerPair, undoChoice, confirmChoices, confirmPhoto, toggleChoice, saveSession
  } = deps

  function onKeydown(event: KeyboardEvent) {
    // 拡大中は拡大のキーだけ。選択には流さない。
    if (zoomPhoto.value) {
      onZoomKeydown(event)
      return
    }
    if (event.target instanceof HTMLInputElement) return
    // ヘルプを開いている間は、選別のキー（数字・Enter・Backspace など）を後ろの画面へ流さない。
    if (helpDialog.value) return

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
    // スライドショーは 1 枚ずつ。Enter・M・Space は使わず（Enter で「落とす」が走らないように）、
    // 1・3・5 と ←・→・↑ は画面（SlideshowView）が受ける（2・4・↓ は何もしない）。修飾キー付き（Ctrl＝拡大など）は下の共通処理へ。
    if (isSlideshow.value) {
      if (event.key === 'Enter' || event.key === ' ' || event.key.toLowerCase() === 'm') {
        event.preventDefault()
        return
      }
      if (!event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey && /^(Digit|Numpad)\d$/.test(event.code)) return
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

  return { onKeydown }
}
