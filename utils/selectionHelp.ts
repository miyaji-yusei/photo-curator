// 選別中の「？」ヘルプの文章（U24）。PC・Web の操作の一覧は**ここ 1 か所**に書く。
// 実際の割り当て（`composables/useCurator.ts` の onKeydown、`utils/slideshowGesture.ts`、
// `components/views/SlideshowView.vue`・`TournamentView.vue`）と食い違わないように直すこと。

export type HelpMethod = 'tournament' | 'slideshow'

export interface HelpSection {
  title: string
  items: string[]
}

export interface HelpGuide {
  title: string
  sections: HelpSection[]
}

export const SELECTION_HELP: Record<HelpMethod, HelpGuide> = {
  slideshow: {
    title: 'スライドショーの操作',
    sections: [
      {
        title: 'クリック',
        items: [
          '写真の左半分をクリック: 落とす',
          '写真の右半分をクリック: 残す',
          '写真の上の帯（上から 4 分の 1）をクリック: ★5 で確定',
          '写真の中心のクリック: 何もしない',
          '写真の中心をダブルクリック: 拡大',
          '右上の ★ ボタン: ★5 で確定、虫眼鏡のボタン: 拡大'
        ]
      },
      {
        title: 'ドラッグ',
        items: [
          '左へドラッグ: 落とす',
          '右へドラッグ: 残す',
          '上へドラッグ: ★5 で確定',
          '長押しの拡大はありません'
        ]
      },
      {
        title: 'キー',
        items: [
          '1 または ←: 落とす',
          '3 または →: 残す',
          '5 または ↑: ★5 で確定',
          'テンキーの 1・3・5 も同じ（NumLock の状態によらず効きます）',
          'Backspace: 1 つ戻す',
          'Esc: 拡大や設定などの画面を閉じる'
        ]
      }
    ]
  },
  tournament: {
    title: 'トーナメントの操作',
    sections: [
      {
        title: 'クリック',
        items: [
          '写真をクリック: その写真を選んで次へ（複数選択中は、選ぶ印の切り替え）',
          '右上の ★ ボタン: その 1 枚を ★5 で確定',
          '右上の虫眼鏡のボタン: 拡大',
          '左下の「連写 n 枚」: まとめられた写真を開く',
          '上のバーの「選択なしで次へ」「n 枚を選択」: 今の選択で確定'
        ]
      },
      {
        title: 'キー',
        items: [
          '1〜9・0（1〜10 枚目）: その写真を選ぶ（複数選択中は印の切り替え）',
          'Ctrl + 数字: その写真を拡大',
          'Shift + 数字: その写真を ★5 で確定',
          'Alt + 数字: その写真のまとめ（連写）を開く',
          'Enter: 今の選択で確定（何も選んでいなければ、選択なしで次へ）',
          'M または Space: 複数選択の切り替え',
          'Backspace: 1 つ戻す',
          'テンキーの数字も同じ',
          '拡大中は ← →: 前後の写真、Esc・Enter・Space: 閉じる'
        ]
      }
    ]
  }
}

/** 今の方式と、もう一方の方式。 */
export function helpGuides(current: HelpMethod): { current: HelpGuide, other: HelpGuide } {
  const otherMethod: HelpMethod = current === 'slideshow' ? 'tournament' : 'slideshow'
  return { current: SELECTION_HELP[current], other: SELECTION_HELP[otherMethod] }
}
