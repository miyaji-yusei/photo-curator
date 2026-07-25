# Routine 2 / Step 4（デコード刷新）・Step 5（候補爆発の抑制）

実施日: 2026-07-23。**git 操作は一切していない。**変更はすべて未 stage で積み上がっている。
写真原本（`C:\Users\miyaj\Pictures\20260630_新宿エール`）は読み取りのみ。
本番 DB `photo-curator.sqlite3` の最終更新は **00:40:37 のまま**（Routine 1 終了時点と同一）。

前提確認: `step0.md` / `session0.md` / `routine1.md` を読み、Step 0/1/2/3/7 が完了済みであることを
確認したうえで着手した。**方針は推測ではなく Routine 1 の実測値（100枚のデコード方式比較）に基づく。**

---

## 1. 結論（271枚・release・シングルスレッド）

| 指標 | Routine 1（フルデコード） | Routine 2 | 改善 |
|---|---:|---:|---:|
| **初回解析の合計** | **40,070 ms** | **496 ms** | **80.8x** |
| デコード+hash フェーズ | 39,738 ms | 92.4 ms | 430x |
| デコード+hash（ms/枚） | 151.20 | **1.66** | **90.9x** |
| 時間近接候補 | 267 / 271 (98.5%) | 54 / 271 (19.9%) | — |
| peak working set | 83.4 MB | **10.0 MB** | 8.3x |
| 2回目（キャッシュあり） | 31 ms | 39 ms | ほぼ同じ |

**目標「フルデコード版の 1/10 以下」に対して実測 1/81。**
候補の自動縮小を無効にして時間窓 4000ms のまま全 267 枚を処理しても **690 ms**（58x）なので、
高速化の 98% はデコード刷新（Step 4）そのものによるもので、候補を減らしたこと（Step 5）ではない。

2回目が 31→39 ms と微増したのは、scan の upsert に列が5つ増えたぶんの書き込みコスト。
実害のない範囲と判断した（scan DB は 23 ms、全体でも 39 ms）。

---

## 2. Step 4: 採用したデコード方式とその根拠

### 3段構え

`decode_hash_source()` が、安い順に試して最初に成功したものを使う。

1. **EXIF 埋め込みサムネイル**（`exif_thumbnail_image`）— APP1 の IFD1 が指す JPEG を取り出す。
   本体の画素には一切触れない。長辺 96px 未満のものは使わず次へ落とす。
2. **JPEG scaled decode**（`scaled_jpeg_decode`）— `jpeg-decoder` の IDCT スケーリングで 1/8 デコード。
3. **フルデコード**（`image::open`）— PNG / WebP / サムネイル非搭載の壊れかけ JPEG 用。

### 根拠（Routine 1 の実測・実画像100枚）

| 方式 | ms/枚 | フル版とのペア判定の反転 | 採否 |
|---|---:|---:|---|
| ① フルデコード + `resize_exact` | 132.27 | ―（基準） | 最終手段 |
| ② **EXIF サムネイル** | **1.72** | **4/99 (4.0%)** | **主経路** |
| ③ JPEG scaled decode (1/8) | 39.80 | 4/99 (4.0%) | フォールバック |
| ④ フルデコード + `thumbnail_exact` | 71.37 | 13/99 (13.1%) | **却下** |

②を主経路に置いた理由は「速いから」だけではない。**③と判定の壊れ方が同じ 4/99 だった**こと。
④は 1.9 倍しか速くならないのに反転が 3 倍以上悪化するため採らなかった。

Routine 1 が指摘した「hash 側（resize）がデコード本体より重い（58.1% 対 41.1%）」という点も、
②③は縮小前の画素数がそもそも小さいので、デコードと resize の両方が同時に消える。

### 実データでの経路内訳（271枚）

```
exif_thumbnail : 264 / 271  (97.4%)
jpeg_scaled    :   7 / 271  ( 2.6%)
full_decode    :   0 / 271  ( 0.0%)
```

Routine 1 の「100枚で 100% サムネイル取得成功」は 271 枚でも 97.4%。
残り 7 枚は scaled decode に落ちて処理できており、**フルデコードは一度も走らなかった**。

### サムネイルキャッシュ

- 置き場: `app_data_dir()/thumbnails/<photo_id>.jpg`（長辺 256px・JPEG 品質 82）
- 生成は1枚につき一度だけ。**dHash も UI 表示も同じ1枚を使い回す。**
- 無効化は Session 0 で修復した fingerprint（mtime / size）の一致で判定する。
  `thumbnail_mtime` / `thumbnail_size` に「作った時点のソースの fingerprint」を控え、
  現在の値と食い違ったら作り直す。fingerprint が読めない写真はキャッシュを信用しない。
- 実測サイズ: 267枚で **1.8 MB**（約 6.9 KB/枚）。5000枚でも約 34 MB。

**dHash は必ず「保存するサムネイルのバイト列」から計算する。** JPEG 符号化の往復を挟んだあとの
画素を使うので、生成直後とキャッシュヒット時で必ず同じ値になる。ここを分けると、
キャッシュに当たったかどうかでハッシュが変わるという最悪の非決定性が入る。

### ハッシュ方式の移行

Routine 1 の申し送りどおり、`d_hash_version` 列を追加した。fingerprint は
「ファイルが変わっていない」ことしか見ておらず**アルゴリズムの変更を検知できない**ため、
これが無いと旧方式と新方式のハッシュが混ざって連写判定が壊れる。

版が合わない `d_hash` はキャッシュとして使わない。**値は消さない**（一括 NULL 化はしていない）。
サムネイルが有効なら原本には戻らず、サムネイルから引き直すだけで済む。

---

## 3. Step 5: 候補爆発の抑制

### `timestamp_source` 列

`captured_at` をどの経路で得たかを記録する。`read_capture_time()` が根拠の強い順に探す:

`exif_original` → `exif_datetime` → `filename_inferred` → `filesystem_mtime` → `unknown`

`filename_inferred` は今回追加した経路。`2026-06-30_18-19-32` / `20260630_181932` /
`IMG_20260630_181932` などを暦として検証したうえで採る。書き出しや転送で EXIF が落ちても
ファイル名は残ることが多く、mtime よりはるかに撮影時刻に近い。

実データ 271枚は **271/271 が `exif_original`**（Routine 1 の「271/271 が EXIF 由来」と一致）。

### mtime 同士を連写の根拠にしない

mtime は撮影時刻ではない。コピー・ダウンロード・展開で大量のファイルが同一 mtime を持つ。
**両側とも弱い根拠（`filesystem_mtime` / `unknown`）のペアは、時間が近いというだけでは候補にしない。**
片側が EXIF なら根拠として成立するので候補に残す。

合成データ（EXIF 無し・ファイル名に時刻無し = mtime 100%）での効果:

| | Routine 1 | Routine 2 |
|---|---:|---:|
| 5000枚の候補率 | **100%** (5000枚) | **0%** (0枚) |
| 5000枚の decode+hash | 645.7 ms | **0.0 ms** |
| 5000枚の合計 | 2,051 ms | **1,313 ms** |

除外された弱いペアは 4,999 件。**5000枚が丸ごと候補化する現象は消えた。**

### 候補率 80% 超での時間窓の自動縮小

候補率が `CANDIDATE_RATIO_LIMIT`（80%）を超える間、時間窓を半分ずつ詰める（下限 500ms）。
狭めたときは progress event の `warning` フィールドに理由と結果を載せ、UI のダイアログに出す。

### ⚠ この機能が実データに与える影響（要判断）

**実測で、自動縮小は連写グループの 69% を失わせた。**同じコードで縮小の有無だけを変えて比較した:

| | 時間窓 | 候補 | **burst グループ** | **所属枚数** | 初回合計 |
|---|---:|---:|---:|---:|---:|
| 自動縮小あり（現状） | 500 ms | 54 (19.9%) | **16 件** | **45 枚** | 496 ms |
| 自動縮小なし | 4,000 ms | 267 (98.5%) | **52 件** | **199 枚** | 690 ms |

このプロジェクトは撮影間隔がほぼ全て 1〜4 秒で、Routine 1 が指摘したとおり
「時間が近いものだけハッシュする」最適化が原理的に効かない。そのため 80% ルールは
**壊れたデータではなく、密に撮影された正常なデータに対して発火する。**

得たもの 194 ms、失ったもの 36 グループ。Step 4 で 1 枚あたり 1.66 ms になった今、
**この交換は割に合っていない。**

仕様どおりに実装して出荷しているが、判断は利用者に委ねる。Routine 3 で以下のいずれかを推奨:

- **(a) 発火条件を「弱い根拠の timestamp が候補の多数を占めるとき」に限定する。**
  もともとの目的（mtime fallback による爆発）は満たしつつ、EXIF が揃ったデータには触れない。
  なお mtime 同士の除外だけで合成 5000枚は既に 0% になっており、(a) にしても退行はしない。
- **(b) 発火条件を件数の絶対値（例: 候補 2,000 枚超）にする。**比率ではなく実コストで縛る。
- (c) 現状維持。ただし利用者が「連写がまとまらない」と感じる可能性を受け入れる。

### `chrono_like_timestamp` の置き換え

旧実装は `display_value()` が整形した**表示用文字列**から数字を拾うだけだった。
timezone 非対応、範囲検証なし。差し替えて `exif::DateTime::from_ascii` で生の ASCII を
規格どおりに解釈し、`OffsetTimeOriginal` / `OffsetTime` を適用するようにした。

日数計算も Howard Hinnant の `days_from_civil` に置き換えた。

**旧実装には実バグがあった**（テストで確認済み）: `-(year - 1901) / 100` が 2100 年以外でも
1 を引くため、**1970 年以降のすべての日付が丸1日（86,400,000 ms）手前にずれていた。**
連写判定は差分しか見ないため実害は出ていなかったが、`captured_at` の絶対値は全件誤っていた。
また 13 月・2月30日・25 時のような値をそのまま受け入れていた。

---

## 4. 変更したファイル

### `src-tauri/src/lib.rs`

| 追加・変更 | 内容 |
|---|---|
| 定数追加 | `CANDIDATE_RATIO_LIMIT` / `MIN_BURST_WINDOW_MS` / `THUMBNAIL_MAX_EDGE` / `THUMBNAIL_QUALITY` / `THUMBNAIL_DIR` / `MIN_EXIF_THUMBNAIL_EDGE` / `D_HASH_VERSION` |
| `TimestampSource` | 5経路の enum。`is_weak()` が連写判定での信用度を表す |
| `read_capture_time()` | 旧 `read_captured_at`。経路も返す |
| `exif_capture_time()` / `exif_timestamp_ms()` | 生 ASCII + timezone。範囲検証つき |
| `days_from_civil()` / `civil_timestamp_ms()` / `days_in_month()` | 暦計算の置き換え |
| `filename_capture_time()` ほか | ファイル名からの推定 |
| `DecodeSource` / `decode_hash_source()` | 3段構えのデコード |
| `exif_thumbnail_image()` / `scaled_jpeg_decode()` | ②③の実装（Routine 1 の計測コードを本体へ移設） |
| `scale_for_thumbnail()` / `encode_thumbnail()` / `hash_thumbnail_bytes()` / `d_hash_of()` | サムネイル生成と dHash |
| `CachedAnalysis` / `ThumbnailState` / `AnalysisOutcome` / `analyse_photo()` | サムネイルキャッシュ本体 |
| `CandidateInput` / `CandidateSelection` / `candidates_within()` / `select_burst_candidates()` | 候補の絞り込み |
| `thumbnail_dir()` | `app_data_dir()/thumbnails/` |
| `open_database()` | 冪等 migration に6列追加（`timestamp_source` / `thumbnail_path` / `thumbnail_mtime` / `thumbnail_size` / `thumbnail_source` / `d_hash_version`） |
| `upsert_photo()` | ファイルが変わったとき、新しい6列も一緒に破棄する |
| `ProjectProgress` | `warning: Option<String>` を追加。`progress_with_warning()` |
| `Photo` | `thumbnail_path` を追加。`PHOTO_COLUMNS` で3クエリを共通化 |
| `run_burst_analysis()` metadata phase | `timestamp_source` も記録。旧行（source が NULL）も読み直す |
| `run_burst_analysis()` hashing phase | `analyse_photo()` に委譲。候補選択を `select_burst_candidates()` に置換 |
| `bench_api` | 新しい型と関数を再公開。計測が実物を呼ぶ構造は維持 |

### その他

- `src-tauri/Cargo.toml`: `jpeg-decoder` を optional から通常依存へ（③がアプリ本体の経路になったため）。
  `bench` feature は bin ターゲットの有効化のみに縮小
- `src-tauri/src/bin/bench.rs`: パイプライン計測を `analyse_photo` / `select_burst_candidates`
  経由に差し替え（**アプリと同じ経路を測る**）。timestamp_source / デコード経路 / サムネイル
  ヒット率 / 時間窓を集計に追加。`decode` サブコマンドの基準①はローカルのフルデコードに変更
- `types/photo.ts`: `Photo.thumbnailPath`、`ProjectProgress.warning`
- `composables/useDesktop.ts`: `photoThumbnailUrl(photo)`
- `app.vue`: 一覧グリッドをサムネイル表示に。警告を進捗ダイアログに表示

#### UI でサムネイルを使う範囲について

一覧グリッド（`previewPhotos`）のみサムネイルにした。**トーナメントと連写レビューは原本のまま。**
EXIF サムネイルは 160x120 程度で、写真の良し悪しを判断する画面に出すには粗すぎるため。
「dHash と UI 表示が同じ1枚を使い回す」という要件は一覧側で満たしている。

---

## 5. テスト

`cargo test --lib`: **18 passed**（Routine 1 時点は 6）。追加した12件:

| テスト | 対象 |
|---|---|
| `parses_representative_exif_timestamps` | 代表フォーマット・年始・うるう日・サブ秒 |
| `applies_the_exif_time_zone_offset` | `+09:00` / `-05:00` / 空欄 / 壊れたオフセット |
| `rejects_broken_exif_timestamps` | 13月・2月30日・平年2月29日・0日・25時・60分・範囲外の年・空欄・区切り違い・空文字。うるう秒(60秒)は通す |
| `infers_capture_time_from_common_filename_shapes` | 6形式を採り、`DSC_0001` など5例は採らない |
| `classifies_the_source_of_every_capture_time` | 5経路すべての分類 + DB 往復の文字列化 |
| `an_mtime_flood_does_not_explode_the_candidate_set` | mtime 500枚 → 候補0。unknown 500枚 → 候補0。EXIF 隣接だけ残る |
| `narrows_the_window_when_almost_everything_is_a_candidate` | 候補率100% → 縮小 → 本当に近い2枚だけ |
| `window_narrowing_stops_at_the_floor` | 下限で止まる（無限ループしない） |
| `an_empty_or_single_photo_project_selects_nothing` | 境界 |
| `picks_the_cheapest_decode_path_that_works` | ②→③→① の選択。小さすぎるサムネイルは飛ばす |
| `reuses_the_cached_thumbnail_until_the_photo_changes` | ミス / ヒット / 版違いでの引き直し / ファイル削除 / 写真差し替え / fingerprint 不明 |
| `a_single_unreadable_photo_does_not_stop_the_others` | 途中切れ JPEG・非画像・空ファイル・存在しない・ディレクトリ。読める1枚は処理される |

テストは EXIF を**実際に組み立てて**検証している。APP1 に IFD0 / Exif サブIFD / IFD1 を持つ
TIFF ブロックを構築するフィクスチャを書いたので、②の経路が本当に通ることを確かめられる。
合成 JPEG を置くだけでは②を一度も通らず、テストが素通りする。

### 修正を戻して本当に落ちることを確認した（5件）

| 一時的に戻したもの | 落ちたテスト |
|---|---|
| `exif_timestamp_ms` を旧 `chrono_like_timestamp` 実装に | 4件。差が**ちょうど 86,400,000 ms**で1日ずれバグを実証 |
| 弱い根拠ペアの除外を無効化 | `an_mtime_flood_...`（500枚全部が候補になった） |
| 時間窓の自動縮小を無効化 | `narrows_the_window_...` / `window_narrowing_stops_at_the_floor` |
| キャッシュ有効性から fingerprint 判定を外す | `reuses_the_cached_thumbnail_...`（無効化されずヒットしてしまう） |
| `decode_hash_source` をフルデコードのみに | `picks_the_cheapest_decode_path_...` / `reuses_the_cached_thumbnail_...` |

確認後はすべて復元し、18件パスを再確認した。

---

## 6. 検証結果

| コマンド | 結果 |
|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | OK |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **18 passed**, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features bench --lib` | **18 passed**, 0 failed |
| `cargo build --release --features bench --bin bench` | 成功 |
| `pnpm typecheck` | エラーなし（exit 0） |
| `pnpm test` | 1 passed |

### 環境上の注意

`cargo test` を feature 指定なしで連続実行すると、生成済みのテスト用 exe が Windows の
アプリケーション制御ポリシーに引っかかって `os error 4551` で起動できないことがある。
**テストの失敗ではない。**`rm -f src-tauri/target/debug/deps/photo_curator_lib-*.exe` して
実行し直せば通る。`--bin photo-curator` のテストバイナリも同じ理由で起動できないため、
`--lib` を付けて実行している（`--bin` 側にテストは無い）。

---

## 7. Routine 3 への申し送り

### 最優先: 自動縮小の発火条件（§3 の ⚠）

実データで burst グループが 52 → 16 に減っている。**(a) 弱い根拠が多数を占めるときだけ発火、
または (b) 件数の絶対値で縛る**への変更を推奨。判断材料は §3 の表。

### Step 6（並列化）の必要性はほぼ消えた

Routine 1 は「フルデコードのまま 8 並列にすると約 560 MB」と警告していたが、
②を入れたことで peak working set は 83.4 → 10.0 MB。5000枚の外挿も

| 枚数 | Routine 1 予測（現行） | Routine 2 実測ベースの外挿 |
|---|---:|---:|
| 271 | 40.1 秒 | **0.50 秒**（実測） |
| 1,000 | 約 2分27秒 | 約 1.8 秒 |
| 5,000 | 約 12分14秒 | 約 9 秒 |

5000枚 9 秒に 8 倍を掛ける価値は小さい。**Step 6 は優先度を下げてよい。**

### 新しいボトルネックは EXIF 読み取り

初回 496 ms のうち **350 ms（70.6%）が EXIF フェーズ**。ただし絶対値が小さく、
2回目は `captured_at` 再利用で 0 ms。急ぐ必要はない。

### 引き続き未解決

- **`HASH_DISTANCE_LIMIT = 14` の妥当性**（Routine 1 からの持ち越し）。
  99ペア中 29ペア (29%) が閾値の ±4 以内に張り付いており、判定が本質的に不安定。
  デコード方式を変えなくても 4% 程度は揺れる。閾値の再設計か UI 側の逃げ道を検討する価値がある。
  参考: 時間窓 4000ms 揃えでの比較は Routine 1 が 51 グループ / 196 枚、Routine 2 が
  52 グループ / 199 枚。**ハッシュ方式を変えた影響は 1 グループ・3 枚に収まっている。**
- `build_burst_groups()` は `timestamp_source` を見ていない。弱い根拠の除外はハッシュ段階
  だけで効いており、ハッシュ済みの弱い写真同士がグルーピングで隣り合う可能性は残る。
  実害は小さい（弱い写真は強い隣人がいないとハッシュされない）が、筋としては通したい。
- **サムネイルの掃除が無い。**写真を削除しても `thumbnails/` に残る。
  scan 時に `is_missing=1` になった行のサムネイルを消すか、起動時に孤児を掃除する処理が要る。
- `assetProtocol.scope` が `"**"`、CSP が null（Session 0 からの持ち越し）。
  サムネイルは `app_data_dir` 配下なので、scope を絞るなら両方を許可する必要がある。
- **スクリーンショットによる目視確認ができていない。**一覧をサムネイル表示に変えたので、
  ユーザーによる実機確認が必要。
- Step 8（モジュール分割）未着手。`lib.rs` は 1,510 → 約 2,800 行になった。

### 計測ハーネスの再実行方法

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml --features bench --bin bench
./src-tauri/target/release/bench.exe real "C:\Users\miyaj\Pictures\20260630_新宿エール" "docs/progress/bench" "C:\Users\miyaj\AppData\Roaming\app.photocurator.desktop\photo-curator.sqlite3"
./src-tauri/target/release/bench.exe synth "1000,5000" "docs/progress/bench"
```

CSV は `docs/progress/bench/`。`real-per-file.csv` に
`fast_hash_ms` / `decode_source` / `captured_at_source` 列を追加した。
