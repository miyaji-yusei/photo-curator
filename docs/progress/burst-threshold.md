# バースト判定を「全件確認」から「閾値の学習」へ（2026-07-23 夕方）

**git 操作は一切していない。**変更はすべて未 stage のまま積み上がっている。

## 何を変えたか

以前は候補になった連写グループを全件確認させていた（実データで 52 回）。
これをやめ、**隣り合う 2 枚を最大 8 問だけ判定させて閾値を学習**し、残りは
自動でまとめる方式にした。

判定の単位をグループからペアに変えたことで、「1 枚目はまとめないが 2 と 3 枚目は
まとめたい」が**自動的に表現される**。`build_burst_groups` は元から隣接ペアの連鎖で
グループを作っているため、閾値を満たさないペアの位置でグループが切れる。
3 枚以上を並べて分割点を指定させる UI は不要だった。

## 変更したファイル

### 新規 `utils/burstThreshold.ts`

距離順に並べた候補ペアへの二分探索と、閾値の推定。純粋関数のみ。

| 関数 | 役割 |
|---|---|
| `sortPairsByDistance` | 距離昇順。同距離は id で安定させ出題順を再現可能に |
| `nextPair` | 探索区間の中央を出題。既出・スキップ済みは中央から外へずらす |
| `applyAnswer` | まとめる→下端を上げる / 別々→上端を下げる |
| `skipPair` | 学習に使わず二度と出題しない。区間は動かさない |
| `shouldStop` | 上限 8 問 / 区間が尽きた / 5 問以上かつ区間幅 ≤ 2 |
| `inferThreshold` | **回答の誤分類数が最小になる閾値**。矛盾した回答でも破綻しない |

`resolution = 2` は dHash のブレ（デコード方式で ±1〜5 ビット）に由来する。
これより細かく詰めても精度は上がらず、質問数だけが増える。

### `src-tauri/src/lib.rs`

- `pair_eligibility()` を切り出し。時間窓と根拠の強さによる一次審査を
  `candidates_within`（解析対象の絞り込み）と `build_burst_pairs`（出題元）で共有する。
  二重実装すると片方だけ直したときに学習と本番がズレる
- `get_burst_pairs` コマンド追加。**距離による足切りはしない**。どこで切るかを
  決めるのが学習なので、出題元を閾値で絞ってはいけない
- `build_burst_groups(entries)` → `build_burst_groups(entries, threshold)`
- `get_burst_groups` に `threshold: Option<u32>`。None なら保存値→既定値
- `projects` に `burst_threshold` / `burst_threshold_learned_at` を冪等 migration で追加
- `save_burst_threshold` / `clear_burst_threshold` コマンド追加

### `types/photo.ts` / `utils/tournament.ts`

- `BurstPair` / `PairAnswer` / `ThresholdState` を追加
- `SelectionSession` の `burstReviewIndex` を廃止し、`burstPairs` /
  `burstThresholdState` / `burstThreshold` を追加。`stage` に `burst-threshold` と
  `burst-preview`
- `normalizeSession()` 追加。閾値学習を入れる前に保存された JSON を読み込むとき、
  連写まわりだけ初期化して rating と survivors は残す

### `app.vue`

- 判定画面（2 枚を大きく並べる）と確認画面（スライダーで微調整）
- キーボード: `1` まとめる / `2` 別々 / `S` 判断できない
- 設定画面に「学習済み（基準 N）」と「学習し直す」
- 候補ペアが揃っていないときは待機表示。`FINAL.md` §4 の
  「初回は連写がまとまらない」問題の受け皿になる

### `assets/main.css`

- `.compare-pair` / `.compare-actions` を追加。**`object-fit: contain`**。
  構図の類似を判断させる画面で切り取るのは誤り
- レスポンシブを追加（従来 `@media` が 1 つも無かった）。900px / 600px / 縦 620px

## 検証

| コマンド | 結果 |
|---|---|
| `cargo test --lib --release` | **31 passed**（新規 4） |
| `pnpm test` | **11 passed**（新規 10） |
| `pnpm typecheck` | エラーなし |
| `pnpm build` | 成功 |
| `cargo fmt --check` | OK |

### テストが本当にバグを検出することの確認

`applyAnswer` の区間を狭める処理を無効化したところ、収束テストだけが期待どおり
失敗した（`expected 5 to be less than or equal to 2`）。他のテストは二分探索に
依存しないため green のまま。確認後に復元し全件パスを再確認した。

### 実測（ブラウザの計算済みスタイル）

| 幅 | compare-pair | 画像 max-height | ドロワー | 横スクロール |
|---|---|---|---|---|
| 1425 | 2 列（660.5px ×2） | 558px | 常設・main が 280px オフセット | なし |
| 600 | 1 列 | 272px | 一時表示・ハンバーガー | なし |
| 375 | 1 列 | 243.6px | 一時表示・ボタン縦積み | なし |

## 途中で見つけて直した実バグ

1. **`useDisplay()` が機能していなかった。** 1265px でも `mdAndUp` が false を返し、
   ドロワーが常時 temporary になっていた。この構成（vite-plugin-vuetify +
   `build.transpile: ['vuetify']`）では plugin 側と別インスタンスを掴むことがある。
   `matchMedia` に置き換えて解消
2. **`permanent` でもドロワーが画面外に隠れた。** `v-model` の初期値 false に
   引きずられ `translate(-280px)` になっていた。開閉状態を幅に連動させて解消
3. **モバイルでボタン順が反転していた。** `column-reverse` により「判断できない」が
   最上部に来ていた。DOM 順を組み替えて `column` に変更し、主操作を最下部へ

## 環境メモ

`vitest.config.ts` に `~` / `@` エイリアスを追加した。これが無いと
`~/utils/...` を import したテストが収集時に落ちる。
`node:url` は `@types/node` を要求するため使わず、`import.meta.url` から
Windows のドライブレターを整形して解決している。

## 未確認 / 残課題

- **判定画面と確認画面の実機での目視確認ができていない。**Tauri 無しでは到達
  できないため、CSS の挙動は DOM を注入して実測した。**要実機確認**
- **ライブリサイズの追従を検証できていない。**この検証環境では
  `matchMedia().matches` は正しく更新されるのに `change` も `resize` も
  発火しない（発火回数 0 を確認）。実ブラウザ / Tauri ウィンドウでは発火するが、
  ここでは各幅で読み込み直して確認した
- accepted バーストをトーナメントで 1 カードに畳み込む処理は未実装のまま。
  閾値が決まった今が着手の順番
- `HASH_DISTANCE_LIMIT = 14` は既定値として残っている。学習済みプロジェクトでは
  使われない
- Step 8（モジュール分割）は未着手。`lib.rs` は約 4,100 行
