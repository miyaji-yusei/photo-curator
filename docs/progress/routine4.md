# Routine 4〜6: 表示の3件と連写の事後扱い

実施日: 2026-07-28。**git 操作は一切していない。**変更はすべて未 stage で積み上がっている。
写真原本には触れていない（読み取りのみ）。本番 DB のスキーマ変更は冪等 migration 1列のみ。

前提確認: `docs/progress/FINAL.md` を読み、Routine 1〜3（2026-07-23 の dHash 高速化）が
完了済みであることを確認したうえで着手した。**この文書の採番はその続き。**

利用者からの指摘は4点。すべて原因を特定し、3つのルーティンに分けて実装した。

| # | 指摘 | 担当 | 状態 |
|---|---|---|---|
| 1 | 一覧（プロジェクト詳細・選別結果）で写真が横倒し | Routine 5 | **完了** |
| 2 | 選別時、画像がホバーしないと暗い | Routine 4 | **完了** |
| 3 | 連写のまとまりも、ホバーしないと画像が白く見える | Routine 4 | **完了** |
| 4 | 連写は選別後どう扱われているのか（事後の画面が無い） | Routine 6 | **完了** |

最終確認（実アプリでの目視）だけが残っている。§5 を参照。

---

## 1. Routine 4: 画像を常に正しい明るさにする

### 原因

指摘 2 と 3 は**同じ1行**が原因だった。`assets/main.css`:

```css
.tournament-card:not(.is-selected) img { opacity: .82; transition: opacity .12s; }
.tournament-card.is-selected img, .tournament-card:hover img { opacity: 1; }
```

- 通常カードは背景が `#16181d` なので、0.82 が素直に「暗い」として出る。
- 連写カードは `background: #ffffff` なので、**半透明の画像越しに白が透けて白飛びして見える**。

どちらもホバーで `opacity: 1` に戻るため「ホバーすると正しく見える」という症状になっていた。

### 直した内容

**画像に掛かる減光を全廃した。** 選択とホバーの区別は枠の色・発光・チェックだけで示す。

1. 上記2行を削除。
2. `.tournament-card::before, .tournament-card .v-card__overlay { opacity: 0 !important; }` を追加。
   `v-card` はホバーで白い膜を写真の上に掛けるので、画像の見えを変える経路をすべて塞ぐ。
   `.choice-card` には既に同じ無効化が入っていた。
3. ホバーの手応えを `outline` に移した。

```css
.tournament-card:hover { outline: 3px solid rgba(214,255,115,.75); outline-offset: 2px; }
```

**3 が必要な理由**: 1 と 2 だけだと、連写カードのホバー表示が完全に消える。
`.tournament-card.is-burst { border-color: #ffffff }` が `.tournament-card:hover { border-color }` と
同じ詳細度で**後ろにある**ため、連写カードの枠はホバーしても白のままで、これまで唯一の
手応えが画像の減光だった。`outline` はカードの**外側**に描かれるので写真に重ならず、
`overflow: hidden` にも切られず、白い枠でも暗いカードでも同じように見える。

### 触らなかったもの

`.result-tile.is-eliminated img { opacity: .45 }` と `.result-tile.is-unpicked { opacity: .4 }` は
**ホバーで変化しない恒常的な表現**で、今回の指摘（ホバーで変わるのが困る）には当たらない。残した。

### 検証

`pnpm dev` を起動し、ブラウザで実際の CSSOM を走査した。

- `.tournament-card` に効く **42 ルール**のうち、画像に `opacity` / `filter` を掛けるものは
  **ゼロ**（メディアクエリ内も含めて走査）
- 通常・連写・選択済みの3種すべてで `getComputedStyle(img).opacity === "1"`、`filter: none`
- `.v-card__overlay` の opacity は `0`
- `:hover` に `outline: rgba(214,255,115,.75) solid 3px` が乗っている

**検出器が空振りしていないことも確かめた。** 削除した規則をわざと注入すると
`opacity: 0.82` として検出され、外すと消える。（最初に書いた走査は CSSStyleRule も
入れ子CSS対応で `cssRules` を持つことを見落として全件を取りこぼしており、0 件という
誤った結果を返していた。selectorText の有無で分岐して直した。）

---

## 2. Routine 5: サムネイルに EXIF Orientation を焼き込む

### 原因

一覧は `photoThumbnailUrl`（Rust が生成した JPEG）、選別・拡大は `photoUrl`（原本）を使う。
原本は WebView が EXIF Orientation を適用するが、`decode_hash_source` は Orientation を
一切読まないので、生成物に回転が焼き込まれない。だから一覧だけ横倒しになる。

**実データで裏を取った。** `L:\Pharfaite Showcase4` の写真は 6000×4000 で
**Orientation=6**（時計回り90°）。まさにこのケース。

### 直した内容（`src-tauri/src/lib.rs`）

1. `orientation_field(fields, ifd)` — 読み込み済みの EXIF から Orientation を取る。
2. `exif_orientation(path)` — 原本の IFD0 の値。読めなければ 1（無変換）。
3. `apply_orientation(image, orientation)` — 1..=8 を `rotate90/180/270` と `fliph/flipv` で
   表現する。**規格外の値（0・9・65535）では回さない。**
4. `decode_hash_source` の3経路すべてで、返す時点で焼き込み済みにした。
5. `exif_thumbnail_image` は画像と一緒に Orientation を返すようにし、**IFD1 の値を優先**する。
   埋め込みサムネイルを既に正立させて保存するカメラで、IFD0 の値を当てると二重に回るため。
   IFD1 に無ければ IFD0 に従う。

### サムネイルの作り直し

`D_HASH_VERSION` を上げるだけでは足りない。`analyse_photo` の `usable` 判定は保存済み
ファイルをそのまま使い回すため、版が古くても「サムネイルから再ハッシュ」するだけで
**中身は作り変わらない**。そこで別建ての版を持たせた。

- `const THUMBNAIL_VERSION: i64 = 1`
- `photos.thumbnail_version` 列を `add_column_if_missing` で追加（冪等）
- `usable` の条件に版の一致を追加
- `get_analysis_backlog` の条件、fingerprint 変更時に列をリセットする upsert、
  結果を書き戻す UPDATE の3か所にも同じ列を通した
- `CachedAnalysis` に `thumbnail_version` を足し、`bin/bench.rs` の SELECT も揃えた

旧ビルドが作った行は NULL になり版が合わないので、**プロジェクトを開いた時点で
backlog が非0になり、自動で作り直しが走る。** 利用者の操作は不要。

**影響**: dHash はサムネイルのバイト列から出すので全件の値が変わる。連写のペア距離は
全枚数が同じ向きに回るので相対関係は保たれるが、学習済みの閾値は微妙にずれうる。
星・プロジェクト・セッションは壊れない。

### ブラウザ側（iPad）

`utils/analyzePhoto.ts` の `createImageBitmap(file)` に
`{ imageOrientation: 'from-image' }` を明示した。既定値は仕様の改訂で `none` から
`from-image` に変わっており、端末によってどちらが効くか分からないため。
**既存の IndexedDB サムネイルは作り直されない**ので、iPad 側は取り込み直しが要る。

### 検証

Rust 51件パス（既存 49 + 2）。**4つのミューテーションがすべて捕まった。**

| 入れた変更 | 落ちたテスト |
|---|---|
| Orientation 6 と 8 を入れ替え | `every_exif_orientation_maps_to_its_own_transform`（「Orientation 6 の変換が違う」） |
| IFD1 の優先をやめて IFD0 だけ見る | `the_thumbnail_source_comes_back_upright`（「IFD1 の指定を無視して二重に回している」） |
| `usable` から版の一致を外す | `reuses_the_cached_thumbnail_until_the_photo_changes`（「生成方式が変わったのに古いサムネイルを使い回している」） |
| 1/8 デコード経路の焼き込みを外す | `the_thumbnail_source_comes_back_upright`（「横長のまま返っている（75x50）」） |

テスト用の TIFF 生成に `Val::Short` を足し、`Fixture` に `orientation` /
`thumbnail_orientation` を持たせた。IFD の項目はタグ昇順で並べてある（規格の要求）。

---

## 3. Routine 6: 連写の事後扱い

### 現状（調査結果）

- `collapseBursts` がまとめを代表1枚に畳み、残りを `candidates` から外す。
- `confirmChoices` は表示中のグループしか DB に書かないので、**外された仲間は元の星の
  まま据え置き**。代表だけが★+1 される。
- 接点は選別中の「連写 N 枚」ボタンで代表を差し替えることだけ。事後の画面は無い。
- 結果一覧では「連写で出番が無かった★0」と「見て落とした★0」が区別できない。

### 直した内容

利用者の指定は「**代表と同じ星にそろえる**＋**連写専用の選別画面**」。

**(a) 星をそろえる** — `spreadBurstRatings(session, chosen)` を `utils/tournament.ts` に追加。
代表が通ったら `burstMembers` の仲間に**代表と同じ値**を配る。`+1` ではないのは、
代表が★5で確定されたときに追いつかないため。

- **survivors には入れない。** 入れると同じラウンドの次の回にまとめの全員が出てきて、
  畳んだ意味が消える。星が揃うので、次に「その星を選別」したときに自然に一緒に出てくる。
- 履歴（`history.chosen`）には仲間も入れる。`undoLastStep` は survivors に居ない id を
  読み飛ばすので、星だけが戻る。

**(b) 見直し画面** — `view === 'burst-review'`。結果画面の「連写を見直す」から入る。

- **まとめは保存していない。** `getBurstGroups(projectId)` が学習済みの閾値から
  そのつど引き直すので、セッションが終わっても何度でも戻ってこられる。永続化は不要だった。
- 読むのは**常に1グループぶんだけ**（`getPhotosByIds`）。連写が数百グループあっても
  フロントに載る枚数は変わらない。
- `reviewBurstRatings(photos, keptIds, maxRating)` … **残した写真は+1、外した写真は−1。**
  通常の選別（外しても下げない）とは意図的に変えてある。ここは星をそろえたあとの
  絞り込みなので、下げられないとまとめの全員が高い星のまま残る。
- 「変更しない」で次のまとめへ送れる。最後まで来たら結果画面に戻る。
- キーボード操作は選別画面と同じ（数字で選ぶ／Ctrl+数字で拡大／Enter で確定）。
  **セッションが無くても開ける画面**なので、キー処理は `session` 判定より手前に置いた。

### 途中で作り込んで直したバグ

`undoChoice` が DB へ書き戻すのは**表示中のグループだけ**だった。まとめの仲間は
そのグループに居ないので、画面の星だけ戻って **DB には上がったままの星が残る**。
`undoLastStep` が履歴を取り出す前に対象を控え、グループと合わせて書き戻すように直した。

### 検証

vitest 159件パス（既存 147 + 12）。**2つのミューテーションが捕まった。**

| 入れた変更 | 落ちたテスト |
|---|---|
| 仲間の星を「代表と同じ値」ではなく `+1` にする | `spreadBurstRatings` の2件（★5確定に追随できない） |
| 外した写真を下げない | `reviewBurstRatings` の3件 |

`spreadBurstRatings` → `undoLastStep` の合成も、app.vue と同じ順序で確かめてある。

---

## 4. 環境

- **Smart App Control のブロックは解消していた。** 前セッションで `pnpm dev` / `build` /
  `tauri:dev` を止めていた `An Application Control policy has blocked this file` が再現しない。
- pnpm は corepack のシム（`C:\nvm4w\nodejs`、PATH 済み）で **10.12.1**。
  `package.json` の `packageManager` 指定と一致し、CI とも揃っている。
- ワークツリーの `.claude/launch.json` の pnpm パスを、codex ランタイムのキャッシュ配下から
  素の `pnpm` に変えた。

## 5. 残っている確認（実アプリ）

自動テストで届かないのはここだけ。**利用者の実データでの目視**が要る。

1. **向き** — `L:\Pharfaite Showcase4` を開き、プロジェクト詳細と選別結果の一覧で
   写真が正立していること。初回はサムネイルの作り直しが走る（114枚なら数秒）。
   縦位置が横倒しにならず、かつ横位置が縦にならないこと（二重回転の確認）。
2. **明るさ** — 選別画面で、連写のまとまりが**ホバーせずに**正しい明るさで見えること。
   ホバーで枠の外側に黄緑の outline が出ること。
3. **連写の見直し** — レーティング画面の「連写を見直す」から入り、残す写真を選ぶと
   その写真が★+1、外した写真が★−1 になること。

## 6. git の状態（未コミット）

**コミットの指示は出ていないので、一切の git 操作をしていない。**

| ファイル | 由来 |
|---|---|
| `assets/main.css` | Routine 4 |
| `src-tauri/src/lib.rs` | Routine 5 |
| `src-tauri/src/bin/bench.rs` | Routine 5（SELECT の列を揃えた） |
| `utils/analyzePhoto.ts` | Routine 5（ブラウザ側） |
| `utils/tournament.ts` | Routine 6 |
| `tests/tournament.test.ts` | Routine 6 |
| `app.vue` | Routine 6 ＋ 前セッションの Shortcuts 連携削除 |
| `utils/shareExport.ts` | 前セッションの Shortcuts 連携削除 |
| `tests/shareExport.test.ts` | 同上 |
| `docs/progress/routine4.md` | この文書 |

前セッションから持ち越していた Shortcuts 削除ぶんの検証は、今回まとめて通した
（`pnpm typecheck` / `pnpm test` / `pnpm build` / `cargo test --lib --release`）。
