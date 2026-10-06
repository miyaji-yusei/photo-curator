# 次の作業（2026-09-30、ユーザーの確認を受けて）

`feat/main-base` に T0〜T16 と修正（#20）をマージした（`55de41d`）。ここからは、ユーザーが実機で確かめて出した要望を片付ける。
**ローカルで続けるときは、このファイルを上から順に進める。**

## ブランチの運用（2026-09-30 変更: 積み上げ式）
- **まとめのブランチは `feat/mb-next`**。`feat/main-base` に U の直しを積み上げていく。まとめの PR（`feat/mb-next` → `feat/main-base`）でユーザーが全部を見て、マージする
- 1 件ごとに、`origin/feat/mb-next` の先端から `fix/mb-uNN-<名前>` を切り、`feat/mb-next` 向けの PR を出す
- 自分で差分とテストを確かめたら、`feat/mb-next` にマージコミット（`git merge --no-ff`）で取り込み、push する（**`feat/mb-next` だけは取り込んでよい。`feat/main-base`・main へは取り込まない**）
- 取り込んだら、この表の「状態」を「済」にし、まとめの PR の概要の一覧を更新する
- どこで止まっても、`feat/mb-next` の先端と、この表の「状態」を見れば続きが分かるようにする

## 共通の決まり
- main・feat/rebuild は書き換えない
- push・PR まではしてよい。merge・force・rebase・ブランチ削除はしない
- 判断（星・連写）は core。人が星を直接決める手直しは `utils/ratingEdit.ts` だけ（`docs/rebuild/spec-changes.md` の 04章）
- コミットの前に通すもの: `cargo test`（core）・`cargo check`（src-tauri）・`pnpm typecheck`・`pnpm test`
- ポート 3000 はユーザーの `pnpm tauri:dev`。確認は 3100 で行い、終わったら止める
- Android 版（`app-android/`）は見本として読むだけ。**U1 以外では変えない**
- **U7 は、実装の前にプランをユーザーと合わせる（ここで止まる）**

## 一覧（おすすめの順）

| # | 要望 | 規模 | 主な場所 | 状態 |
| --- | --- | --- | --- | --- |
| U1 | 「1 つ戻す」で、まとめの中の ★5 の仲間が ★4 になる（レビュー #2）→ **core の「1 つ戻す」を直す**（ユーザー決定） | M | `core/src/lib.rs`・`app-android/`（`.so` の作り直し） | 済（#28。Android の .so の作り直しはユーザー） |
| U2 | メタデータに反映が失敗する | S | `src-tauri/src/lib.rs` の `write_ratings_to_photos_blocking` | 済（#22） |
| U3 | 写真を移動すると、結果に数は出るが、当てはまる写真が無くなる | S〜M | `src-tauri/src/lib.rs`（移動と集計）・`composables/useCurator.ts` | 済（#29） |
| U4 | 「8 枚を ★5 から ★0 へ移しました」などの通知が × を押すまで消えない。通知の分だけ画面が下がる | S | `app.vue` 67〜79 行（`error`・`taskWarning`・`moveReport`）、`ResultsView.vue` の `resultsMessage` | 済（#23） |
| U5 | プロジェクトの詳細に「選別を最初からやり直す」の入口が無い（Android にはある） | S | `ProjectView.vue`・`RestartDialog.vue`・`restartFromScratch` | 済（#24） |
| U6 | 選別画面の下のボタン列を上に移し、写真を大きく見せる | M | `TournamentView.vue`・`composables/useCurator.ts` | 済（U6 の PR） |
| U7 | 選別の並べ方（4 枚で 2×2 か 1×4 か）を窓の形に合わせて変える（2026-09-30 プランをユーザーと合わせた。下の「決定」） | M | `TournamentView.vue`・新規 `utils/gridFor.ts` | 済（#35。ユーザーの目視待ち） |
| U8 | 「表示枚数」の画面で、クリックと矢印キーで増減できるようにする | S | `components/dialogs/GroupSizeDialog.vue` | 済（#25） |
| U9 | 拡大で Ctrl+スクロールで拡大・縮小。Ctrl を押しても拡大を閉じない | M | `useCurator.ts` の `onZoomKeydown`・拡大の部品 | 済（#36。ユーザーの目視待ち） |
| U10 | 拡大の読み込み中にぐるぐる（Android と同じ） | S | 拡大の部品（`zoomLoading` はもうある） | 済（#26） |
| U11 | 複数モードで ★5 を押しても、すぐ次の組へ進まない（Android と同じ） | S〜M | `useCurator.ts` の ★5（`keepAndTop`）の呼び方 | 閉じた（変更なし。2026-09-30 ユーザー決定: 今のまま＝Android と同じ） |
| U12 | まとまり編集を、Android と同じ横並び＋境目のバーで切る／ずらす UI にする | M | `components/dialogs/BurstDialog.vue`・`utils/burstEdit.ts` | 済（#37。PC の実機は未確認。ユーザーの目視待ち） |
| U13 | 表示用画像の大きさを、PC・Web でもプロジェクトの作成時から決められるようにする | S〜M | 作成ダイアログ・アプリの設定・`save_display_edge` | 済（#30。Web の Amazon の表示用は 1024 固定のまま） |
| U14 | PC の Amazon の読み込みが遅い（Web は速い）→ 速くできるか調べて比べる | 調査 → M | `src-tauri/src/amazon.rs`・`lib.rs` の Amazon の準備 | 調査済み（10章 §7 の U14。おすすめ (a)＝`agent` の使い回し＋`preview()` の二重取得をやめる、並列 4 のまま）→ (a) は済（#38。サムネ 16.1→6.9 秒・表示用 21.5→6.6 秒）。並列 8（(b')）も済（#40。ユーザー決定で 08章の約束を 8 に変更。Android の PARALLEL と 08章の書き換えは残り） |
| U15 | プロジェクト詳細の上のボタンの高さをそろえ、並びを「⋮・選別結果を見る・削除・写真を再読み込み・選別を再開」にする（2026-09-30 #27 の確認で） | S | `components/views/ProjectView.vue` 49 行付近 | 済（#31） |
| U16 | 拡大のぐるぐるを、右上でなく写真の真ん中に重ねる（Android と同じ）。U10 の見直し | S | `app.vue`（`zoom-overlay__photo`）・`assets/main.css` 254〜255 行 | 済（#33） |
| U17 | アプリの設定の画面を足し、表示用画像の既定の大きさを変えられるようにする（Android の設定と同じ） | S〜M | 新規 `components/views/AppSettingsView.vue`・`AppNav.vue`・`useCurator.ts`（`view`）・`displayEdge` まわり | 済（#34。PC の実機は未確認） |
| U18 | 選別の並べ方で、3 枚のときだけ 1×3 に固定する（4〜10 枚は U7 のまま）。縦長の 3 枚が 2×2 になっていた（2026-09-30 #32 の目視で。縦横比を測る案は見送り） | S | `utils/gridFor.ts`・`tests/gridFor.test.ts` | 済（#39。3 枚だけ 1×3 固定） |
| U19 | スライドショー選別（1 枚ずつ、左＝落とす・右＝残す）。トーナメントと切り替え可 | L | `TournamentView.vue`・`useCurator.ts`・core（グループ 1 枚）・Android | 済（#41。PC・Web・Android とも実装済み（`feat/mb-slideshow`。ユーザーの目視待ち。Android はエミュレーターで確認、実機は未確認）。core は下限 1 に変更（Android の .so は作り直し済み。`scripts/build-core.mjs`）） |
| U20 | 表示用画像の px の不具合（1536 で作られない・作成ダイアログの値がアプリの既定に負ける）と、詳細の ⋮ から px を変える機能 | M | `useCurator.ts`・`ProjectView.vue`・新規 `DisplayEdgeDialog.vue`・`utils/displayEdge.ts`・`lib.rs`（`get_display_settings`）・`backends/local.ts` | 済（#42。PC 実機は確認待ち） |
| U21 | スライドショー選別の操作の直し（上の帯のクリック・タップで ★5。方式の並び） | M | `utils/slideshowGesture`・`SlideshowView.vue`・`app-android/`（`Slideshow.kt`） | 済（#44。PC・Web 側は #41 の先に取り込み済み） |
| U22 | Android の Amazon の並列を 4 → 8（PC の U14 (b') にそろえる。ユーザー決定） | S | `app-android/`（`Amazon.PARALLEL`） | 済（#43） |
| U23 | スライドショーの中心のクリック・タップは何もせず、二度押し（タップ）で拡大 | S〜M | `SlideshowView.vue`・`app-android/`（`SlideshowGesture`） | 済（#44） |
| U24 | 選別画面のヘルプ「？」ボタンとテンキー | S〜M | `SelectionHelpButton.vue`・`app-android/`（`CullBar`・`SelectionHelpDialog`） | 済（#44） |
| U25 | 折りたたみ端末を閉じた幅（約 411dp）での設定・作成・上バーの表示崩れ（Android） | S | `app-android/` | 済（#44） |
| U26 | Android の選別中の画像の出方のもたつき（先読み・サムネイルの先出し） | M | `app-android/` | 済（#45 に含む） |
| U27 | PC Rust のレビュー R1〜R14 のうち、挙動を変えずにできる改善 | M | `src-tauri/` | 済（#45 に含む。残りは U37） |
| U28 | Web・PC 共通の画面コードのレビュー W1〜W24 の速さ・不具合の直し | M〜L | `composables/`・`components/` | 済（#45 に含む） |
| U29 | Android: スライドショーの中央タップが振り分けになる実機報告の調査（件 1）と、準備を撮影時刻の昇順にする（件 2） | S〜M | `app-android/` | 済（#45 に含む） |
| U30 | Android のレビュー A1〜A36 の速さと不具合の直し | L | `app-android/` | 済（#45 に含む） |
| U31 | （10章 §7・PR #45 の概要・コミット履歴に見当たらない） | — | — | 要確認 |
| U32 | スライドショー選別のタップの領域の変更（上 30% ＝★5・左 30% ＝落とす・右 30% ＝残す・中央の縦帯は無反応。ユーザー決定） | S〜M | `utils/slideshowGesture`・`app-android/`（`SlideshowGesture`） | 済（#45 に含む） |
| U33 | サイドカー同期の core（正規化・比較キー・未着手・`sidecar_plan`・混ぜ方・鍵の変換・catalog.json v2）。下の「U33」の節 | L | `core/src/sidecar_sync.rs`・UDL・`core-wasm/` | 済（#45 に含む） |
| U34 | PC・Web のサイドカー同期を `sidecar_plan` と楽観ロックに切り替える。下の「U34」の節 | L | `composables/useSidecarSync.ts`・`src-tauri/src/sidecar.rs` | 済（#45 に含む） |
| U35 | Android のサイドカー同期を `sidecarPlan` に切り替える。下の「U35」の節 | L | `app-android/`（`Sidecar.kt`） | 済（#45 に含む。実機は未確認） |
| U36 | Web の準備（解析）の順序と軽い改善（W10〜W18 など） | M | `composables/`・`utils/analysisOrder.ts` | 済（#45 に含む） |
| U37 | PC Rust の走査（取り消し・失敗・NAS 不通で欠損の印を確定しない。R4・R5・R11〜R14・R3） | M | `src-tauri/` | 済（#45 に含む） |
| U38 | core のレビュー R15〜R17（次のラウンドで仲間の星が追従する・`advance` の検証・手直しの重複は先勝ち） | S〜M | `core/src/lib.rs` | 済（#46） |
| U39 | 「指紋」を「ハッシュ値」に言い換える（docs・文言。識別子は変えない） | S | docs・画面の文言 | 済（#47） |
| U40 | Android のホームのカードが「…」のまま固まる件 | S | `app-android/` | 済（#48） |
| U57 | PC の RAW（CR2 など）を解析・選別できるようにする。RAW に埋め込まれたプレビュー JPEG を取り出してサムネイル・dHash・表示用画像の元にする（Android の U49 と同じ規則） | M | `src-tauri/src/raw_preview.rs`・`image_pipeline.rs`・`capture.rs` | 済（PC。Web 版は未対応） |
| U58-A | Android: 解析できなかったファイルの一覧・非対応は再試行しない・準備の分母と選別の対象に含めない（`11-ユーザーフロー.md` §5.3） | M | `app-android/`（`Analyse.kt`・`Failures.kt`・`FailuresDialog.kt`・`Album.kt`・`Home.kt`） | 済（`feat/mb-u58-unsupported-android`。実機・NAS は未確認。PC・Web は別ブランチ） |
| U58 | 解析できなかった写真の一覧・非対応形式は再試行しない・準備の分母と選別の対象から外す（PC・Web。Android は別担当）。`photos.analysis_error_kind` を 1 列追加 | M | `src-tauri/`（`analysis.rs`・`image_pipeline.rs`・`display.rs`・`lib.rs`）・`composables/`・`utils/analysisFailures.ts`・`PhotoFailuresDialog.vue` | 済（PC・Web。Android は未着手。詳細は 10 章 §7 の U58 の行） |
| R2 | 解析の失敗の理由を `PhotoFailure`（`message()`・`kind()`）に集約（PC。挙動・文言は不変） | S | `src-tauri/src/parallel.rs`・`analysis.rs`・`display.rs` | 済（`refactor/mb-r2-analysis-failure`） |

---

## U1 core の「1 つ戻す」を直す（レビュー #2）
- **症状**:
  1. 連写まとめ on で、まとめの中の仲間を手で ★5 にする
  2. 代表を確定すると、仲間は `shift_star(+1)` で 5 のまま（上限）
  3. 「1 つ戻す」で `shift_star(-1)` が効き、仲間が ★4 になる。押す前の ★5 に戻らない（C-4・INV-3）
- **直し方（core）**:
  - `Decision` に、確定の前の星の控え `before: HashMap<String, i32>` を足す。確定で星を動かした写真（代表と仲間）の分を入れる
  - `undo` は差分で戻さず、この控えの値をそのまま書き戻す
  - 足すフィールドは `#[serde(default)]`。**控えが無い古い `Decision` は今までどおり差分で戻す**（保存済みの Session・Android が書いたサイドカーを読めるように）
  - `keep_and_top` の `topped` も同じ控えで扱えるなら一つにまとめる。ただし既存のテストの期待は変えない
- **テスト**: 上の症状の再現を core のテストにする（仲間 ★5 → 確定 → 戻す → ★5）。INV-3 のテストを全部通す。古い形の Session の JSON を読んで戻せるテスト
- **Android**:
  - core を変えたので `.so` と Kotlin の束ね（UniFFI）を作り直す（`app-android/` の手順。`scripts/build-core.mjs` を feat/rebuild から持ってくる必要があるかを先に確かめる）
  - Android のビルドが通ることと、実機の 1 つ戻すを確かめるのはユーザーに頼む
- **wasm**: `pnpm core:wasm` で作り直し、`tests/core-wasm-fixtures.test.mjs` を通す
- 04章に 1 行（`spec-changes.md` に追記）: 「2026-09-30 変更: 1 つ戻すは差分ではなく、確定の前の星に書き戻す」

## U2 メタデータに反映が失敗する（原因は分かっている）
- **原因**: `write_ratings_to_photos_blocking`（`src-tauri/src/lib.rs` 3780 行付近）が、書いたあとの確かめで、拡張子 `.photocurator-tmp` の一時ファイルを `image::ImageReader::open(&temporary)` で開いている。拡張子から形式が分からないので、確かめが毎回失敗し、書き込みを取り消している
- 新版で同じ不具合を直した `6e56524`（`git show 6e56524`）と同じ直し: `ImageReader::open(&temporary)?.with_guessed_format()?` で中身から形式を判定する
- **テスト**: 一時フォルダの JPEG に星を書き、`xmp:Rating` が入ること（Rust の `cargo test --lib`）
- ユーザーの確認: 作業用にコピーしたフォルダで「メタデータに反映」

## U3 写真を移動すると、結果に数は出るが写真が無くなる
- **分かっていること**:
  - #20（review#1）で、移動した写真だけを `is_missing=1` にした
  - 結果の一覧（`get_project_photo_page`）は `is_missing=0` だけを出す
  - 一方、星ごとの数（`get_selection_summary`）が `is_missing` を見ていない見込み（**確かめる**）
  - 移動先はプロジェクトのフォルダの外なので、写真はプロジェクトから消える
- **決めること（実装の前に確かめる）**: 移動した写真を結果に「移動済み」として残すか、数からも外すか
  - おすすめは「数からも外し、移動した旨を 1 回だけ通知」（原本はもうそのフォルダに無いので）
- 数と一覧の条件をそろえる。移動したあと、Session の `ratings` に残る移動済みの写真が、次のラウンドや書き出しに入らないことも確かめる（`getCoreInputs` は `is_missing=0` だけ）

## U4 通知が消えない・画面が下がる
- `app.vue` の `v-alert`（`error`・`taskWarning`・`moveReport`）と `ResultsView.vue` の `resultsMessage` は、本文の中に置かれているので、出ると本文が下がる
- **直し方**:
  - 成功と情報の通知（`moveReport`・`resultsMessage`・`taskWarning` のうち情報のもの）は、画面の下に重ねて出す `v-snackbar`（`location="bottom"`、4 秒で自動で消える、× でも消せる）1 つにまとめる
  - 出す口は `composables/useNotice.ts`（`notify(text, kind)`）
  - **エラー（`error`）は消えないままにする**（何が起きたかを読ませるため）が、本文を下げない位置（上に重ねる）にする
- 確認: 星の一括移動のあと通知が 4 秒で消え、格子の位置が動かない

## U5 プロジェクトの詳細に「最初からやり直す」
- Android の `Album.kt` の「…」メニューの「最初からやり直す」と同じにする
- 旧版の `ProjectView.vue` の右上（削除の隣）に「…」メニューを置き、その中に「選別を最初からやり直す」を入れる。押すと `RestartDialog`（確認）→ `restartFromScratch`（#20 で連写の手直しと学習も消すようにした）
- 削除も同じメニューに入れるかは、見た目の好みなので今のまま（削除ボタン）でよい

## U6 選別の下のボタン列を上に移す
- **並び（Android の `Cull.kt` の上のバーと同じ順）**: 左から ← 中断（**一時停止のアイコン `mdi-pause`**）・1 つ戻す（`mdi-undo`）・複数選択（`mdi-checkbox-multiple-outline`）・（真ん中に「★n を選別中 · ROUND n」と残り枚数）・「…」（選別中の設定）・主ボタン（確定／n 枚とも落とす）
- アイコンは Android の `Cull.kt` で使っているものに合わせる（`app-android/app/src/main/java/app/photocurator/next/Cull.kt` の `Icons.…` を読み、同じ意味の mdi に置き換える）
- 文字は残す（画面が広いので、アイコンだけにしない）。**「1〜0 選ぶ・Ctrl+数字 拡大・Shift+数字 ★5 で確定・Alt+数字 まとめを開く・Enter 決定・⌫ 戻す」の説明の行は消す**
- 複数選択のときのボタン（この写真をまとめる など）は、今の並びのまま上のバーへ移す
- 下のボタン列が無くなった分、写真の格子を下端まで広げる
- 確認: 1440×900 と 933×704 のスクリーンショットで、写真の枠が前より大きい

## U7 選別の並べ方を窓の形に合わせる（**プランを合わせてから**）
- **背景**: 新版で一度試してうまくいかなかった（ユーザー）。新版の `cull.vue` の `gridFor()` は、`ResizeObserver` で測った縦横比から行×列を決めていた（`git show origin/feat/rebuild:pages/project/[id]/cull.vue`）
- **プラン案（ユーザーと合わせる）**:
  1. 並べ方の候補を作る。N 枚に対し、行 r = 1..N、列 c = ceil(N/r)。空きマスが 1 行分以上になる組み合わせは捨てる
  2. 各候補で、1 マスの大きさ = (枠の幅 − 隙間) / c × (枠の高さ − 隙間) / r を出す
  3. 写真の縦横比 a（その組の写真の長辺/短辺の平均。横長の写真が多ければ 3:2）を仮定し、1 マスに収まる写真の面積 = min(マスの幅, マスの高さ × a) × min(マスの高さ, マスの幅 / a) を出す
  4. **写真の面積が最も大きい候補を選ぶ**。同じくらい（差が 5% 未満）なら、今の並びを保つ（窓を少し動かしただけで並びがちらつかないように）
  5. 枠の大きさは `ResizeObserver` で測る（窓の大きさではなく、ボタンのバーを除いた写真の枠）。変わったら 100ms 待ってから計算し直す
  6. 計算は純関数 `utils/gridFor.ts`（`gridFor(n, frameW, frameH, gap, aspect, current?) → {rows, cols}`）にし、テストで 4 枚の 1440×900（→ 2×2）、2560×800（→ 1×4）、800×1200（→ 4×1 か 2×2）などを確かめる
- **決定（2026-09-30、ユーザー）**:
  - 縦横比は**固定（3:2 の横長）**にする（写真ごとには測らない。上のプラン 3 の `aspect` は常に 1.5）
  - 空きマスは、**空きが 1 行未満になる並びまで許す**（プラン 1 のとおり）
  - ちらつき防止は **5%**（プラン 4 のとおり）
  - 前に失敗した症状は、ユーザーからはまだ聞いていない。実装したら、1440×900・933×704・縦長の窓のスクリーンショットを並べて報告し、ユーザーに見てもらう

## U8 表示枚数を、クリックと矢印キーで増減する
- `GroupSizeDialog.vue` のスライダーの左右に − と ＋ のボタンを置く。←→（↑↓）キーでも ±1（最小・最大で止める）。今のスライダーは残す
- 変えたらその場で今の組に効く（今の `core.resize` のまま）

## U9 拡大で Ctrl+スクロールで拡大・縮小する
- 拡大の画面で `wheel` を受け、`ctrlKey` のときだけ倍率を変える（1〜8 倍、マウスの位置を中心に）。ドラッグで動かせる。ダブルクリックで元の倍率（Android の `Zoom.kt` の `scale` と「N.N 倍」の表示と同じ）
- `onZoomKeydown` は、Ctrl・Shift・Alt・Meta の単独の押下では閉じない（今は何かのキーで閉じる）。Esc・← → など決まったキーだけで動くようにする
- ブラウザの Ctrl+ホイールの拡大（ページ全体）は `preventDefault` で止める（`{ passive: false }`）

## U10 拡大の読み込み中のぐるぐる
- `zoomLoading`（`useCurator.ts` 1082 行付近）が true の間、写真の真ん中に `v-progress-circular indeterminate` を重ねる。原本に替わる前は表示用画像を出したまま、その上に重ねる（Android の `Zoom.kt` と同じ）

## U11 複数モードで ★5 を押しても、すぐ次の組へ進まない
> [!success] 2026-09-30 決定: **1（今のまま＝Android と同じ）。変更なしで閉じた**
> 以下は判断のときの記録。
>
> Android の `Cull.kt` を読むと、複数モードの ★5（`onTop`）も `keepTop` → `keepAndTop(live, selected, path)` ですぐ確定して次の組へ進む（今の Web と同じ）。ご要望（進まない）は Android の実装と食い違う。
> core の `keep_and_top` は 1 回目で組が進むので、★5 の印を何枚も付けて主ボタンでまとめて確定する形は、core を変えないと作れない。
> **ユーザーに次のどちらかを決めてもらう**:
> 1. 今のまま（Android と同じ）。U11 は変更なしで閉じる
> 2. Web だけ「印＋主ボタンで確定」にする。印は 1 枚まで（2 枚目で 1 枚目を外す）。core は変えない。封筒に `toppedInGroup` を足し、確定の関数の選び方は純関数に切り出す
> 決まったら `fix/mb-u11-multi-top` を `origin/feat/mb-next` から切って続ける

- Android の `Cull.kt` を読む（複数モードで ★5 を押すと、その写真に ★5 の印が付き、選択の一つとして残る。主ボタンで組を確定したときに ★5 が決まる）
- 今（T4）は、複数モードの ★5 で `keepAndTop` を呼んで、すぐ確定している
- **直し方**:
  - 複数モードの ★5 は「★5 の印」を選択に足すだけにする（封筒に `toppedInGroup: string[]` を足す。保存の形が変わるので、古い封筒は空として読む）
  - 主ボタンで `keepAndTop(session, 選んだ写真, ★5 の印の写真)` を呼ぶ。★5 の印が 2 枚以上なら、core に合わせて 1 枚ずつ呼ぶか、core の口を確かめてから決める（**core に口が無ければ止まってユーザーに聞く**）
  - 単数モードの ★5 は今のまま（押したら確定）

## U12 まとまり編集を横並び＋境目のバーにする
- Android の `Burst.kt` の `BurstEditSheet` と同じにする: 写真を撮影順に横 1 列に並べ、写真のあいだに縦のバー（境目）を置く。バーをタップで「切る／つなぐ」、左右にドラッグで隣の境目へずらす
- 新版の `components/BurstEditSheet.vue`（`git show origin/feat/rebuild:components/BurstEditSheet.vue`）がこの作り（`pointerdown`/`pointermove`/`pointerup`＋`setPointerCapture`）なので、それを旧版の見た目（ダイアログの枠）に合わせて移す
- 形の保存は今の `utils/burstShape.ts`（`isSameBurst` との食い違いだけを保存）のまま

## U13 表示用画像の大きさを作成時から決める
- 旧版には、プロジェクトの画面に「大きな画像で選別する」（1024／1536）がある。PC の Rust には `save_display_edge`（アプリ全体）・`save_project_display_edge`（プロジェクトごと）がある
- **直し方**:
  - アプリの設定の置き場（旧版のサイドバーの下か、ホームの右上）に「表示用画像の既定の大きさ」（768・1024・1280・1536・1920、容量の見込みを添える。FR-7.2）を足す
  - 作成ダイアログにも同じ選択を足し、既定はアプリの設定の値にする
  - 作成時に選んだ大きさで、最初から表示用画像を作る（あとで作り直さない）
  - Web も同じ（`web/store.ts` の設定）

## U14 PC の Amazon の読み込みを速くする（調べてから）
- **まず測る**（実リンク「嵐山」454 枚。PC・Web・Android）: 一覧（走査）・名前・撮影時刻・サムネ・表示用のそれぞれの時間
- **比べる**:
  - PC は `ureq` の同期の 1 本の agent を、並列 4 のスレッドで使っている（`src-tauri/src/amazon.rs`）
  - Web はブラウザ（HTTP/2 で同じ接続に多重化、キャッシュあり）
  - Android（`Amazon.kt`・`AmazonImages.kt`）は OkHttp（HTTP/2・接続の使い回し）で、並列の数も確かめる
- **候補**:
  - (a) `ureq` の agent を 1 つにして使い回す（接続の使い回し。今は呼ぶたびに作っていないか確かめる）
  - (b) HTTP/2 に対応したクライアント（`reqwest` の blocking か、`tokio`＋`reqwest`）で並列 8〜16
  - (c) 一覧のページ（200 件ずつ）を並べて取る
  - (d) サムネは `viewBox=160` を先に全部、表示用はあとで（今の順を確かめる）
- 走査と名前（一覧）は (a)(c) で速くなる見込み。撮影時刻は一覧に入っているので、別に取っていたらやめる。サムネと表示用は帯域しだいで、Android も遅い
- 測った結果と選んだ方法を 10章 §7 に書いてから実装する

## U15 プロジェクト詳細の上のボタンをそろえる（2026-09-30 追加）
- **症状**: 上の行の「削除」「選別結果を見る」などのボタンと、⋮（三点リーダ）の高さがそろっていない
- **場所**: `components/views/ProjectView.vue` 49 行付近（`<div class="d-flex flex-wrap ga-2">` の中）
- **並び（左から）**: ⋮ → 選別結果を見る → 削除 → 写真を再読み込み（出所によって「写真を追加」「フォルダへのアクセスを許可」「再試行」に替わる。**この位置のまま**）→ 選別を再開（主ボタン。始まっていなければ「選別を開始」）
- **高さ**: アイコンだけのボタン（⋮）と文字つきのボタン（text・outlined・主ボタン）で、Vuetify の既定の高さが違う。同じ `size`・同じ高さにそろえる（outlined と text で枠の分ずれないか、実測で確かめる。`min-height`・`height` を CSS で揃えるか、⋮ も `variant="outlined"` にするなど、いちばん素直なもの）
- 画面が狭くて折り返したときも、ボタンの高さが保たれること
- 確認: 1440×900 と 933×704 で、ボタンの `getBoundingClientRect().height` がすべて同じ

## U16 拡大のぐるぐるを、写真の真ん中に重ねる（U10 の見直し）
- **症状（ユーザー）**: 拡大のぐるぐるが右上に出る。Android と同じく、写真の中央に、写真にかぶさるように出したい
- **今のコード**: `app.vue` 171 行（`<v-progress-circular class="zoom-overlay__spinner">`）と `assets/main.css` 254〜255 行（`.zoom-overlay__photo { position: relative; … }`、`.zoom-overlay__spinner { position: absolute; top: 50%; left: 50%; margin: -24px 0 0 -24px; … }`）。**コードの上では中央に見える**ので、**まず実際に開いて位置を測る**（`getBoundingClientRect` とスクリーンショット）
- 右上に出るなら、その原因を直す（Vuetify の `.v-progress-circular { position: relative }` が勝っている、`.zoom-overlay__photo` の幅が写真より広く中央が写真の外にずれる、`img` が `display: grid` の子で高さが違う、など）。写真の見えている範囲の真ん中（画像の `getBoundingClientRect` の中心）に重ねる
- 中央に出ているなら、ユーザーが見たのが古い版（U10 の前）かを確かめ、測定値とスクリーンショットを報告して終わる（直さない）
- 原本に替わったら消える／替わる前は表示用画像を出したまま、その上に重ねる（今のとおり）
- 確認: 幅の違う写真（横長・縦長）で、ぐるぐるの中心と写真の中心の差が 2px 以内

## U17 アプリの設定の画面（表示用画像の既定）
- **背景**: 作成ダイアログには表示用画像の大きさの選択があり（U13）、選んだ値はアプリの既定にもなる。ただし**アプリの設定の画面が無い**ので、既定だけを後から変える場所が無い。表示用画像は作成時に先に作られるので、選別の直前に変えても作り直しは起きる（これは今のままでよい、ユーザー了承）
- **Android と同じにする**: `app-android/app/src/main/java/app/photocurator/next/Settings.kt` の「表示用画像の既定」（標準 1024px／大きく 1536px／詳細 768〜1920、それぞれに容量の見込み）。ホームの右上の歯車から開く
- **Web・PC の実装**:
  - 新規 `components/views/AppSettingsView.vue`（旧版の見た目。`SettingsView.vue` は「トーナメントの設定」で別物なので名前を混ぜない）
  - `View` 型に `'app-settings'` を足し（`composables/useCurator.ts` の `type View`）、`app.vue` で出す
  - 入口: ホームの右上に歯車アイコン（`mdi-cog-outline`）、`AppNav.vue` の下のほうにも「設定」（`mdi-cog-outline`）
  - 選択肢は `utils/displayEdge.ts` の `createDisplayChoices`（作成ダイアログと同じ）。選ぶと `displayEdge` の既定（PC は `save_display_edge`、Web は `web/store.ts` の設定）を保存する。作成ダイアログの初期値がそれに変わること
  - **既存のプロジェクトの表示用画像は作り直さない**（ここで変えるのは「これから作るプロジェクトの既定」だけ。説明文にそう書く）
  - Android の設定にある NAS・選別の既定は、この件では扱わない
- 確認: 設定で 1536 にする → 作成ダイアログの初期値が 1536 → 再読み込み（Web）・再起動（PC）でも残る

## U18 選別の並べ方で、3 枚のときだけ 1×3 に固定する
- **症状（ユーザーの目視、2026-09-30）**: 縦長の写真が 3 枚の組が 2×2（1 マス空き）で出る。1×3 の方が大きく見える
- **原因**: U7 で「縦横比は固定 3:2（横長）」と決めた（`utils/gridFor.ts` の `PHOTO_ASPECT = 1.5`）。横長として評価すると 2×2 が最大になる。枠 1805×840・3 枚・隙間 10 で計算すると、固定 3:2 の評価は 2×2 が 258,338・1×3 が 236,017（2×2 が勝つ）、実際の縦長 3:4（aspect = 0.75）の評価は 1×3 が 472,033・2×2 が 129,169（**1×3 が約 3.7 倍**）
- **決定（2026-09-30、ユーザー）**: **実際の縦横比を測る案は見送る**（写真の縦横とウィンドウの幅を毎回計算する複雑さを避ける。Android は幅と高さが変わらないので気にしなくてよい）。**4〜10 枚の表示ロジックは U7 のまま（固定 3:2・5% の保持）。3 枚のときだけ 1×3 に固定する**
- **直し方**: `utils/gridFor.ts` の `gridFor` で、`count === 3` のとき、枠の大きさ・`current` にかかわらず `{ rows: 1, cols: 3 }` を返す。ほかの枚数の挙動は変えない
- **テスト**: `tests/gridFor.test.ts` に、3 枚が 1440×900 相当・縦長の枠（800×1200）・横に広い枠・`current` が 2×2 のときも 1×3、を足す。4 枚以上の期待は変えない
- **確認**: 「表示枚数」を 3 にして、1×3 で出る。4 枚・7 枚は今までどおり。縦長の窓（600×1000）では 1×3 の 1 マスが小さくなるが、決定どおり固定（実測値を報告に書く）

---

## 済んだこと（2026-09-30）
- ユーザーの実機確認は、おおむね OK（T5 の空から始まる・窓を閉じるとき・NAS・Amazon の向きなど）
- #20 を `feat/main-base` にマージした


## U19 スライドショー選別（2026-09-30 追加。ユーザー承認済みの仕様）
**PC・Web 版（共通の Nuxt の画面）→ Android 版の順。ブランチは `feat/mb-slideshow`（`feat/mb-next` から）。**
- 写真を **1 枚ずつ**出し、残す／落とすを決める。トーナメント（2〜10 枚）と**同じセッション・同じ★の上げ方**で、違いは **1 グループが 1 枚か複数枚か**だけ。落とす＝そのグループで選ばない、残す＝選ぶ（★が 1 つ上がる）。1 周したら残った写真で次のラウンド。★5 確定・連写のまとめ・1 つ戻す・拡大もトーナメントと同じ処理を使う
- 途中で中断し、再開するときに**方式（スライドショー／トーナメント）を選び直せる**。セッションの枚数設定と同じ仕組みで切り替える（中身のモデルは変えない）
- **操作**: 左＝落とす／右＝残す。PC: 画面の左半分・右半分のクリック、左右のドラッグ、`1`（落とす）・`2`（残す）。Web・Android: 左右のフリック、左右のタップ。Web でも `1`・`2` のキーは効かせる。**上スワイプ＝★5 で確定**。下スワイプは使わない。**長押しの拡大は無効**（拡大は右上のボタンだけ）
- **見た目（写真を最大限大きく）**: 「落とす／残す」の帯やボタンは置かない。写真は上バーと進み具合の下から枠いっぱい（余白 6px、縦横比は保つ）。ドラッグ中だけ、写真が回転しながら動き、色の膜＋アイコン＋文字（落とす＝赤系・×、残す＝緑系・ハート）がドラッグ量に応じて濃くなる。決定すると画面外へ飛ぶ。見本: `C:\Users\miyaj\Documents\tools\Koreha\frontend\src\components\SwipeCard.vue` と `views/SwipeView.vue`（動きだけ参考。配色・文字の帯は取り入れない。色と配置は Photo Curator のテーマ）
- **上のバーは、トーナメントと同じ並び・文言**: 左＝中断して戻る・1 つ戻す、真ん中＝「★N を選別中 · ROUND n」と「残り n 枚」、右＝設定（…）。細い進み具合バーも同じ。「複数選択」「この写真をまとめる」「選択なしで次へ」は、スライドショーでは出さない。Android は丸いボタンを置かず、1 つ戻すも上のメニューに置く
- **写真の右上**に、トーナメントと同じ「拡大」「★5 で確定」のボタン
- 落とす・残すの取り消し（1 つ戻す）は必須
- 連写の組は代表 1 枚で出し、トーナメントと同じ「連写 n 枚」の印から、同じ手直しのダイアログを開く
- 将来、目標枚数を決める機能を足してもよいが、今は作らない
- **core の確認（最初にやる）**: グループ 1 枚を core が扱えるか（下限が 2 になっていないか）を調べる。変えるなら最小にして、`cargo test` を通す。core を変えるときは、この節に理由を書いて**そこで報告して止まる**（Android の .so の作り直しがユーザーの作業になるため）

### U19 の core の変更（2026-09-30、ユーザー承認済み）
- **理由**: `start_round`・`resize` が `group_size.max(2)` で 1 を 2 に切り上げていたため、1 枚ずつのグループ（スライドショー）を作れなかった。**下限を 1 にした**（`.max(1)`）。
- 変えていないもの: `next_round`・`round_for` の「通った写真が 2 枚未満なら終了」（ユーザー決定: 今のまま）。セッションの保存形式。`advance`・`undo`・`keep_and_top`・連写の扱い（グループの大きさに依存しない）。
- テスト: `枚数は一枚を下回らない` に直し、group_size 1 の 4 点（1 枚ずつ出る・選ぶと★が上がり選ばないと据え置きで外れる・1 つ戻せる・連写の代表と仲間の★）を追加。`cargo test`（96 件）・`cargo check`（src-tauri）通過。
- **影響**: Android の `.so` の作り直しはユーザーの作業（作り直すまで、group_size=1 のセッションを Android で開くと `resize` で 2 に直る可能性）。PC・Web は core-wasm の再ビルド（`pnpm core:wasm`）が要る。

## U33 サイドカー同期で選別状況が消える不具合の根本対策（2026-10-02）
設計書（「サイドカー（`.photo-curator/catalog.json`）同期の調査と直し方の設計」）の PR-3〜PR-5 に当たる。3 段に分ける。

| 段 | 中身 | ブランチ | 状態 |
| --- | --- | --- | --- |
| U33 | core: 正規化・比較キー・未着手・`sidecar_plan`・混ぜ方（D・E）・鍵の変換・catalog.json v2 の項目。UDL と core-wasm に公開 | `fix/mb-u33-core-sidecar` | 済（#45 に含む。10章 §7 の U33） |
| U34 | PC・Web: `useSidecarSync` を `sidecar_plan` と楽観ロック（lock → 読む → 確かめる → 一時ファイル → rename → 読み戻し）に切り替える。`sidecar_state` に `seen_token`・`seen_key`・`seen_epoch`・`detached`（移行: 古い seen_at/seen_by から `legacy:` の token、localChanged=true なら key を空に）。5 択のダイアログ・B の帯と「NAS に書き込む」・取り込みの「元に戻す」。`lib/core.ts` に新しい関数の型を足す。鍵は書くとき `sidecarKeysToFolder(…, '')`、読んだ直後に `sidecarNormalizeKeys(…, 選んだフォルダ)`、取り込むとき `sidecarKeysFromFolder(…, '', '\\')`（Windows） | `fix/mb-u34-pcweb-sidecar` | 済（#45 に含む。10章 §7 の U34・下の「U34」） |
| U35 | Android: `Sidecar.kt` の判断を core の `sidecarPlan` に置き換える（`SyncState` を token・key・epoch・detached に）。鍵は prefix＝共有の根からのフォルダで変換。プロジェクトごとに 1 本の列・一時ファイル → rename・ON_STOP は開いているプロジェクトだけ・最初の確認が終わるまで「選別を開始」を押せない・5 択のダイアログ。**先に `node scripts/build-core.mjs` で `.so` と Kotlin の束ねを作り直す（ユーザー）** | `fix/mb-u35-android-sidecar` | 済（#45 に含む。下の「U35」の節。実機は未確認） |

### U33 で足した core の公開 API（`core/src/sidecar_sync.rs`。UDL・core-wasm にも同名／camelCase）
- 正規化・比較: `normalize_key`・`canonical_judgement`・`sidecar_judgement`・`judgement_equivalent`・`judgement_key`
- 未着手・要約: `is_untouched`・`judgement_progress`・`progress_cmp`
- 版: `sidecar_token`・`sidecar_seen`・`sidecar_stamp`（書く前に v2 の項目を入れる）
- 判断: `sidecar_plan(seen, local, remote, writable, detached)` → `Settled{seen?,reason}`／`Push{expected,aside_theirs,reason}`／`Pull{theirs,aside_mine,seen,reason}`／`Clash{theirs,mine_progress,theirs_progress,order,reason,preview}`
- 混ぜ方: `merge_stars`・`merge_overrides`・`merge_judgements`・`merge_preview`・`session_from_ratings`
- 鍵: `sidecar_keys_to_folder`・`sidecar_keys_from_folder`・`sidecar_normalize_keys`・`sidecar_key_coverage`
- UDL には `sidecar_to_json`・`sidecar_from_json` も足した（Android が core の形で読み書きできるように）
- 型: `Sidecar` に `write_id`・`based_on`・`lineage`・`epoch`・`key_base`・`progress`（すべて省略可・型違いは読み捨て）。`SidecarProgress`・`Judgement`（と部品）・`SeenRecord`・`SidecarPlan`・各 Reason・`ProgressOrder`・`MergeMode`・`MergePreview`・`MergeResult`・`KeyCoverage`
- 旧い `sidecar_decide` は互換のため残した（UDL には足さない）。新しい呼び出し側は `sidecar_plan` を使う

### 呼び出し側の使い方（U34・U35 向けのメモ）
1. NAS から読む（つながらない・壊れている → plan を呼ばない）→ `sidecar_normalize_keys(remote, 選んだフォルダ)`
2. 端末の状態から `Sidecar` を組み、`sidecar_keys_to_folder(…, prefix)` → `sidecar_judgement` で `local`（または `canonical_judgement` に直接）
3. `sidecar_plan(seen, local, remote, writable, detached)`
   - `Settled{seen: Some}` → 控えをその値にする（警告は出さない）
   - `Push{expected}` → lock を取り、読み直して `sidecar_token` が `expected` と同じか確かめる → `aside_theirs` なら相手を `catalog.<相手>.json` へ → `sidecar_stamp(local の Sidecar, 新しい乱数, Some(remote))` を書く → 控え `{writeId, 書いた写しの judgement_key, epoch}`
   - `Pull{theirs, aside_mine, seen}` → 端末の分を退避（`aside_mine` なら NAS にも）→ `sidecar_keys_from_folder` で端末の形にして取り込む → 控え `seen`
   - `Clash` → 5 択（A 取り込む／B 残す＝detached・控えは NAS の版／C 書き込む／D 積集合／E 和集合＝`merge_judgements`）。C・D・E は `sidecar_stamp(…, Some(theirs))` で書くので、相手の端末からは早送りになる
4. 取り込む前に `sidecar_key_coverage` で一致が半分未満なら取り込まず理由を出す（設計書 §4.8）

### 仮置きの判断（ユーザー未確認。設計書の推奨に無い細部を決めたもの）
1. **早送りは `basedOn` だけでなく、新しい項目 `lineage`（これまでの版の見分け、新しい順に最大 32）でも見る**。PC がラウンドの終わりと窓を隠したときの 2 回書いても、Android から早送りになるように。古いアプリが 1 回でも書くと系統が途切れ、早送りと見なさない（確認になる）
2. 手直しの同じ 2 枚の重複は**先に出た方**を採る（core の `group_bursts` が実際に使う方。設計書の表は「最後」）
3. 星とセッションが同じで手直し・境目が違うとき、**片方だけが多いなら自動**（Q9）。両方に相手に無い分がある・食い違うときは確認（`ExtrasConflict`。和集合の自動はしない）
4. 切り離し中（B のあと）は、#3 と #7 の自動の書き込みをしない。#5（NAS が未着手の版）は設計書どおり書く
5. 見た版のままで端末がやり直した（未着手になった）とき、NAS の着手済みの版は退避してから書く（`aside_theirs=true`）
6. やり直しの検出: 見た版の `epoch` と比べ、NAS 側が変えていれば `TheirsRestarted`、端末側なら `MineRestarted`（端末のやり直しは「未着手」と見なさないので、相手の進んだ版を黙って取り込まない）。一度も見ていない版では epoch で判断しない。**決定により変更（U42。ユーザー決定 2026-10-02）**: NAS 側がやり直した版は、系統がつながっていても早送り（#6）にせず、端末が着手済みなら `TheirsRestarted` で確認する（以前は、端末が見た版のあと何も変えていなければ早送りが先に当たり、確認なしに取り込んでいた）。端末が未着手なら確認なしに取り込む・意味が同じなら何もしない、は変えない。世代の違う版の上に C・D・E で書いた版も、ほかの端末（着手済み）からは確認になる（10章 §7 の U42）
7. 両方とも未着手で中身だけ違う（写真の顔ぶれなど）ときは取り込む（#4。失うものが無い）
8. `version > 2` の版には書かない（`NewerVersion`）。端末が未着手なら取り込みはする
9. 書けない共有では、早送り・手直しだけの取り込みはする。それ以外は `ReadOnly`（この端末だけの結果）。**決定により変更（U42）**: やり直された版は早送りにしないので、書けない共有では取り込まず `ReadOnly`（端末の分を消さない）
10. 進み具合の比べ: ROUND（終われば半歩）→ 同じ ROUND なら決めた組の数 → ★1 以上の数。食い違えば `Unclear`。全部同じなら手直しの数・境目の学習
11. 未判定（積集合）: 途中のセッションの `current ＋ queue` とその連写の仲間。セッションが無く★0 も未判定（設計書どおり）。両方未判定は小さい方
12. 混ぜた完了状態: ROUND と対象の★は両方の大きい方、`survivors` は対象を超え★5 未満、連写のまとまりは端末の分（無ければ NAS の分）、`group_size` は呼ぶ側が渡す。世代が違えば呼ぶ側が作った新しい `epoch`。**U45 により変更（2026-10-03。実機の報告: 残り 444 枚が未選別のまま ROUND 1 が終わった）**: D・E を「いつも完了状態にする」のをやめた。どちらの端末もまだ見ていない写真が残るなら、**未完了のまま、その写真を queue に残して続きから選別できる形**にする（判定済みの写真は★と `survivors` に入れ、片方だけが判定した写真はその側の判断）。完了状態にするのは残りが無いときだけ。途中の ROUND があるのに 2 つの ROUND か対象の★が違うときは混ぜない（`merge_preview` の `mergeable = false`。ダイアログで D・E を押せなくする）。詳しくは 10章 §7 の U45
13. 古い形の鍵の推定: `keyBase` が無い版は、全部の鍵に共通の頭のフォルダが、この端末で選んだフォルダのパスの末尾と一致すれば外す（Android の `photo/x` にも PC の `\\NAS\share\photo\x` にも効く）。合わなければ外さない
14. NFC にそろえるのはフォルダ形式へ変えるときだけ。端末の形へ戻すとき（`from_folder`）は NFC のまま（macOS 由来の NFD の名前だと端末の鍵と合わない可能性。U34・U35 で必要なら端末側の鍵も NFC にする）
15. `progress` に `overrides`（手直しの数）・`learned`（境目の学習）を足した。`total` は `sidecar_stamp` のとき（`photos` と `session.ratings` の多い方）
16. v2 の項目は型が違えば読み捨てる（catalog.json 全体は「壊れている」にしない）
17. 比較キーは `j1:` ＋ SHA-256 の 16 進。正規形の作り方を変えたら `j2:` にする
18. `Settled` の「控えを進めるか」は `seen: Option<SeenRecord>` で表し、`Pull` も控える値を返す
19. 鍵の一致率の 50% の線引きは呼ぶ側（`sidecar_key_coverage` は数だけ返す）
20. ロック・退避ファイル・壊れたファイルの扱い（`catalog.broken-<時刻>.json`）は呼ぶ側（U34・U35）

### テストと設計書 §3 の表の対応
core の `core/src/sidecar_sync/tests.rs` に、関数名の末尾「_3のn」で §3 の行を示した。1〜5・7・9・10・12・13・16 は判断の表どおり、6・8（壊れた・書きかけは読めないことにする）・11（同じフォルダの別プロジェクトも黙って上書きしない）・14（明示の保存も同じ判断）・15（取り込み後に落ち着く・書いた版は相手から早送り）・17（書いている間の判断は次に変更ありとして残る）は core で確かめられる範囲だけ。ロック・rename・保存の列・最初の確認が終わるまで開始させない、は U34・U35 の呼び出し側のテストで確かめる。

### 再ビルド
- PC・Web: `pnpm core:wasm`（`core-wasm/pkg` は gitignore）
- **Android: `node scripts/build-core.mjs`（`.so` と uniffi の Kotlin。gitignore）はユーザーの作業**。U33 では `--debug` で通ることまで確かめた（arm64-v8a の `.so`・Kotlin に `sidecarPlan` などが出る）。今の Android の動きは変わらない（新しい関数はまだ呼ばない）

## U35 Android のサイドカー同期を core の `sidecarPlan` に切り替える（2026-10-02）
設計書の PR-5（PR-1 の応急処置の中身も含む）。**PC・Web の U34 と両方がそろって初めて、PC と Android を同じ NAS のフォルダで同時に使うのが安全になる**（片方だけだと、新しくない側が確かめずに上書きする。設計書 §4.9 の最後の行）。両方を同じ時期に入れること。

### 切り替えた範囲
- 新しい `SidecarSync.kt`（Android に依存しない）: 判断は core の `sidecarPlan`。NAS は `CatalogIO`、端末の控えは `SeenStore`、端末の選別状況は `LocalState` の interface 越し（JVM テストで偽物を渡す）
- プロジェクトごとに 1 本の列（区切りの書き込みは積むだけ、確認はその完了を待つ）・楽観ロック（`catalog.lock` → 読んで見た版と同じか → `.catalog.<writeId>.tmp` → rename → 読み戻し。違えば最大 3 回判定し直す）・取り込む前の退避（端末 `filesDir/aside/<プロジェクト>-<時刻>.json` を最後の 3 つ、NAS `catalog.<端末>.json`）
- `Sidecar.kt` はつなぎだけ。`SyncState` は `-seenToken`・`-seenKey`・`-seenEpoch`・`-detached`・`-epoch`（古い `-seenAt`/`-seenBy`/`-dirty` は最初に読んだときに移して消す）。`touch`（dirty の印）の呼び出しは消した
- `Smb.write` は一時ファイル → rename。`rename`・`createExclusive`（`FILE_CREATE`）・`delete` を足し、書けるのは `.photo-curator` の下だけにした
- プロジェクト画面: 確認が終わるまで「選別を開始」を押せない（「NAS を確認中…」。つながらなければ押せる）。始める・続ける前にも同期。メニュー「NAS に保存」も同じ判断。切り離し中は「この端末だけの結果（NAS とは別）」と「NAS に書き込む」
- 5 択のダイアログ `SidecarDialog.kt`（文言は設計書 §4.6。定数は `SidecarSync` の companion）。ON_STOP は開いているプロジェクトだけ
- 写真の鍵: 書くとき `sidecarKeysToFolder(…, 共有の根からのフォルダ)`、読んだ直後に `sidecarNormalizeKeys(…, フォルダ)`、取り込むとき `sidecarKeysFromFolder(…, フォルダ, "/")`。取り込む前に `sidecarKeyCoverage` で一致が半分未満なら取り込まない
- テスト: `app-android/app/src/test/.../SidecarSyncTest.kt`（20 件）。`build.gradle.kts` で PC 向けの core（`core/target/debug` の cdylib）を UniFFI の `libraryOverride` で読ませる（`testDebugUnitTest` の前に `cargo build --lib`）

### 仮置きの判断（ユーザー未確認）
1. 書けるかどうか（`writable`）は常に true で渡す（書けない共有は、書いたときの失敗として出す）
2. ロックの古さは中身の `at`（書いた端末の時計）で 60 秒。形は `{"device","name","at"}`（U34 と同じ名前 `catalog.lock`）
3. 書き込みが「読んでから書くまでに変わった」なら最大 3 回まで判定し直し、それでもだめなら「あとでもう一度」
4. 取り込みの退避: 端末が未着手なら退避しない。NAS への退避（`aside_mine`）に失敗しても、端末に退避できていれば取り込む。**端末への退避に失敗したら取り込まない**
5. セッションが無く星だけある版を取り込むときは、その星の「完了した状態」（`session_from_ratings`、ROUND 1）にする（星を落とさない）
6. 取り込んだあとの控えの比較キーは、取り込んだあとの端末の選別状況から作る（5 のように形が変わっても、次に「変更あり」と読まない）
7. B（残す）の控えは NAS の版（比較キーも NAS の中身）。端末と違うので「変更あり」のまま、切り離し中は書かず、NAS の同じ版では聞き直さない。NAS の早送りでも黙って取り込まない（確認になる）
8. 意味が同じ（Settled の Same）になったら切り離しを解く
9. ダイアログを閉じた（外を押した・戻る）ときは何も変えず、選別を始める前にもう一度聞く（03 の「選ぶまで始めさせない」は、Q8 のとおり B が「先へ進む」役を持つ）
10. ダイアログを出したあとで NAS が変わっていたら、答えを実行せずに判定し直す（新しい確認になることもある）
11. D・E の手直し・境目・世代は core の `merge_judgements` のまま（食い違えば端末）。1 組の枚数は端末の設定（`Prefs.groupSize`）。混ぜた結果は端末に入れてから C と同じ書き込みをする（書けなかったら端末は混ぜたまま、次に開いたときにまた判断）
12. 背面への移動（ON_STOP）・画面を離れる・ラウンドの終わりは「書くだけ」。取り込み・確認になる場合は何もしない（次に開いたとき）
13. やり直しは新しい `epoch`（`e-` ＋乱数 16 文字）。初めて同期する前のやり直しは世代を見ない（core）
14. 「元に戻す」（取り込みの直後）は見送り。端末の `aside/` に最後の 3 つが残る（手で戻す手がかり）
15. 写真の場所の一致率は、端末の一覧（`Listing`）がまだ無ければ確かめない
16. 書き込みの通知: 書いたら「この端末の結果を NAS に保存しました」（退避したら「（NAS にあった記録は catalog.<端末>.json に残しました）」）、取り込んだら「<端末名> の記録から続きを取り込みました」。意味が同じ・変更なしは何も出さない
17. 文言で設計書に無いもの: 理由「この端末で最初からやり直しています」（MineRestarted。**U42 で「この端末で最初からやり直したあと、ほかの端末で選別が進んでいます」に変更**。TheirsRestarted は「ほかの端末が最初からやり直しました」）、「★と選別の進みは同じで、連写のまとまりの手直しか学習した境目が違います」（ExtrasConflict）、要約の「境目を学習済み」。「こちらが進んでいます」は名札の次の行

### 確かめたこと・まだのこと
- `./gradlew testDebugUnitTest`（92 件。新規 20）・`assembleDebug`・core の `cargo test`（core は変えていない）
- 再現テストは、今までの規則（見た版と同じか × 変更があるか、列の待ちなし）に差し替えて**赤**（9 件失敗。テスト 2 は Pull＝今回の不具合、2b・2c は確認が出る、テスト 1 は確認が書き込みを追い越して読む）を確かめてから緑にした
- エミュレーター（dev_pixel8）: ダイアログを一時的な差し込みで出し、縦（412dp。1 列）と横（左右 2 列）で崩れないこと・クラッシュしないことを見た（差し込みはコミットしていない）
- **未確認**: NAS を使った実機の確かめ（設計書 §5 の末尾「実機での確かめ方」の 1〜9。U34 と合わせて行う）、smbj の `rename`・`FILE_CREATE` が実際の NAS で期待どおり動くか、ほかの ABI
## U34 PC・Web のサイドカー同期を core の `sidecar_plan` に切り替える（2026-10-02）
設計書の PR-4（PR-2 の応急処置の中身も含む）。core と `app-android/` は変えていない。

### 切り替えた範囲
- `composables/useSidecarSync.ts`: 判断は `core.sidecarPlan` だけ。読む → 鍵をそろえる → 判断 → 実行を、プロジェクトごとに 1 本の列で行う。変わったかは「見た版の比較キー ≠ 今の比較キー」（古い控えは `legacy:` に読み替え）
- 書き込み: 楽観ロックの `writeSidecarChecked`（PC は `src-tauri/src/sidecar.rs` の `write_checked` と `lib.rs` の `write_sidecar_checked`、Web は `HandleFolderIO.writeSidecarChecked`）。退避（`catalog.<id>.json`）は従来の `writeSidecar`
- 控え: `sidecar_state`（SQLite）と IndexedDB に `seenToken`・`seenKey`・`seenEpoch`・`localEpoch`・`detached`
- 取り込み（`applyLocal`）: 鍵を端末の形に戻す・星は Session があれば Session から・相手に無い Session と境目は端末が未着手でない限り残す・鍵の一致が半分未満なら取り込まない
- 5 択のダイアログ（`components/dialogs/SidecarConflictDialog.vue`）・プロジェクト画面の「この端末だけの結果（NAS とは別）」と「NAS に書き込む」・お知らせ（取り込んだ など）
- タイミング（`composables/useCurator.ts`）: 開いたとき・「選別を開始」の前・選別／結果から戻ったとき＝全部の判断。ラウンドの終わり・ホームへ戻る・隠れる・窓を閉じる＝書くだけ。やり直しで `markRestarted`
- `lib/core.ts` に U33 の型と関数（`sidecarPlan` ほか）

### 仮置きの判断（ユーザー未確認。設計書の推奨に無い細部）
1. 楽観ロックの「見た版」は、token ではなく**読んだ catalog.json の中身そのもの**で比べる（Rust に版の解釈を持たせない。token より厳しい）
2. 書く直前に別の端末が書いた、が続いたら 3 回で諦め、何もせず次の契機に回す（設計書の「2 回、その後は確認」の確認は出さない）
3. ロックは `catalog.lock`、60 秒より古いものは壊す。取れなければ書かずに「ほかの端末が書き込んでいた」。Web は「無いのを確かめて作り、読み直す」（原子的ではない。本命は読み戻し）
4. 自動の書き込み（ラウンドの終わり・ホーム・隠れる・窓を閉じる）は**書くだけ**。取り込みと確認は次に開いたとき（設計書は背面だけ明記。残りも同じ扱いにした）
5. 選別・結果からプロジェクト画面へ戻ったときに、全部の判断を走らせる（ラウンドの終わりで見つかった食い違いはここで出る）
6. 「選別を開始」の前の確認で取り込んだら、**始めずにプロジェクト画面へ戻す**（そのまま始めると取り込んだ星を 0 にするため）。食い違えばダイアログで止まる
7. 相手に Session が無い取り込みでは、端末が未着手でなければ Session を残し、星だけ取り込んだ値にそろえる（`utils/ratingEdit.ts` の `applyChanges`。開いたときの自己修復で戻されないように）。境目も相手に無ければ残す。A（取り込む）でも同じ
8. 取り込んだあとの控えの比較キーは「取り込んだあとの端末」の値（設計書 §4.4 どおり）。残した Session・境目は、次に端末が変わったときに書かれる
9. 鍵の一致が半分未満なら取り込まず、赤字で理由を出す（ダイアログにはしない）。A でも同じ
10. 端末の中の退避（設計書の `aside/`）と「元に戻す」は**見送り**。退避は NAS の `catalog.<id>.json` だけ（A・早送りの端末の分、C と「NAS に書き込む」の NAS の分、D・E の両方）
11. B（残す）の控えは NAS の版。意味が同じになったとき・取り込む／書く／混ぜるときに切り離しを解く
12. 書けなかったときは、書けるかを取り直してダイアログを残す（書けない共有なら C・D・E を隠す）。何も変えない
13. 「今すぐ保存」は開いたときと同じ判断（食い違えばダイアログ、取り込めば読み直す）。変更が無ければ「保存する変更はありません。」をお知らせ（赤字にしない）
14. やり直しと「星を全部消してやり直す」（開始の確認）で、この端末の世代 `localEpoch` を `e-<乱数>` にする
15. 古い控えのままで変わっていなければ（NoChange）、その場で新しい形に移す。古い `localChanged` は古い版のアプリ向けに書き続け、新しい控えがあるときは判断に使わない
16. 端末の鍵の区切りは、行に `\` を含む写真があれば `\`、無ければ `/`
17. 退避のファイル名は従来どおり `catalog.<id 先頭 12 文字>.json`（同じ端末の退避は上書き）
18. D・E の `group_size` は端末の Session、無ければ NAS の Session、無ければ 4
19. 取り込んだお知らせは「＜端末名＞ の記録を取り込みました。」、意味が同じときは何も出さない
20. ダイアログの文言: 見出し「NAS の記録と、この端末の記録が違います」、理由の 1 行（設計書 §4.6）、両側の 1 行（★1 以上・ROUND と途中／完了・決めた組・残り・手直し・境目）、ボタンの下に短い説明、下に「どれを選んでも、元の記録は消しません。…」（端末の中には控えないので、設計書の「この端末にも控えます」は書かない）

### 既存テストの期待を変えたもの（理由）
- `sidecarSync.test.mjs` の「記録に Session が無ければ、Session と距離を空にする」→ **反転**（ユーザーの決定・設計書 PR-2 の 1。テストにコメント）
- 「readonly: 端末に変更があっても取り込む」→ 取り込まない（設計書 §4.3 の #8）
- 「Pull（端末に判断があるのに取り込む）」「時刻の大小」の入力 → 端末を未着手にした（両方着手で違えば確認、のユーザー決定）。古い fixture は photos と Session の星が食い違っていたので、そろえた
- 「食い違いの選択」「書けなければ読むだけとして取り込む」→ 5 択と「決めつけない」に置き換え（設計書 PR-2 の 5）
- 「markChanged のあと書く」など → 印だけでは書かないので、端末に実際の判断を入れる形に。状態の比較は項目が増えたので `toMatchObject`
- src-tauri の `state_defaults_to_never_seen_and_round_trips` は構造体に項目が増えたので `..Default::default()`。`tests/folderScan.test.ts` は `SourceIO` の口が増えたので空の実装を足した

### テスト
- `tests/sidecarSync.test.mjs` 48 件（2 台が 1 つのファイルを共有する偽物。PC 視点・Android 視点の再現、楽観ロック、`markChanged`、書いている最中の変更、5 択それぞれ、出し直し、書けないとき、鍵の `\`／古い Android 形式、やり直し）。**旧実装で 30 件の失敗を確かめてから実装した**
- `tests/curatorSidecar.test.ts` 3 件（ラウンドの終わりで書く・開始の前に取り込んで始めない・食い違えば開始で止まる）
- src-tauri `cargo test --lib` 97 件（sidecar 16 件。新規 7: 新しい列の往復・古い表への列の追加・楽観ロックの作成と置き換え・見ていない版には書かない・新しいロックで待ち古いロックを壊す・失敗してもロックを放す・結果の文字列）

### 実機での確かめ方（PC と Web の 2 台。Android は U35 のあと）
準備: 試験用のフォルダ（数十枚）を、PC（`pnpm tauri:dev`）と Chrome（Web 版・「フォルダを選ぶ」で書き込みを許可）の両方でプロジェクトにする。毎回 `.photo-curator/` を消してから始め、`catalog.json` の `updatedByName`・`writeId`・`basedOn`・`keyBase`・`progress` と、`catalog.*.json` の有無を控える。
1. **未着手の版で上書きしない**: Web で 3 組選んでホームへ戻る（書かれる）→ PC で「選別を開始」まで進む。**期待**: 開始の前に Web の続きを取り込み、プロジェクト画面に戻って「ブラウザ の記録を取り込みました。」。星は 0 にならない
2. **意味が同じ**: `catalog.json` をエディタで開き、並びと空白を変え、`updatedAt` を書き換えて保存 → PC で開き直す。**期待**: 何も出ない・書かない
3. **早送り**: 1 のあと PC で 3 組 → ホームへ戻る → Web で開き直す。**期待**: 確認なしに取り込む。`catalog.<Web の id>.json` に Web の分が残る
4. **両方で進める**: 3 のあと両方で 2 組ずつ進めて、Web → PC の順にホームへ戻り、PC で開き直す。**期待**: 5 択。A〜E を 1 つずつ（毎回 4 からやり直して）選び、端末の星・`catalog.json`・`catalog.*.json`・もう一方で開いたときの挙動（C・D・E は確認なしに取り込む）を見る。B のあとは「この端末だけの結果（NAS とは別）」と「NAS に書き込む」が出て、自動では書かない
5. **開いている間に相手が書いた**: PC でプロジェクトを開いたまま、Web で選別して書く → PC で選別してホームへ戻る。**期待**: PC は書かない（Web の版のまま）。開き直すと 5 択
6. **ロック**: `.photo-curator/catalog.lock` を手で作っておき、PC でホームへ戻る → 書かない（「ほかの端末が書き込んでいた」）。1 分後なら壊して書く
7. **やり直し**: PC で「選別を最初からやり直す」→ ホームへ戻る → Web で（その前に Web で 1 組進めておいて）開き直す。**期待**: 「ほかの端末が最初からやり直しました」の 5 択。Web で 1 組進めて書いたあと何も変えていなくても 5 択（U42 で変更。以前は早送りで確認なしに空になった）。Web が未着手なら確認なしに取り込む
8. **鍵**: 入れ子のフォルダを含む写真で 1〜3 をやり、PC の `catalog.json` の鍵が `sub/IMG.JPG`（`/` 区切り・`keyBase: "folder"`）になっていること
9. **古い catalog.json**: U34 より前の PC か Android が書いた `catalog.json`（`version` なし・Android はフォルダ名付きの鍵）を置いて、未着手のプロジェクトで開く → 確認なしに取り込み、★の数が合う（Web の開発用フォルダで確認済み）

### 見送り
- 端末の中の退避と、取り込みの「元に戻す」（仮置き 10）
- 同じフォルダのプロジェクトが端末に 2 つあるとき（設計書 §3 の #11・Q11）・壊れた catalog.json の `catalog.broken-<時刻>.json` への移動（今は今までどおり理由を出して何もしない）

### 迷った点
- core の判断では、相手が**やり直した**版でも、この端末が見た版のあと何も変えていなければ早送り（#6）が先に当たり、確認なしに空になる（端末の分は NAS の `catalog.<自分>.json` に退避される）。設計書 §4.3 では #9 で確認。core は変えない決まりなので、そのままにした（必要なら core の順番を変える）。→ **U42 で core の順番を変えた**（ユーザー決定 2026-10-02。確認する）
- 自動の書き込みで「取り込み」を後回しにしたので、窓を隠しただけでは取り込まない（次に開いたとき・戻ったときに取り込む）

### 未確認
- Tauri の実機（ロック・rename・NAS 越しの読み戻し）、Web の書き込み（File System Access。偽物でだけ確かめた）、ダイアログの実画面（書ける 2 台を用意できなかった）
- Android は U35（今の Android は旧い判断のまま。PC が U34 で書いた v2 の catalog.json を読んでも壊れないことは U33 のテストで固定済み）

## U48 「同名の JPEG と RAW を 1 枚として扱う」をサイドカーで同期する（2026-10-03）

ユーザー決定（2026-10-03）: U46 のプロジェクト設定（PC・Web `pair_raw_jpeg`／`pairRawJpeg`、既定オン）の ON・OFF を、
サイドカー（NAS のフォルダの `.photo-curator/catalog.json`。NAS 以外のプロジェクトは端末）にも記録して同期する。
PC と Android で違う値にしたときは**新しく切り替えた方**を採る（確認ダイアログは出さない。選別状況の食い違いとは別扱い）。

core・PC・Web は `fix/mb-u48-settings-sync` で済み（10章 §7 の U48 の行）。**Android の配線も U51 で済み**（`fix/mb-u51-android-settings-sync`。10章 §7 の U51 の行）。

### 形（catalog.json v2 に足した省略可能な項目）

```json
"settings": { "pairRawJpeg": { "value": true, "at": 1790955613101 } }
```

- `at` は切り替えた時刻（ms）。0 は「作ったまま一度も切り替えていない」（NAS に値があれば NAS を採る）
- 設定は**選別状況ではない**。比較キー・`judgementEquivalent`・`sidecarSeen` には入らない
- 知らない設定は core が読んで書き戻す（`SettingsRecord.other`）。`sidecarStamp(mine, writeId, base)` は、
  `base`（置き換える NAS の版）にあって `mine` に無い設定を引き継ぐ

### core の API（UDL・Kotlin。`node scripts/build-core.mjs` で作り直すこと）

- `Sidecar.settings: SettingsRecord?`（既定 null。今の Kotlin はそのままコンパイルできる）
- `SettingsRecord(pairRawJpeg: SettingValueBool?, other: Map<String, String>)`（`other` は必須。新しく作るときは `emptyMap()`）
- `SettingValueBool(value: Boolean, at: Long)`
- `settingsResolve(local: SettingsRecord?, remote: SettingsRecord?): SettingsPlan`
  - `SettingsPlan.Keep`／`SettingsPlan.AdoptRemote(value, at)`／`SettingsPlan.PushLocal`
  - 規則: どちらかが無ければある方、両方あって値が同じなら Keep（at が違っても）、違えば at が新しい方、同じ at なら NAS
- `sidecarPlan` の #7b: 版の見分けは違うが NAS の選別状況の比較キーが見た版のもの（ほかの端末が設定だけを書き直した版）なら、
  #3 と同じく端末の変更を確認せずに書く（Push LocalChanged）。Android は呼ぶだけで効く

### Android への依頼（U49 の写真の一覧・RAW・トグルの画面のあと）— **U51 で済み**

1〜5 はすべて U51 で済み（2026-10-03）。違いは 2 点: 取り込むのは Open に加えて Explicit（「NAS に保存」。判断が Open と同じ）でも行う。
書けない共有で設定だけを書けなかったときは、黙らず Blocked で理由を出す（NAS は変えない）。以下は依頼の元の文（記録として残す）。

1. **設定の置き場**: プロジェクトごとに `pairRawJpeg`（既定 true）と `pairRawJpegAt`（既定 0）を持つ（U49 でトグルを足すときに一緒に）。
   画面で切り替えたら `at = 今の時刻`。作成時に既定のままなら 0
2. **書くとき**（`SidecarSync.folderSidecar`）: `settings = SettingsRecord(SettingValueBool(value, at), emptyMap())` を入れる（at が 0 でも値は書く）
3. **読んだとき**（`sync` の中、`readRemote` のあと・`sidecarPlan` の前）: `settingsResolve(mine.settings, remote.settings)` を呼ぶ
   - `AdoptRemote(value, at)`: 開いたとき（`SyncMode.Open`）はプロジェクトの設定を `value`・`at` にし、
     「ほかの端末の設定に合わせて「同名の JPEG と RAW を 1 枚として扱う」を{オン/オフ}にしました。写真を反映するには「写真を再読み込み」を押してください。」を出す（自動では再走査しない）。
     どのモードでも、この回に書く版（Push・C・D・E・退避）の `settings.pairRawJpeg` は NAS の値にする（古い端末の値で上書きしない）
   - `PushLocal`: 書く版に端末の値を入れる（2 のとおり）
4. **設定だけが変わったときに書く契機**: `sidecarPlan` が `Settled`（理由 `Same`・`NoChange`・`Nothing`）で、`PushLocal` かつ端末の `at > 0`、
   書ける共有なら、既存の楽観ロックの書き込み（`push` と同じ経路・ロック・読み戻し）で書く。`Detached`・`ReadOnly`・`NewerVersion` では書かない。
   一度も切り替えていない既定値のためだけには書かない。トグルを切り替えたら、いつもの区切りの書き込み（`pushIfChanged`）を呼ぶ
5. **テスト**（JVM の `SidecarSyncTest` の偽物で、先に赤）: PC が切り替えた版を取り込む・新しい方が勝つ・同じなら何もしない・
   settings なしの古い版・設定の違いだけでは確認も「変更あり」も出ない（PC 側は `tests/sidecarSync.test.mjs` の U48 の節と同じ場面）

### 迷った点（PC・Web で決めたこと。Android も合わせる）

- 自動の書き込み（背面へ回る・ラウンドの終わりなど）では端末の設定を変えない（お知らせを出せないため）。書く版にだけ NAS の値を入れ、
  端末は次に開いたときに取り込む
- 写真 0 枚で作った直後は走査が先に走るので、NAS の設定を取り込むのは走査のあと（お知らせで再読み込みを促す）
