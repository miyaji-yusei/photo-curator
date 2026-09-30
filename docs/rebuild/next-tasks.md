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
| U20 | 表示用画像の px の不具合（1536 で作られない・作成ダイアログの値がアプリの既定に負ける）と、詳細の ⋮ から px を変える機能 | M | `useCurator.ts`・`ProjectView.vue`・新規 `DisplayEdgeDialog.vue`・`utils/displayEdge.ts`・`lib.rs`（`get_display_settings`）・`backends/local.ts` | 済（#42。PC 実機は確認待ち） |

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
