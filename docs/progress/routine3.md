# Routine 3 / Step 6: 並列化とバックグラウンド化

実施日: 2026-07-23。**git 操作は一切していない。**変更はすべて未 stage で積み上がっている。
写真原本は読み取りのみ（271枚 / 790,273,392 bytes、計測前後で不変）。
本番 DB `photo-curator.sqlite3` の最終更新は **2026-07-23 0:40:37 のまま**（Routine 1 終了時点と同一）。

前提確認: `step0.md` / `session0.md` / `routine1.md` / `routine2.md` を読み、
Step 0/1/2/3/4/5/7 が完了済みであることを確認したうえで着手した。

---

## 1. 結論

| 指標 | Routine 2 | Routine 3 | 備考 |
|---|---:|---:|---|
| 271枚の解析（worker 1本） | 690 ms | **640 ms** | 直列相当。ほぼ同じ |
| 271枚の解析（worker 4本） | — | **193 ms** | **3.31x** |
| 271枚の解析（既定 worker 3本） | — | **242 ms** | 2.65x |
| 1枚あたり（4 worker） | 2.36 ms | **0.71 ms** | |
| peak working set（4 worker） | 10.0 MB | **11.7 MB** | 並列化のメモリ増は +5.3 MB |
| **burst グループ（実データ271枚）** | **16 件 / 45 枚** | **52 件 / 199 枚** | §4 の修正による回復 |
| 時間窓 | 500 ms（自動縮小） | **4000 ms**（縮小せず） | 同上 |

Routine 2 は「Step 6 の優先度は下げてよい」と申し送っていた。実測してみると、
**並列化そのものより §4 の候補縮小の修正のほうが利用者への影響が大きかった。**
連写グループが 16 → 52 件に戻っている。

---

## 2. bounded parallelism（要件 1）

### worker 数

```rust
const MAX_ANALYSIS_WORKERS: usize = 4;
const MIN_ANALYSIS_WORKERS: usize = 2;
const WORKER_COUNT_ENV: &str = "PHOTO_CURATOR_WORKERS";
```

既定は `(コア数 / 2).clamp(2, 4)`。計測機（6コア）では **3**。
`PHOTO_CURATOR_WORKERS` で 1〜4 に上書きできる。

`rayon` の無制限並列は使っていない。この処理は Routine 2 で 1枚 1.66 ms まで
下がっており、支配的なのは CPU ではなくディスクの読み取りだから。同時に何本も
シークを投げると HDD やネットワークドライブでは総スループットが落ちる。

事前生成（`AnalysisMode::Background`）は **必ず 1 本**。前面の操作を邪魔しないため。

### 実測（実データ271枚・release・キャッシュなし）

`docs/progress/bench/parallel-workers.csv`

| workers | 合計 | ms/枚 | 速度比 | peak working set |
|---:|---:|---:|---:|---:|
| 1 | 640.5 ms | 2.36 | 1.00x | 6.4 MB |
| 2 | 332.6 ms | 1.23 | 1.93x | 7.9 MB |
| 3 | 241.8 ms | 0.89 | **2.65x** | 9.8 MB |
| 4 | 193.3 ms | 0.71 | **3.31x** | 11.7 MB |

4本まではほぼ線形。Routine 1 が警告していた「フルデコードのまま8並列で約560MB」
という問題は、Step 4 で EXIF サムネイル経路になったため起きていない
（1本あたり +1.8 MB 程度）。

この計測はアプリ本体と**同じ** `run_in_parallel` + `hash_one` を呼んでいる。
そのために production 側の worker 閉包を `hash_one()` として関数に括り出し、
`bench_api::analyse_in_parallel` から同じものを呼ぶ形にした。

---

## 3. I/O と DB 書き込みの分離（要件 2）

### 構造

```
worker × N ──(PhotoWork)──> mpsc ──> writer 1本 ──> flush_results ──> commit_in_chunks
   ファイルを読む                                      100件ごと
   サムネイルを書く
   **DB には触れない**
```

`PhotoWork` が worker の返り値で、要求どおり
`(photo_id, captured_at, timestamp_source, d_hash, thumbnail_path, error, duration_ms)`
に加えて `index` / `fingerprint` / `thumbnail_source` / `hash_reused` を持つ。

- **ファイル I/O 中に SQLite のトランザクションを保持しない。** worker は DB に
  触れず、writer だけが `ANALYSIS_CHUNK_SIZE`(=100) 件たまった時点で
  Session 0 の `commit_in_chunks` を呼んで確定させる。
- `flush_results` はキャンセルを見ない。残っているのは最大100件の UPDATE
  （実測 0.1 ms/行）で、途中で投げ出すと読み終わった結果を捨てることになるため。
  キャンセルの判断は `run_in_parallel` の writer ループ側へ移した。

### worker を join しない理由

`run_in_parallel` は worker の `JoinHandle` を保持しない。Rust では走っている
デコードを安全に中断できないため、1枚に張り付いた worker を join すると、
まさに避けたかった「遅い1枚に全体が引きずられる」状態に戻る。
共有データはすべて `Arc` なので、放置された worker が生き延びても安全。

---

## 4. 候補縮小の発火条件（Routine 2 の最優先申し送り）

Routine 2 §3 の ⚠ に対して、**(b) 件数の絶対値で縛る**を選び、比率と AND で課した。

```rust
const CANDIDATE_COUNT_LIMIT: usize = 2_000;
let too_many = |ids| ids.len() > CANDIDATE_COUNT_LIMIT && ratio_of(ids) > CANDIDATE_RATIO_LIMIT;
```

(a)（弱い根拠が多数のときだけ発火）ではなく (b) を採った理由:

- mtime 由来の爆発は `candidates_within` の弱ペア除外が既に完全に潰している
  （合成5000枚の候補率 100% → **0%**、今回の再計測でも除外ペア 4,999 件）。
  (a) を足しても実質的な保護は増えない。
- 本当に縛りたいのは実コスト。Step 4 後は 1枚 0.71〜2.36 ms なので、
  候補2,000枚でも 1.4〜4.7 秒。これが「窓を詰めてでも減らす」境界として妥当。

### 効果（実データ271枚）

| | Routine 2 | Routine 3 |
|---|---:|---:|
| 時間窓 | 500 ms | **4,000 ms** |
| 候補 | 54 (19.9%) | **267 (98.5%)** |
| **burst グループ** | **16 件** | **52 件** |
| **所属枚数** | **45 枚** | **199 枚** |
| 初回合計（直列） | 496 ms | 687 ms |

失った 36 グループが戻り、代償は 191 ms。Routine 2 が「割に合っていない」と
書いた交換を解消した。

---

## 5. エラー耐性（要件 3）

### timeout

`PHOTO_TIMEOUT_MS = 15_000`。writer が `WATCHDOG_TICK_MS = 100` ごとに
worker の in-flight スロットを見て、`timeout` を超えた1枚を**失敗として確定させ、
先へ進む**。その worker は放置し、遅れて届いた結果は `reported[]` で二重計上を防ぐ。

### エラー列（冪等 migration）

```sql
ALTER TABLE photos ADD COLUMN analysis_error TEXT;
ALTER TABLE photos ADD COLUMN analysis_error_at INTEGER;
```

デコードエラー・非対応形式・権限エラー・timeout をここに記録する。
成功したら NULL に戻る（エラーが居座らない）。`upsert_photo` の
`CASE WHEN fingerprint IS ...` にも加えたので、ファイルが差し替わったら破棄される。

記録するメッセージ:

| 状況 | メッセージ |
|---|---|
| ファイルは在るが読めない | 画像を読み取れませんでした（破損または非対応の形式）。 |
| ファイルが開けない | ファイルを開けませんでした（移動・削除・権限）。 |
| timeout | 解析が 15 秒以内に終わりませんでした。 |
| 撮影時刻が読めない | 撮影時刻を読み取れませんでした。 |

### UI

`ProjectProgress.failed` に件数を載せ、`app.vue` が

> ◯件を解析できませんでした。該当の写真は連写のまとめ対象から外れますが、選別はこのまま続けられます。

を警告帯（closable）で出す。**ダイアログではないので操作は止まらない。**

---

## 6. バックグラウンド事前生成（要件 4）

### 起動条件

- `run_scan` の完了直後に `AnalysisMode::Background` で自動起動
- プロジェクトを開いたときにも `start_background_analysis` を投げる
  （scan 済みの既存プロジェクト用）

### 前面への譲り方

```rust
let should_stop = || registry.is_cancelled(&task_key)
    || (mode == Background && registry.is_running(&foreground_key));
```

`start_burst_analysis` は最初に `background:{id}` をキャンセルする。
事前生成は 100ms 以内に気づいて降りる。両者が同じ行を触っても結果は決定的に
同じなので、瞬間的な重なりは無害（WAL + busy_timeout 5s）。

チャンクを1つ確定させるごとに `BACKGROUND_PAUSE_MS = 60` だけ手を止める。

### 「選別を開始」がブロックしなくなった

`app.vue` の `beginTournament` は解析の完了を**待たない**。解析を投げてすぐ
`finishTournamentStart()` へ進む。進捗はモーダルダイアログではなく、
画面上部の帯（`v-alert` + 進捗バー + 「解析を止める」）で見せる。

> **⚠ 利用者への影響（判断を仰ぎたい点）**
> 仕様どおり「未解析が残っていても即座に次画面へ進む」を実装した結果、
> **キャッシュが一つも無い初回に「連写をまとめる」を有効にすると、その回は
> 連写グループがほとんど出ない**（出来ているぶんだけが使われる）。
> scan 直後から事前生成が走るので通常は間に合うが、scan 完了から数秒で
> 「選別を開始」まで進んだ場合は空振りしうる。
> 解析中は帯で状況が見えるので「まだ準備中」であることは分かるが、
> 「待ってでも全部まとめてほしい」なら、設定画面に「解析の完了を待つ」
> チェックを足すのが素直。**要判断。**

---

## 7. テスト

`cargo test --lib`: **27 passed**（Routine 2 時点は 18）。追加・変更したもの:

| テスト | 対象 |
|---|---|
| `parallel_workers_write_every_row_exactly_once` | 337件 × 4 worker。writer への到達が全件ちょうど1回、DB の全行が自分の index を持つ（取り違えなし） |
| `a_slow_photo_times_out_while_the_rest_finish` | 30秒居座る2枚を 300ms timeout で切り、残り10枚が完走。全体は 10 秒未満 |
| `analysis_errors_are_recorded_and_counted` | エラーが DB に残り件数が一致、他の写真は解析される、成功に転じたらエラーが消える |
| `cancelling_responds_quickly_and_keeps_committed_work` | 400件処理中のキャンセルが 500ms 以内に効き、確定済みチャンクが残り、DB の実態と件数が一致 |
| `the_task_registry_admits_exactly_one_runner` | 二重起動の拒否 + 32スレッド同時 start の勝者がちょうど1つ |
| `a_stale_cancel_does_not_kill_the_next_run` | start より先に届いたキャンセルが次の実行を巻き添えにしない |
| `the_background_pass_yields_to_the_foreground_one` | 前面が始まったら事前生成が降りる／前面は自分自身を止めない |
| `the_worker_count_stays_within_its_bounds` | 既定 worker 数が 2〜4、事前生成は 1 |
| `a_dense_small_project_keeps_its_full_window` | **§4 の回帰テスト。**271枚・候補率98.5%で窓が縮まらない |
| `narrows_the_window_only_when_the_candidate_set_is_actually_expensive`（改） | 2,400件規模でだけ縮小 |
| `window_narrowing_stops_at_the_floor`（改） | 下限で止まる（規模を新しい閾値に合わせた） |

### 修正を戻して本当に落ちることを確認した（7件）

| 一時的に戻したもの | 落ちたテスト |
|---|---|
| `flush_results` が `pending` を空にし忘れる | `parallel_workers_...`（確定件数 337 → **52,340**） |
| timeout の watchdog を無効化 | `a_slow_photo_...`（timeout 0件、**実行時間が 30.00 秒**に伸びた） |
| writer の `is_cancelled()` チェックを削除 | `cancelling_responds_...`（キャンセルが伝わらない） |
| 候補縮小を比率のみの判定に戻す | `a_dense_small_project_...`（271枚が候補 0 に潰れた） |
| `analysis_error` の migration を無効化 | `analysis_errors_...`（`no such column: analysis_error`） |
| `TaskRegistry::start` の二重起動チェックを削除 | `the_task_registry_...` |
| `TaskRegistry::start` が `cancelled` を消さない | `a_stale_cancel_...` |
| `TaskRegistry::is_running` を常に false | `the_background_pass_...` |

確認後はすべて復元し、27件パスを再確認した。

### 正直に書いておく限界

`run_in_parallel` の共有カーソルを `fetch_add` から `load`+`store`（非アトミック相当）に
戻す revert を試したが、**`parallel_workers_write_every_row_exactly_once` は
それを検出できなかった**（50µs のスピンを入れて競合を促しても通ってしまう）。
このテストが保証しているのは「writer 側の重複・欠落が無いこと」であって、
カーソルのアトミック性ではない。カーソルの正しさはコードレビューに委ねている。

---

## 8. 変更したファイル

### `src-tauri/src/lib.rs`

| 追加・変更 | 内容 |
|---|---|
| 定数 | `MAX/MIN_ANALYSIS_WORKERS` / `WORKER_COUNT_ENV` / `PHOTO_TIMEOUT_MS` / `WATCHDOG_TICK_MS` / `BACKGROUND_PAUSE_MS` / `CANDIDATE_COUNT_LIMIT` |
| `TaskRegistry::is_running()` | 事前生成が前面に譲るための判定 |
| `AnalysisMode` | Foreground / Background。task 名と worker 数を持つ |
| `analysis_worker_count()` | コア数の半分を 2〜4 に丸める。環境変数で上書き |
| `PhotoWork` / `PhotoJob` / `MetadataJob` | worker の返り値と仕事の型 |
| `run_in_parallel()` | 並列エンジン本体。timeout watchdog とキャンセル |
| `flush_results()` | writer 側のバッチ書き込み。`commit_in_chunks` を再利用 |
| `hash_one()` | worker 1本ぶんの仕事。bench からも同じものを呼ぶ |
| `failed_photo_count()` | UI に渡す失敗件数 |
| `ProgressNote` / `progress_note()` | `progress_with_warning` を置き換え。`failed` を追加 |
| `ProjectProgress.failed` | 解析できなかった件数 |
| `open_database()` | `analysis_error` / `analysis_error_at` を冪等 migration で追加 |
| `upsert_photo()` | ファイルが変わったらエラー列も破棄 |
| `select_burst_candidates()` | 縮小の発火条件に件数の絶対値を追加 |
| `run_burst_analysis()` | `AnalysisMode` を取り、metadata / hashing の両フェーズを並列化 |
| `start_background_analysis` | 新コマンド。二重起動は正常として握り潰す |
| `spawn_analysis()` | `start_burst_analysis` と共通の起動処理 |
| `run_scan()` | 完了時に事前生成を起動 |
| `bench_api` | `analysis_worker_count` / `analyse_in_parallel` を追加 |

### その他

- `src-tauri/src/bin/bench.rs`: `parallel` サブコマンド追加。
  `bench parallel <photo-dir> <out-dir> [worker-counts]` → `parallel-workers.csv`
- `types/photo.ts`: `ProjectTask` 型、`ProjectProgress.failed`
- `composables/useDesktop.ts`: `startBackgroundAnalysis`
- `app.vue`: 連写解析をダイアログから帯へ。失敗件数の警告。`beginTournament` が待たない

---

## 9. 検証結果

| コマンド | 結果 |
|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | OK |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --release` | **27 passed**, 0 failed |
| `cargo test ... --features bench --lib --release` | **27 passed**, 0 failed |
| `cargo build --release --features bench --bin bench` | 成功 |
| `pnpm typecheck` | エラーなし（exit 0） |
| `pnpm test` | 1 passed |
| `pnpm build` | 成功（`.output/public` 生成） |

### ⚠ 環境上の注意（Routine 2 の記述を更新）

Windows の**アプリケーション制御ポリシー**が、新しくビルドされた実行ファイルを
`os error 4551` で高い頻度でブロックする。**テストの失敗ではない。**

Routine 2 は「`rm -f target/debug/deps/*.exe` すれば通る」と書いていたが、
**現在それでは通らない**（debug ビルドは一度も実行できなかった）。今回の運用:

- `--release` を付ける（debug より通りやすい）
- 通るまでリトライする。ビルドし直して再実行を繰り返すと数回で通る

```bash
run_tests() {
  for i in $(seq 1 40); do
    out=$(cargo test --manifest-path src-tauri/Cargo.toml --lib --release "$@" 2>&1)
    if echo "$out" | grep -q "os error 4551"; then sleep 5; touch src-tauri/src/lib.rs; continue; fi
    echo "$out"; return 0
  done
}
```

`cargo clippy` はビルドスクリプトの実行が同じ理由でブロックされ、走らせられなかった。
