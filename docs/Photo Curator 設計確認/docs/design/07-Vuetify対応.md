# 07. Vuetify 対応と実装順

Vue 3 + Vuetify 3（Nuxt 3 SPA）。テーマは現状のダーク 1 つを変更しない。`app.vue` の分割は実装側の判断でよい。

## 判定の 2 軸

```ts
const { width, xs, smAndDown, lgAndUp } = useDisplay()
const wide   = computed(() => width.value >= 1100)   // アプリバー＋ドロワー構成
const narrow = computed(() => width.value < 600)     // ラベルをアイコンのみに
const landscape = computed(() => gridArea.width > gridArea.height) // 枠の行×列の規則
```

`capabilities`: `keyboard`（キー案内・ホバー UI）／`largeGroups`（2–10 か 2–4）／`browseFolders`（OS ダイアログ）／`fullResolution`（拡大で原本）／`exportFolders`・`writeMetadata`（PC の取り出し）／新規に `smb`（設定の NAS カード・作成シートの NAS タブ）と `share`・`mediaStoreWrite`（Android の取り出し）を追加する。

環境名（Android/PC/Web）で分岐しない。**幅と capabilities だけ。**

## 部品の対応

| 設計 | Vuetify |
|---|---|
| 各画面のバー（56/64px） | `v-toolbar` `density="comfortable"`（`v-app-bar` は `wide` のホーム・詳細・結果のみ） |
| 進捗線 2px | `v-progress-linear height="2" color="primary"` |
| 主ボタン pill | `v-btn rounded="pill" color="primary" size="large"` |
| チップ（アウトライン） | `v-btn variant="outlined" rounded="pill" size="default"` |
| フィルタチップ | `v-chip-group` ＋ `v-chip filter` |
| プロジェクトカード | `v-card` ＋ `v-progress-linear height="3"` |
| 星の内訳バー | 自作（`display:flex` の div 列） |
| 下から出るシート | `v-bottom-sheet`（`wide` では `v-dialog max-width`） |
| 作成シート | `v-dialog`（720×600、<1100 は `fullscreen` 相当） |
| メニュー（PC の取り出す） | `v-menu` ＋ `v-list two-line` |
| 確認ダイアログ | `v-dialog` ＋ `v-card` |
| スナックバー | `v-snackbar timeout="4000"` |
| 選別の枠 | CSS grid（`grid-template-columns: repeat(cols, 1fr)`、`gap: 6px`、`padding: 6px`）。`v-row/v-col` は使わない（gap 制御のため） |
| 写真 | `<img style="object-fit:contain; max-width:100%; max-height:100%">` を枠中央に。`v-img` は `contain` でもよいが `aspect-ratio` を渡さない |
| タイル上の半透明ボタン | `v-btn icon variant="flat"` ＋ `background: rgba(16,17,20,.55)` |
| 長押し | `@pointerdown` ＋ 300ms タイマー（`v-touch` は使わない） |
| まとまり編集の線 | 自作。`@pointermove` でドラッグ、ハンドル 44px |
| 拡大表示 | `v-dialog fullscreen transition="fade-transition"`、ピンチは `touch-action: none` ＋ 自作 |
| スライダー（緩く／厳しく） | `v-slider` `thumb-size="20"` |
| トグル | `v-switch inset color="primary"` |
| セーフエリア | ルート `v-app` に `padding-top: env(safe-area-inset-top)` |

## 枠の行×列（実装）

```ts
const GRID = { 2:[1,2], 3:[1,3], 4:[2,2], 5:[2,3], 6:[2,3], 7:[2,4], 8:[2,4], 9:[3,3], 10:[2,5] } // [rows, cols] 横長
function gridFor(n: number, landscape: boolean) {
  const [r, c] = GRID[n]
  return landscape ? { rows: r, cols: c } : { rows: c, cols: r }
}
```

## まとまり色の巡回（実装）

```ts
const BURST_COLORS = ['#d6ff73', '#a7c8ff', '#ffb3e6']
// 編集シート内の帯で、まとまり（2 枚以上）に出会うごとに index++。単独写真では進めない。
```

## 実装順（1 ステップごとに動く状態で止める）

1. **状態モデル**：`source` / `prep` / `selection` を 01 の形に。保存タイミングを揃える。`prep` をプロジェクト ID で作り直す（D-2）。名前の既定と `source.label`（ID 混入の修正）
2. **セーフエリアと幅判定**：`wide` / `narrow` / `landscape`。Android のアプリバー廃止、各画面に `v-toolbar`
3. **選別画面**（03）：バー 1 本、枠の規則、contain、タイル記号、主ボタン文言、「どれも選ばない」廃止、長押し・右上ボタン、キー案内の capabilities 分岐。表示枚数の即適用は既存を維持
4. **ホーム・詳細**（04）：カード、準備カード、写真一覧の 5 状態、主ボタン 1 つ、`…`
5. **設定・作成シート**（04）：`settings-app`、NAS の複数保存、出所タブ、`method` 画面と旧設定画面の削除、`startSheet`
6. **選別の前後**（04）：基準学習の骨格、まとまり確認の要約、ラウンド完了シート、拡大表示の差し替え読み込みと「残す」
7. **まとまり編集シート**（03）：区切り線・代表・3 色巡回。未判定のみ対象
8. **結果**（04）：星チップ、複数選択モード、「[対象] を…」メニュー、確認ダイアログ（赤字）、同期状態の席
9. **エラーと空**（05）：2 段構え、スナックバー、空の状態文、上部の赤帯廃止
10. **PC / iPad の仕上げ**：`wide` 構成の確認、ホバー UI、ドロワーに設定を追加、選別中のアプリバー非表示

各ステップの完了条件：Fold 開（933×704）・カバー（476×752）・PC（1440×900）の 3 サイズで崩れないこと。
