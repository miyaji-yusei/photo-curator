# Photo Curator 改修 最終レポート

2026-07-23。Session 0 / Routine 1〜3 の全体まとめ。

- **git 操作は一度も行っていない。**全変更が未コミットのまま積み上がっている（§6）
- 写真原本（`C:\Users\miyaj\Pictures\20260630_新宿エール`）は**読み取りのみ**。
  271枚 / 790,273,392 bytes が作業開始時と同一
- 本番 DB `photo-curator.sqlite3` の最終更新は **2026-07-23 0:40:37** のまま。
  migration の検証は常に一時ディレクトリ上のコピーに対して行った
- DB バックアップ: `photo-curator.backup-20260723.sqlite3`（復元手順は `step0.md`）

各回の詳細は `step0.md` / `session0.md` / `routine1.md` / `routine2.md` / `routine3.md`。

---

## 1. 完了した内容

| Step | 内容 | 状態 |
|---|---|---|
| 0 | DB バックアップと保全記録 | **完了**（Session 0） |
| 1 | チャンク commit 化。キャンセルしても解析済みが残る | **完了**（Session 0） |
| 2 | fingerprint の backfill と `IS` 比較。再 scan で解析結果を失わない | **完了**（Session 0） |
| 3 | 計測ハーネス `bench.rs` と初期実測 | **完了**（Routine 1） |
| 4 | デコード刷新（EXIF サムネイル → scaled decode → フルデコードの3段構え）とサムネイルキャッシュ | **完了**（Routine 2） |
| 5 | `timestamp_source` の導入と候補爆発の抑制 | **完了**（Routine 2）／発火条件を Routine 3 で修正 |
| 6 | 並列化・I/O と DB の分離・エラー耐性・バックグラウンド事前生成 | **完了**（Routine 3） |
| 7 | UI の余白とホバーの修正（`vuetify/styles` の読み込み漏れ） | **完了**（Session 0）／**目視未確認** |
| 8 | `lib.rs` / `app.vue` のモジュール分割 | **未着手** |

### 完了しなかった内容

- **Step 8（モジュール分割）は未着手。**`lib.rs` は 1,510 → 約 3,900 行になった
- **Step 7 のスクリーンショットによる目視確認ができていない。**Browser ペインが
  使えず、計算済みスタイルの実測値で検証した（§7）
- `HASH_DISTANCE_LIMIT = 14` の妥当性検証（Routine 1 から持ち越し）
- `cargo clippy` が環境要因で実行できなかった（§7）

---

## 2. 性能の改善実測値

実データ 271枚 / 平均 2.92 MB / Windows 11 / ローカル SSD / release ビルド。

### 初回解析（キャッシュなし）

| | Routine 1（初期計測） | Routine 2 | **Routine 3** | 対 Routine 1 |
|---|---:|---:|---:|---:|
| 合計 | **40,070 ms** | 496 ms<sup>※</sup> | **193 ms**（4 worker） | **208x** |
| | | | 242 ms（既定 3 worker） | 166x |
| | | | 640 ms（1 worker） | 63x |
| デコード+hash（ms/枚） | 151.20 | 1.66 | **0.71** | **213x** |
| peak working set | 83.4 MB | 10.0 MB | **11.7 MB** | 7.1x 減 |

<sup>※</sup> Routine 2 の 496 ms は候補が 54 枚に自動縮小された状態の数字。
同じ 267 枚を処理した条件では 690 ms。Routine 3 は 267 枚を処理して 193 ms。

### 2回目以降（キャッシュあり）

| | Routine 1 | Routine 2 | Routine 3 |
|---|---:|---:|---:|
| 271枚 | 31 ms | 39 ms | **60 ms** |
| 5,000枚（合成） | 654 ms | — | **769 ms** |

増えているのは scan の upsert に列が増えたぶんの書き込みコスト（scan DB が
27.9 ms / 529 ms）。デコードは 0 ms で、**キャッシュが効いている限り再解析は
事実上ゼロコスト**という性質は保たれている。

### worker 数ごとのスケーリング（Routine 3 実測）

| workers | 合計 | ms/枚 | 速度比 | peak |
|---:|---:|---:|---:|---:|
| 1 | 640.5 ms | 2.36 | 1.00x | 6.4 MB |
| 2 | 332.6 ms | 1.23 | 1.93x | 7.9 MB |
| 3（既定） | 241.8 ms | 0.89 | 2.65x | 9.8 MB |
| 4（上限） | 193.3 ms | 0.71 | **3.31x** | 11.7 MB |

### 5,000枚での外挿

| | Routine 1 予測 | **Routine 3 実測ベース** |
|---|---:|---:|
| 初回解析 | 約 12分14秒 | **約 3.6 秒**（4 worker・0.71 ms/枚） |
| scan + EXIF（合成実測） | — | 1,378 ms |
| 2回目（キャッシュあり） | 654 ms | 769 ms |
| DB サイズ | 約 9〜11 MB | 7.82 MB |
| peak working set | 約 560 MB（8並列フルデコード） | **約 15 MB** |

### 連写グループ（実データ271枚）

| | Routine 1 | Routine 2 | **Routine 3** |
|---|---:|---:|---:|
| 時間窓 | 4,000 ms | 500 ms（自動縮小） | **4,000 ms** |
| burst グループ | 51 件 / 196 枚 | **16 件 / 45 枚** | **52 件 / 199 枚** |

Routine 2 で自動縮小により失われた 36 グループを Routine 3 で回復した。

---

## 3. 受け入れ基準の達成状況

| # | 基準 | 判定 | 根拠 |
|---|---|---|---|
| 1 | 連写解析がキャッシュ済みならほぼ即時 | **達成** | 271枚 60 ms、5,000枚 769 ms。デコード 0 ms、ヒット率 267/267 (100%) |
| 2 | キャンセル後に再開すると解析済み分がスキップされる | **達成** | `commit_in_chunks` + fingerprint/サムネイルキャッシュ。テスト `cancelling_keeps_the_chunks_that_were_already_committed` / `cancelling_responds_quickly_and_keeps_committed_work` |
| 3 | 再 scan しても `captured_at` / `d_hash` / `rating` / session が失われない | **達成** | `upsert_photo` の `IS` 比較。テスト3件 + 実DBコピーへの migration で271行完全一致 |
| 4 | 5,000枚で「応答なし」にならない | **達成** | 解析は別スレッド、進捗は 100件ごと。合計 1.4〜5 秒。ピークメモリ約 15 MB |
| 5 | トーナメント開始 500ms 程度 | **達成**（要注記） | `get_burst_groups` は 5,000枚・全件候補でも 3.6 ms。**さらに Routine 3 で解析完了を待たなくなったため、開始は解析状況に依存しなくなった**（§4 の注記） |
| 6 | 旧 DB で project 一覧・作成・open が動く | **達成** | 冪等 migration（8列追加）。テスト `migrates_the_legacy_photo_schema_before_creating_indexes` と、実DBコピーへの `open_database()` 適用で271行が完全一致 |
| 7 | 写真原本を変更しない | **達成** | 271枚 / 790,273,392 bytes が不変。全経路が読み取り専用。サムネイルは `app_data_dir/thumbnails/` にのみ書く |
| 8 | キーボード操作（1〜0選択、M複数選択、Enter確定）と中断再開が退行しない | **未検証（コード上は無変更）** | `onKeydown` / `toggleChoice` / `confirmChoices` / `resumeSession` に一切手を入れていない。**実機確認が必要** |
| 9 | Vuetify の余白ユーティリティが効き、カードのホバーで白化しない | **達成（実測）／目視未確認** | `nuxt.config.ts` に `vuetify/styles` を追加。`.ga-4` gap 0 → 16px、`.mb-8` 0 → 32px、`--v-hover-opacity` が解決。**スクリーンショット未取得** |

---

## 4. ⚠ 動作が変わった点（利用者への影響）

Routine 3 の要件「未解析分が残っていても UI は即座に次画面へ進み、解析は
バックグラウンドで継続（ブロックしない）」を仕様どおり実装した結果:

**キャッシュが一つも無い状態で「連写をまとめる」を有効にして選別を開始すると、
その回は連写グループがほとんど出ない。**出来ているぶんだけが使われる。

- scan 完了直後から事前生成が走るので、通常は間に合う
- 解析中は画面上部の帯で進捗が見えるので「まだ準備中」であることは分かる
- 「待ってでも全部まとめたい」場合は、設定画面に「解析の完了を待つ」チェックを
  足すのが素直。**この扱いは要判断。**

---

## 5. 残タスク一覧

### Step 8: モジュール分割（未着手）

- `src-tauri/src/lib.rs` が約 3,900 行。少なくとも
  `db`（migration / upsert / クエリ）/ `capture_time`（EXIF・ファイル名・暦）/
  `decode`（3段構え・サムネイル）/ `burst`（候補選択・グルーピング）/
  `analysis`（並列エンジン・writer）/ `commands`（Tauri コマンド）に分けられる
- `app.vue` が約 400 行の単一ファイル。ビュー（home / project / method / settings /
  burst-review / tournament / result）ごとにコンポーネント化できる
- テストも `lib.rs` 内に約 2,000 行ある。分割時に一緒に動かす必要がある

### 引き継ぎ書にある未完成機能

| 機能 | 状態 |
|---|---|
| accepted バーストの1カード畳み込み | 未実装。`BurstGroup.accepted` を立てるところまで（`answerBurst`）。トーナメントのグルーピングには反映されていない |
| バースト内の最終選択（`stage: 'burst-final'`） | 型に値はあるが到達する経路が無い |
| rating の永続化 | `session.ratings` は `project_states` の JSON に入るだけ。`photos.rating` 列へは書かれていない |
| project の rename / delete | コマンドが無い |
| missing 写真の一覧通知 | `photos.is_missing` は立つが UI に出ない |
| スライドショー選別 | 「準備中」チップのみ |
| HEIC / RAW 対応 | `is_supported` は jpg/jpeg/png/webp のみ |

### 技術的な残課題

- **`HASH_DISTANCE_LIMIT = 14` の妥当性**（Routine 1 から持ち越し）。
  99ペア中 29ペア (29%) が閾値の ±4 以内に張り付き、判定が本質的に不安定。
  デコード方式を変えなくても 4% 程度は揺れる
- **サムネイルの掃除が無い。**写真を削除しても `thumbnails/` に残る
- `build_burst_groups()` が `timestamp_source` を見ていない。弱い根拠の除外は
  ハッシュ段階だけで効いている
- `assetProtocol.scope` が `"**"`、CSP が null。最終版では要見直し
- 並列エンジンの共有カーソルのアトミック性は、テストでは検出できていない
  （`routine3.md` §7 に明記）
- `cargo clippy` が環境要因（アプリケーション制御ポリシー）で実行できていない

---

## 6. ユーザーへの確認事項

### (1) commit するかどうか

**Session 0 から Routine 3 まで、git 操作を一切していない。**
すべての変更が未コミットのまま積み上がっている。

```
git status --short
MM app.vue                            AM composables/useDesktop.ts
MM assets/main.css                    M  nuxt.config.ts / package.json / pnpm-lock.yaml
MM plugins/vuetify.ts                 M  src-tauri/capabilities/default.json
MM src-tauri/Cargo.lock / Cargo.toml  M  src-tauri/tauri.conf.json
MM src-tauri/src/lib.rs               A  src-tauri/icons/*
AM types/photo.ts                     A  utils/tournament.ts
?? docs/  ?? src-tauri/src/bin/  ?? vitest.config.ts
```

- staged（作業開始時点の実装本体）: 15 files, +1,933 / -55
- unstaged（Session 0〜Routine 3 の改修）: 9 files, +3,388 / -255
- untracked: `docs/`（進捗記録と計測 CSV）、`src-tauri/src/bin/bench.rs`、`vitest.config.ts`

**commit の可否と粒度（1つにまとめるか、Step ごとに分けるか）を判断してほしい。**
指示があるまで `git add` / `commit` は行わない。

### (2) 実機での目視確認

**Session 0 の UI 修正（Step 7）はスクリーンショットによる目視確認ができていない。**
Browser ペインが使えず、CSSOM の計算済みスタイル実測で検証したため、
「実際に見てどうか」は未確認。以下を実機で確かめてほしい。

1. **余白**: コンポーネント間の間隔が空いているか（`.ga-4` / `.mb-8` が効く）
2. **ホバー**: カードにマウスを乗せて真っ白にならないか
3. **`color="surface-variant"` の文字色**: ホームの右カード、トーナメント下部の
   シート、アバターが読めるか（コントラスト 1.40:1 → 12.20:1 に修正済み）
4. **トーナメントカードの比率**: 2枚設定で極端な横長クロップにならないか
5. **一覧グリッド**: Routine 2 でサムネイル表示に変えた。粗すぎないか
6. **受け入れ基準 #8**: キーボード操作（1〜0 / M / Enter）と中断再開
7. **Routine 3 の新 UI**: 解析中の帯、「解析を止める」、失敗件数の警告

### (3) §4 の動作変更

「選別を開始」が解析完了を待たなくなった件（§4）。この扱いでよいか、
「解析の完了を待つ」オプションを足すか。
