#[cfg(target_os = "android")]
mod android_photos;
#[cfg(target_os = "android")]
mod android_secret;
#[cfg(target_os = "android")]
mod android_smb;

use exif::{In, Reader, Tag, Value};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use walkdir::WalkDir;

const PROGRESS_EVENT: &str = "project-progress";
const PAGE_SIZE_LIMIT: i64 = 200;
const BURST_WINDOW_MS: i64 = 4_000;
const HASH_DISTANCE_LIMIT: u32 = 14;
// 候補率がこれを超えたら時間窓を自動的に狭める。撮影間隔がほぼ全て窓の内側に
// 収まるフォルダでは「時間が近いものだけハッシュする」最適化が原理的に効かず、
// 全枚数が候補になる。
const CANDIDATE_RATIO_LIMIT: f64 = 0.80;
// 窓を狭める下限。これ以上詰めると連写そのものを取りこぼす。
const MIN_BURST_WINDOW_MS: i64 = 500;
// キャッシュするサムネイルの長辺。dHash も UI 表示もこの1枚を使い回す。
const THUMBNAIL_MAX_EDGE: u32 = 256;
const THUMBNAIL_QUALITY: u8 = 82;
const THUMBNAIL_DIR: &str = "thumbnails";
// 選別画面に出す「表示用」画像の置き場。サムネイル(160x120 相当)では
// 写真の良し悪しを判断できず、かといって原本(6.7MB)を毎回読むと
// Android では 1 ラウンドで 13.4GB 流れる。その中間をここに作る。
const DISPLAY_DIR: &str = "display";
/// 既定の長辺。Galaxy Z Fold 8 の実機計測で 1 グループ 3〜4 枚なら 733px、
/// 9 枚なら 489px あれば足りる。普段使いはこれで過不足ない。
const DISPLAY_EDGE_DEFAULT: u32 = 1024;
/// 「大きな画像で選別する」を on にしたときの長辺。2 枚を並べて見比べる
/// ときに必要な 1238〜1420px（実測）を満たす。
const DISPLAY_EDGE_LARGE: u32 = 1536;
/// 選べる長辺。ここに無い値は既定に丸める。
const DISPLAY_EDGES: [u32; 5] = [768, 1024, 1280, 1536, 1920];
const DISPLAY_QUALITY: u8 = 82;
// これより小さい EXIF サムネイルは dHash にも表示にも使わない。
const MIN_EXIF_THUMBNAIL_EDGE: u32 = 96;
// dHash の算出方式のバージョン。fingerprint は「ファイルが変わっていない」ことしか
// 見ておらず、**アルゴリズムの変更を検知できない**。方式を変えたらこの値を上げる。
// 版が合わない d_hash はキャッシュとして使わず、サムネイルから引き直す。
const D_HASH_VERSION: i64 = 2;
// サムネイルの生成方式のバージョン。**d_hash とは別に持つ必要がある。**
// `analyse_photo` は保存済みのサムネイルが使えると判断したら原本に戻らないため、
// `D_HASH_VERSION` を上げても「古いサムネイルから引き直す」だけで、サムネイルの
// 中身そのものは作り変わらない。生成方式を変えたらこちらを上げる。
// 1 = EXIF Orientation を焼き込む（それ以前は回転を無視して保存していた）。
const THUMBNAIL_VERSION: i64 = 1;
// 解析結果をこの件数ごとに確定させる。処理全体をひとつのトランザクションで
// 囲むと、キャンセル時の rollback で解析済みの分まで消えてしまい、再開しても
// 毎回ゼロからやり直しになる。
const ANALYSIS_CHUNK_SIZE: usize = 100;
// fingerprint が NULL の旧レコードを、1回の接続で埋める上限。大きなプロジェクト
// でも起動が止まらないよう区切り、接続のたびに少しずつ収束させる。
const FINGERPRINT_BACKFILL_LIMIT: usize = 1_000;
// worker 数の上限。写真の解析はディスク I/O 律速で、並列度を上げても線形には
// 伸びない。HDD やネットワークドライブではシークが増えて逆に遅くなるため、
// `rayon` のような「コア数ぶん全開」は使わない。
const MAX_ANALYSIS_WORKERS: usize = 4;
const MIN_ANALYSIS_WORKERS: usize = 2;
// worker 数を上書きする環境変数。実機のディスク特性に合わせて調整できる。
const WORKER_COUNT_ENV: &str = "PHOTO_CURATOR_WORKERS";
// 1枚の処理に許す時間。異常に遅い / 壊れた1枚で全体が停滞しないようにする。
const PHOTO_TIMEOUT_MS: u64 = 15_000;
// writer がキャンセルと timeout を確認する間隔。キャンセルの体感応答はここで決まる。
const WATCHDOG_TICK_MS: u64 = 100;
// バックグラウンド事前生成でチャンクごとに空ける間隔。前面の操作を邪魔しないよう
// 意図的に手を止める。
const BACKGROUND_PAUSE_MS: u64 = 60;
// 候補が多すぎるときの時間窓の自動縮小は、この件数を超えるときだけ働かせる。
// 比率だけで判定していた頃は、密に撮影された正常なデータ（実測 271枚・候補 98.5%）
// でも発火し、連写グループを 52 → 16 に減らしていた。Step 4 で 1 枚 1.66 ms に
// なった今、数百枚のために窓を詰める価値は無い。実コストで縛る。
const CANDIDATE_COUNT_LIMIT: usize = 2_000;

#[derive(Default)]
struct TaskRegistry {
    running: Mutex<HashSet<String>>,
    cancelled: Mutex<HashSet<String>>,
}

impl TaskRegistry {
    fn start(&self, key: &str) -> Result<(), String> {
        let mut running = self
            .running
            .lock()
            .map_err(|_| "Task registry is unavailable")?;
        if !running.insert(key.to_owned()) {
            return Err("This project already has a task in progress.".into());
        }
        self.cancelled
            .lock()
            .map_err(|_| "Task registry is unavailable")?
            .remove(key);
        Ok(())
    }

    fn finish(&self, key: &str) {
        if let Ok(mut running) = self.running.lock() {
            running.remove(key);
        }
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.remove(key);
        }
    }

    fn cancel(&self, key: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(key.to_owned());
        }
    }

    fn is_cancelled(&self, key: &str) -> bool {
        self.cancelled
            .lock()
            .map(|cancelled| cancelled.contains(key))
            .unwrap_or(true)
    }

    fn is_running(&self, key: &str) -> bool {
        self.running
            .lock()
            .map(|running| running.contains(key))
            .unwrap_or(false)
    }
}

/// 解析を誰のために走らせているか。同じ `run_burst_analysis` を、前面の
/// 「選別を開始」からもアイドル時の事前生成からも使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnalysisMode {
    /// 利用者が待っている。worker を既定数まで使う。
    Foreground,
    /// scan 完了後の事前生成。1 worker で、チャンクごとに手を止める。
    Background,
}

impl AnalysisMode {
    /// progress event の `task`。UI はこれを見て、前面のダイアログを出すか
    /// 邪魔にならない帯で知らせるだけにするかを決める。
    fn task(self) -> &'static str {
        match self {
            Self::Foreground => "burst",
            Self::Background => "background",
        }
    }

    fn workers(self) -> usize {
        match self {
            Self::Foreground => analysis_worker_count(),
            Self::Background => 1,
        }
    }
}

/// worker 数。既定は「コア数の半分」を 2〜4 に丸めたもの。
///
/// 全開にしないのは、この処理が CPU ではなくディスク律速だから。Routine 2 で
/// 1枚 1.66 ms まで下がった今、支配的なのは読み取りそのものであり、同時に
/// 何本もシークを投げると HDD やネットワークドライブでは総スループットが落ちる。
fn analysis_worker_count() -> usize {
    if let Some(value) = std::env::var(WORKER_COUNT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        return value.clamp(1, MAX_ANALYSIS_WORKERS);
    }
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    (cores / 2).clamp(MIN_ANALYSIS_WORKERS, MAX_ANALYSIS_WORKERS)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    name: String,
    folder_path: String,
    photo_count: i64,
    status: String,
    created_at: i64,
    updated_at: i64,
    /// 連写まとめの学習済み閾値。未学習なら None。
    burst_threshold: Option<i64>,
    burst_threshold_learned_at: Option<i64>,
    /// 写真の出所。**画面はこれだけを見る。**
    ///
    /// `folder_path` は `smb://192.168.11.8/Share/2021_06_13` のような
    /// 機械の言葉で、そのまま画面に出すと読みにくい。種類とラベルを別に持ち、
    /// 画面には `source_label` を、技術情報には `folder_path` を出す。
    source_kind: String,
    source_label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Photo {
    id: String,
    project_id: String,
    path: String,
    relative_path: String,
    name: String,
    captured_at: Option<i64>,
    d_hash: Option<String>,
    /// 0〜5 の星。**選別状態を表す唯一の値**。0 は未評価。
    /// 「通過」「落選」「確定」といった別の状態は持たない。選別は星を上げる操作で、
    /// 次に何を選別するかも星で決める。同じ星に集まった写真は自然に合流する。
    rating: i64,
    /// 生成済みサムネイルの絶対パス。UI はここがあれば原本ではなくこちらを出す。
    thumbnail_path: Option<String>,
    /// 選別画面に出す表示用画像の絶対パス。**まだ作っていなければ None**。
    /// 画面はここが無いときだけ原本へ落ちる。
    display_path: Option<String>,
    /// 解析できなかった理由。**画面が「空のタイル」の意味を出し分けるのに使う。**
    /// これが無いと、まだ作っていないのか読めなかったのかが区別できない。
    analysis_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoPage {
    photos: Vec<Photo>,
    total: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionSeed {
    id: String,
    rating: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BurstGroup {
    id: String,
    photo_ids: Vec<String>,
    captured_span_ms: i64,
    similarity: i64,
    accepted: Option<bool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectProgress {
    project_id: String,
    task: String,
    phase: String,
    processed: usize,
    total: usize,
    message: String,
    /// 解析を止めるほどではないが利用者に伝えるべきこと。
    /// 例: 候補が多すぎて時間窓を自動的に狭めた。
    warning: Option<String>,
    /// 解析できなかった写真の累計。0 でない限り UI に出す。
    /// **これがあっても解析は続行する。**1枚の失敗で全体を止めない。
    failed: usize,
}

/// 進捗に添える補足。引数を増やし続けないためにまとめてある。
#[derive(Default)]
struct ProgressNote {
    warning: Option<String>,
    failed: usize,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("photo-curator.sqlite3"))
}

fn connection(app: &AppHandle) -> Result<Connection, String> {
    open_database(&db_path(app)?)
}

// サムネイルの置き場。DB と同じ app_data_dir 配下に置くので、
// 利用者のデータをまたいで散らばらない。
fn thumbnail_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(THUMBNAIL_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// 表示用画像の置き場。サムネイルと同じ app_data_dir 配下。
fn display_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(DISPLAY_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn database_error(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}: {error}")
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
    Ok(names.iter().any(|name| name == column))
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if !has_column(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    }
    Ok(())
}

// 旧ビルドで作られた photos 行は fingerprint 列が NULL のまま残っている。
// その状態ではハッシュの再利用判定が常に失敗し、キャッシュが原理的に効かない。
// さらに scan の upsert が fingerprint の一致で captured_at / d_hash の保持を
// 判定するため、NULL のままだと再 scan のたびに解析結果が消える。
// ここでは captured_at と d_hash には一切触れず、現在のファイルから
// fingerprint だけを補う。冪等。
fn backfill_missing_fingerprints(conn: &Connection) -> Result<(), String> {
    let pending: Vec<(String, String)> = {
        let mut statement = conn
            .prepare(
                "SELECT id,path FROM photos
                 WHERE fingerprint_mtime IS NULL OR fingerprint_size IS NULL
                 LIMIT ?1",
            )
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
        let rows = statement
            .query_map(params![FINGERPRINT_BACKFILL_LIMIT as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?
    };
    if pending.is_empty() {
        return Ok(());
    }

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    for (id, path) in &pending {
        // ファイルが見つからない行はそのままにする。次回の scan で
        // is_missing として扱われる。
        if let Some((mtime, size)) = fingerprint(Path::new(path)) {
            transaction
                .execute(
                    "UPDATE photos SET fingerprint_mtime=?1,fingerprint_size=?2 WHERE id=?3",
                    params![mtime, size, id],
                )
                .map_err(|error| {
                    database_error("ローカルデータベースを更新できませんでした", error)
                })?;
        }
    }
    transaction
        .commit()
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    Ok(())
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|error| database_error("ローカルデータベースを開けませんでした", error))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;

    // Keep this bootstrap schema compatible with databases created by earlier
    // builds. Indexes that reference migrated columns must be created only
    // after those columns have been added below.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
           photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS photos (
           id TEXT PRIMARY KEY, project_id TEXT NOT NULL, path TEXT NOT NULL,
           relative_path TEXT NOT NULL, name TEXT NOT NULL, captured_at INTEGER,
           d_hash TEXT, rating INTEGER NOT NULL DEFAULT 0,
           fingerprint_mtime INTEGER, fingerprint_size INTEGER,
           is_missing INTEGER NOT NULL DEFAULT 0,
           UNIQUE(project_id, path)
         );
         CREATE TABLE IF NOT EXISTS project_states (
           project_id TEXT PRIMARY KEY, state_json TEXT NOT NULL, updated_at INTEGER NOT NULL
         );",
    )
    .map_err(|error| database_error("ローカルデータベースを初期化できませんでした", error))?;

    add_column_if_missing(&conn, "photos", "fingerprint_mtime", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "fingerprint_size", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "is_missing", "INTEGER NOT NULL DEFAULT 0")?;
    // captured_at をどの経路で得たか。連写判定の重み付けに使う。
    // 連写まとめの閾値はプロジェクトごとに学習する。固定値
    // HASH_DISTANCE_LIMIT は実データの距離分布の最も密な領域に当たっており、
    // どのフォルダでも同じ値が正しいという前提が成り立たない。
    // 出所の種類とラベル。既存の行は NULL のままで、読むときに
    // folder_path から導く（下の source_of）。移行の書き込みはしない。
    add_column_if_missing(&conn, "projects", "source_kind", "TEXT")?;
    add_column_if_missing(&conn, "projects", "source_label", "TEXT")?;
    add_column_if_missing(&conn, "projects", "burst_threshold", "INTEGER")?;
    add_column_if_missing(&conn, "projects", "burst_threshold_learned_at", "INTEGER")?;

    // 選別結果。以前は session の JSON にしか無く、DB へは一度も書かれていなかった
    // ため、アプリ側で結果を見ることができなかった。
    add_column_if_missing(&conn, "photos", "eliminated_round", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "confirmed", "INTEGER NOT NULL DEFAULT 0")?;

    add_column_if_missing(&conn, "photos", "timestamp_source", "TEXT")?;
    // 使い回すサムネイルの参照と、それを作った時点のソースの fingerprint。
    add_column_if_missing(&conn, "photos", "thumbnail_path", "TEXT")?;
    add_column_if_missing(&conn, "photos", "thumbnail_mtime", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "thumbnail_size", "INTEGER")?;
    // どのデコード経路でサムネイルを作ったか。速い経路がどれだけ効いているかを
    // あとから実データで確かめられるようにしておく。
    add_column_if_missing(&conn, "photos", "thumbnail_source", "TEXT")?;
    // サムネイルの生成方式。旧ビルドが作った行は NULL になり、版が合わないので
    // 原本から作り直される。回転を無視して保存された古いサムネイルは、これで
    // 自動的に置き換わる。
    add_column_if_missing(&conn, "photos", "thumbnail_version", "INTEGER")?;
    // 選別画面に出す表示用画像と、**実際に生成した長辺**。
    // 長辺を持たないと、設定を変えても古い画像が使われ続ける
    // （thumbnail_version と同じ轍）。
    add_column_if_missing(&conn, "photos", "display_path", "TEXT")?;
    add_column_if_missing(&conn, "photos", "display_edge", "INTEGER")?;
    // プロジェクトごとの上書き。NULL なら全体の設定に従う。
    add_column_if_missing(&conn, "projects", "display_edge", "INTEGER")?;
    // d_hash の算出方式。旧ビルドの行は NULL になり、キャッシュとして使われない。
    // 古い方式のハッシュと新しい方式のハッシュが混ざると連写判定が壊れるため、
    // 値を消さずに「使わない」ことで移行する。
    add_column_if_missing(&conn, "photos", "d_hash_version", "INTEGER")?;
    // 解析できなかった理由と、その時刻。デコードエラー・非対応形式・権限エラー・
    // timeout がここに残る。成功したら NULL に戻す。UI はこの列の件数を出して
    // 「◯件を解析できませんでした」と伝え、**解析自体は続行する**。
    add_column_if_missing(&conn, "photos", "analysis_error", "TEXT")?;
    add_column_if_missing(&conn, "photos", "analysis_error_at", "INTEGER")?;

    // 手で直したまとめ。**グループ単位では持てない。** まとめは dHash と閾値から
    // そのつど導出していて実体が無く、閾値が変われば別物になって紐づかないため。
    // 導出の材料である「隣り合うペア」に対する例外として持つ。
    //   split … 閾値では繋がるが、利用者が切った
    //   join  … 閾値では切れるが、利用者が繋いだ
    // グループは「連続するペアがすべて閾値を満たす区間」なので、これだけで
    // 分割・切り離し・結合・全解除のすべてを表せる。
    // アプリ全体の設定。いまは表示用サイズの既定だけだが、キーと値だけの表なので
    // 増えても migration が要らない。
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_settings (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );",
    )
    .map_err(|error| error.to_string())?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS burst_pair_overrides (
           project_id TEXT NOT NULL,
           left_photo_id TEXT NOT NULL,
           right_photo_id TEXT NOT NULL,
           decision TEXT NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY (project_id, left_photo_id, right_photo_id)
         );",
    )
    .map_err(|error| error.to_string())?;

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS photos_project_visible
           ON photos(project_id, is_missing, relative_path);
         CREATE INDEX IF NOT EXISTS photos_project_capture
           ON photos(project_id, is_missing, captured_at);",
    )
    .map_err(|error| database_error("ローカルデータベースの更新を完了できませんでした", error))?;

    backfill_missing_fingerprints(&conn)?;

    Ok(conn)
}

fn emit_progress(app: &AppHandle, progress: ProjectProgress) {
    let _ = app.emit(PROGRESS_EVENT, progress);
}

fn progress(
    app: &AppHandle,
    project_id: &str,
    task: &str,
    phase: &str,
    processed: usize,
    total: usize,
    message: impl Into<String>,
) {
    progress_note(
        app,
        project_id,
        task,
        phase,
        processed,
        total,
        message,
        ProgressNote::default(),
    );
}

#[allow(clippy::too_many_arguments)]
fn progress_note(
    app: &AppHandle,
    project_id: &str,
    task: &str,
    phase: &str,
    processed: usize,
    total: usize,
    message: impl Into<String>,
    note: ProgressNote,
) {
    emit_progress(
        app,
        ProjectProgress {
            project_id: project_id.to_owned(),
            task: task.to_owned(),
            phase: phase.to_owned(),
            processed,
            total,
            message: message.into(),
            warning: note.warning,
            failed: note.failed,
        },
    );
}

// 進捗イベントを何件ごとに送るか。全体が少ないときは1件ごとに送らないと
// 完了までバーが動かず固まったように見え、大量のときは送りすぎると重くなる。
fn progress_interval(total: usize) -> usize {
    (total / 100).clamp(1, 50)
}

// (id, path, captured_at, timestamp_source)
type MetadataRecord = (String, String, Option<i64>, Option<String>);

/// ハッシュ段階が1枚について必要とするものすべて。
#[derive(Clone, Debug)]
struct HashRecord {
    id: String,
    path: String,
    captured_at: i64,
    source: TimestampSource,
    cached: CachedAnalysis,
}

struct ChunkOutcome {
    committed: usize,
    /// 並列化後、本番の呼び出し（`flush_results`）は中断しない書き込みしか
    /// 行わないため常に false。キャンセルの判断は `run_in_parallel` の側へ
    /// 移った。`commit_in_chunks` 自体の契約は変えていないので残してある。
    #[cfg_attr(not(test), allow(dead_code))]
    cancelled: bool,
}

// items を ANALYSIS_CHUNK_SIZE ごとに commit しながら処理する。
// キャンセルされたら処理中のチャンクだけを rollback し、それまでに commit
// 済みの件数を返す。解析全体をひとつのトランザクションで囲んでいた頃は、
// 中断のたびに全件 rollback され、何度やり直しても結果が残らなかった。
fn commit_in_chunks<T>(
    conn: &Connection,
    items: &[T],
    is_cancelled: &dyn Fn() -> bool,
    on_item: &mut dyn FnMut(&Connection, &T, usize) -> Result<(), String>,
) -> Result<ChunkOutcome, String> {
    let mut committed = 0usize;
    for chunk in items.chunks(ANALYSIS_CHUNK_SIZE) {
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let mut cancelled = false;
        for (offset, item) in chunk.iter().enumerate() {
            if is_cancelled() {
                cancelled = true;
                break;
            }
            on_item(&transaction, item, committed + offset)?;
        }
        if cancelled {
            transaction.rollback().map_err(|error| error.to_string())?;
            return Ok(ChunkOutcome {
                committed,
                cancelled: true,
            });
        }
        transaction.commit().map_err(|error| error.to_string())?;
        committed += chunk.len();
    }
    Ok(ChunkOutcome {
        committed,
        cancelled: false,
    })
}

// ---------------------------------------------------------------------------
// 並列解析エンジン
//
// worker は「ファイルを読んで結果を返す」だけに徹し、DB には一切触れない。
// 単一の writer が結果を受け取り、`commit_in_chunks` でまとめて書く。
// **ファイル I/O 中に SQLite のトランザクションを保持しない**のがこの分離の
// 目的で、そうしないと遅い1枚がデータベース全体を握り続ける。
// ---------------------------------------------------------------------------

/// worker が1枚について返すもの。
#[derive(Clone, Debug)]
struct PhotoWork {
    /// 投入した順序。writer が重複と欠落を検出するのに使う。
    index: usize,
    photo_id: String,
    captured_at: Option<i64>,
    timestamp_source: Option<TimestampSource>,
    d_hash: Option<String>,
    thumbnail_path: Option<String>,
    thumbnail_source: Option<&'static str>,
    fingerprint: Option<(i64, i64)>,
    /// DB に書き戻す必要が無かった（ハッシュもサムネイルも据え置き）。
    hash_reused: bool,
    /// デコード失敗・非対応形式・権限エラー・timeout。
    /// **値が入っていても解析は続く。**
    error: Option<String>,
    duration_ms: u64,
}

impl PhotoWork {
    fn new(index: usize, photo_id: &str) -> Self {
        Self {
            index,
            photo_id: photo_id.to_owned(),
            captured_at: None,
            timestamp_source: None,
            d_hash: None,
            thumbnail_path: None,
            thumbnail_source: None,
            fingerprint: None,
            hash_reused: false,
            error: None,
            duration_ms: 0,
        }
    }
}

/// 並列に流せる仕事。timeout した1枚を writer が単独で確定させるために、
/// 仕事そのものから写真の id を取れる必要がある。
trait PhotoJob: Send + Sync + 'static {
    fn photo_id(&self) -> &str;
}

/// 撮影時刻をまだ読んでいない1枚。
#[derive(Clone, Debug)]
struct MetadataJob {
    id: String,
    path: String,
}

impl PhotoJob for MetadataJob {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

impl PhotoJob for HashRecord {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Default)]
struct ParallelOutcome {
    /// writer が確定させた件数（timeout を含む）。
    completed: usize,
    cancelled: bool,
    timed_out: usize,
}

/// `items` を高々 `workers` 本のスレッドで処理し、結果を到着順に `on_result` へ渡す。
///
/// - キャンセルは `WATCHDOG_TICK_MS` ごとに見るので、200ms 程度で反応する。
/// - 1枚が `timeout` を超えたら、その1枚を失敗として確定させて先へ進む。
///   Rust では走っているデコードを安全に中断できないため、張り付いた worker は
///   放置する（`Arc` で共有しているので生き残っても安全）。**残りの worker と
///   writer は止まらない**のがここでの要件。
/// - そのため worker は join しない。join すると、まさに避けたかった
///   「遅い1枚に全体が引きずられる」状態に戻ってしまう。
fn run_in_parallel<T, F>(
    items: Arc<Vec<T>>,
    workers: usize,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
    work: F,
    on_result: &mut dyn FnMut(PhotoWork) -> Result<(), String>,
) -> Result<ParallelOutcome, String>
where
    T: PhotoJob,
    F: Fn(usize, &T) -> PhotoWork + Send + Sync + 'static,
{
    let total = items.len();
    if total == 0 {
        return Ok(ParallelOutcome::default());
    }
    let workers = workers.clamp(1, MAX_ANALYSIS_WORKERS).min(total);

    // worker 側はこのフラグだけを見る。`is_cancelled` は Tauri の State を
    // 借りていて 'static にできないため、writer が毎ティック転記する。
    let stop = Arc::new(AtomicBool::new(false));
    let cursor = Arc::new(AtomicUsize::new(0));
    let in_flight: Arc<Vec<Mutex<Option<(usize, Instant)>>>> =
        Arc::new((0..workers).map(|_| Mutex::new(None)).collect());
    let work = Arc::new(work);
    let (sender, receiver) = mpsc::channel::<PhotoWork>();

    for slot in 0..workers {
        let items = items.clone();
        let cursor = cursor.clone();
        let in_flight = in_flight.clone();
        let work = work.clone();
        let stop = stop.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let index = cursor.fetch_add(1, Ordering::SeqCst);
                let Some(item) = items.get(index) else { break };
                if let Ok(mut cell) = in_flight[slot].lock() {
                    *cell = Some((index, Instant::now()));
                }
                let started = Instant::now();
                let mut result = work(index, item);
                result.duration_ms = started.elapsed().as_millis() as u64;
                if let Ok(mut cell) = in_flight[slot].lock() {
                    *cell = None;
                }
                if sender.send(result).is_err() {
                    break;
                }
            }
        });
    }
    // writer 側の複製を落とす。全 worker が終わると recv が Disconnected になる。
    drop(sender);

    let mut reported = vec![false; total];
    let mut outcome = ParallelOutcome::default();
    let tick = Duration::from_millis(WATCHDOG_TICK_MS);
    while outcome.completed < total {
        if is_cancelled() {
            stop.store(true, Ordering::Relaxed);
            outcome.cancelled = true;
            return Ok(outcome);
        }
        match receiver.recv_timeout(tick) {
            Ok(result) => {
                // timeout として先に確定させた1枚が遅れて届くことがある。
                // 二重に数えない。
                if !reported[result.index] {
                    reported[result.index] = true;
                    outcome.completed += 1;
                    on_result(result)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        for cell in in_flight.iter() {
            let Some((index, started)) = cell.lock().ok().and_then(|cell| *cell) else {
                continue;
            };
            if reported[index] || started.elapsed() < timeout {
                continue;
            }
            reported[index] = true;
            outcome.completed += 1;
            outcome.timed_out += 1;
            let mut result = PhotoWork::new(index, items[index].photo_id());
            result.duration_ms = started.elapsed().as_millis() as u64;
            result.error = Some(format!(
                "解析が {} 秒以内に終わりませんでした。",
                timeout.as_secs()
            ));
            on_result(result)?;
        }
    }
    stop.store(true, Ordering::Relaxed);
    Ok(outcome)
}

/// 溜まった結果をまとめて書く。
///
/// ここでキャンセルを見ないのは意図的。残っているのは最大
/// `ANALYSIS_CHUNK_SIZE` 件の UPDATE だけ（実測 0.1 ms/行）で、
/// 途中で投げ出すと読み終わった結果を捨てることになる。
fn flush_results(
    conn: &Connection,
    pending: &mut Vec<PhotoWork>,
    apply: &dyn Fn(&Connection, &PhotoWork) -> Result<(), String>,
) -> Result<usize, String> {
    if pending.is_empty() {
        return Ok(0);
    }
    let outcome = commit_in_chunks(
        conn,
        pending,
        &|| false,
        &mut |tx: &Connection, item: &PhotoWork, _index: usize| apply(tx, item),
    )?;
    pending.clear();
    Ok(outcome.committed)
}

// ---------------------------------------------------------------------------
// 撮影時刻
// ---------------------------------------------------------------------------

/// `captured_at` をどの経路で得たか。連写判定でどれだけ信用してよいかが変わる。
///
/// 特に `FilesystemMtime` は撮影時刻ではない。コピー・ダウンロード・展開で
/// 大量のファイルが同一 mtime を持つため、これを連写の根拠にすると
/// フォルダ全体が候補になってしまう。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampSource {
    ExifOriginal,
    ExifDateTime,
    FilenameInferred,
    FilesystemMtime,
    Unknown,
}

impl TimestampSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExifOriginal => "exif_original",
            Self::ExifDateTime => "exif_datetime",
            Self::FilenameInferred => "filename_inferred",
            Self::FilesystemMtime => "filesystem_mtime",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("exif_original") => Self::ExifOriginal,
            Some("exif_datetime") => Self::ExifDateTime,
            Some("filename_inferred") => Self::FilenameInferred,
            Some("filesystem_mtime") => Self::FilesystemMtime,
            _ => Self::Unknown,
        }
    }

    /// 連写の根拠としては弱い経路かどうか。
    fn is_weak(self) -> bool {
        matches!(self, Self::FilesystemMtime | Self::Unknown)
    }
}

pub struct CaptureTime {
    at: i64,
    source: TimestampSource,
}

/// 写真の実体をどこから取るか。**解析コードはこれ以外を知らない。**
///
/// 要点は `head` があること。EXIF 埋め込みサムネイル経路は**先頭 26KB 程度で
/// 用が済む**（実データで計測）。ここを `all` に一本化すると、Android から
/// NAS 越しに読むときに 1 枚 6.7MB を落とすことになり、転送量が 260 倍になる。
/// デスクトップでは OS の SMB クライアントが同じことを黙ってやってくれていた。
pub trait PhotoSource {
    /// 先頭 `want` バイト。ファイルがそれより短ければあるだけ返す。
    fn head(&self, want: usize) -> Option<Vec<u8>>;
    /// 全体。EXIF サムネイルが無い写真だけがここへ落ちる。
    fn all(&self) -> Option<Vec<u8>>;
    /// mtime と size。**既存のキャッシュ無効化がそのまま効く。**
    fn fingerprint(&self) -> Option<(i64, i64)>;
    /// ファイル名。EXIF が無いときの撮影時刻の手がかりになる。
    fn name(&self) -> Option<String>;
}

/// EXIF を読むために先に取る量。実データでは APP1 が先頭 26KB で終わるので
/// 64KB あれば 1 往復で足りる。足りなかったときだけ全体を取り直す。
const EXIF_HEAD_PROBE: usize = 64 * 1024;

/// `photos.path` に入っている文字列が、どこの写真を指しているか。
///
/// **DB のスキーマは変えない。** 列は既に `TEXT` で、`content://…` も
/// そのまま入る。前置きだけで見分けられるので、既存の行はすべて
/// ローカルパスとして読まれ、移行処理が要らない。
pub enum PhotoRef {
    Local(PathBuf),
    /// Android の MediaStore。`content://…`
    Content(String),
    /// NAS。`smb://<host>/<share>/<相対パス>`
    Smb(String),
}

impl PhotoRef {
    pub fn parse(raw: &str) -> Self {
        if raw.starts_with("content://") {
            Self::Content(raw.to_owned())
        } else if raw.starts_with(SMB_PREFIX) {
            Self::Smb(raw.to_owned())
        } else {
            Self::Local(PathBuf::from(raw))
        }
    }

    /// その指し先を読む口を返す。**解析コードはこれだけを見る。**
    pub fn source(&self) -> Box<dyn PhotoSource + '_> {
        match self {
            Self::Local(path) => Box::new(LocalPhoto(path)),
            #[cfg(target_os = "android")]
            Self::Content(uri) => Box::new(ContentPhoto(uri)),
            #[cfg(target_os = "android")]
            Self::Smb(url) => Box::new(SmbPhoto(url)),
            // Android 以外では読めない。壊れた行として扱う。
            #[cfg(not(target_os = "android"))]
            Self::Content(_) | Self::Smb(_) => Box::new(MissingPhoto),
        }
    }
}

/// ローカルのファイル。デスクトップはこれだけを使う。
pub struct LocalPhoto<'a>(pub &'a Path);

/// 読めない指し先。**この環境では扱えない**ことを、失敗として素直に返す。
#[cfg(not(target_os = "android"))]
pub struct MissingPhoto;

#[cfg(not(target_os = "android"))]
impl PhotoSource for MissingPhoto {
    fn head(&self, _want: usize) -> Option<Vec<u8>> {
        None
    }
    fn all(&self) -> Option<Vec<u8>> {
        None
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        None
    }
    fn name(&self) -> Option<String> {
        None
    }
}

/// NAS の写真を指す前置き。
pub const SMB_PREFIX: &str = "smb://";

/// Android の MediaStore。Kotlin 経由で読む。
#[cfg(target_os = "android")]
pub struct ContentPhoto<'a>(pub &'a str);

/// NAS（SMB）。Kotlin 経由で読む。**先頭だけ読めることが要点。**
#[cfg(target_os = "android")]
pub struct SmbPhoto<'a>(pub &'a str);

#[cfg(target_os = "android")]
impl PhotoSource for SmbPhoto<'_> {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        crate::android_smb::read_bytes(self.0, 0, want as i32)
    }
    fn all(&self) -> Option<Vec<u8>> {
        crate::android_smb::read_bytes(self.0, 0, 0)
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        crate::android_smb::stat(self.0)
    }
    fn name(&self) -> Option<String> {
        // smb://host/share/a/b.jpg の末尾。撮影時刻の手がかりになる。
        self.0.rsplit('/').next().map(str::to_owned)
    }
}

#[cfg(target_os = "android")]
impl PhotoSource for ContentPhoto<'_> {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        crate::android_photos::read_bytes(self.0, 0, want as i32)
    }
    fn all(&self) -> Option<Vec<u8>> {
        crate::android_photos::read_bytes(self.0, 0, 0)
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        crate::android_photos::stat(self.0)
    }
    fn name(&self) -> Option<String> {
        // content:// の末尾は数字の id で、名前にならない。
        // 撮影時刻の手がかりは EXIF と mtime に任せる。
        None
    }
}

impl PhotoSource for LocalPhoto<'_> {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let file = File::open(self.0).ok()?;
        let mut buffer = Vec::new();
        // `take` で読む量を区切る。**ここを fs::read にすると、EXIF だけ見たい
        // ときにも全体を読んでしまう。**
        BufReader::new(file)
            .take(want as u64)
            .read_to_end(&mut buffer)
            .ok()?;
        Some(buffer)
    }

    fn all(&self) -> Option<Vec<u8>> {
        fs::read(self.0).ok()
    }

    fn fingerprint(&self) -> Option<(i64, i64)> {
        fingerprint(self.0)
    }

    fn name(&self) -> Option<String> {
        self.0.file_name()?.to_str().map(str::to_owned)
    }
}

/// 撮影時刻を、根拠の強い順に探す。
/// EXIF → ファイル名 → mtime。ファイル名を mtime より優先するのは、
/// 書き出しや転送で EXIF が落ちても `20260630_181932` の類は残ることが多く、
/// mtime よりはるかに撮影時刻に近いため。
fn read_capture_time_from(source: &dyn PhotoSource) -> Option<CaptureTime> {
    source
        .head(EXIF_HEAD_PROBE)
        .and_then(|head| exif_capture_time_bytes(&head))
        .or_else(|| {
            let name = source.name()?;
            let stem = Path::new(&name).file_stem()?.to_str()?.to_owned();
            filename_capture_time_of(&stem).map(|at| CaptureTime {
                at,
                source: TimestampSource::FilenameInferred,
            })
        })
        .or_else(|| {
            source.fingerprint().map(|(mtime, _)| CaptureTime {
                at: mtime,
                source: TimestampSource::FilesystemMtime,
            })
        })
}

/// ローカルのファイルから撮影時刻を読む。**テストと計測ハーネス専用。**
///
/// 本番の経路は `PhotoRef` から `PhotoSource` を得て
/// `read_capture_time_from` を呼ぶ。Android では写真が content:// や
/// smb:// を指すので、ここを本番で使うと**全部が「撮影時刻を読み取れません」
/// になる**（実際にそうなっていた）。
///
/// **本番ビルドには存在させない。** これは書き忘れではなく、同じ間違いを
/// 二度としないための仕掛け。うっかり本番から呼ぶとコンパイルが通らない。
#[cfg(any(test, feature = "bench"))]
fn read_capture_time(path: &Path) -> Option<CaptureTime> {
    read_capture_time_from(&LocalPhoto(path))
}

fn ascii_field(exif: &exif::Exif, tag: Tag) -> Option<Vec<u8>> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(values) => values.first().cloned(),
        _ => None,
    }
}

// 以前は `display_value()` が整形した文字列から数字を拾っていた。表示用の
// 文字列に依存していたうえ timezone を無視し、月や日の範囲も検証していなかった
// ため、壊れた EXIF が「それらしい値」に化けるか、失敗して mtime fallback に
// 落ちて候補爆発を誘発していた。ここでは生の ASCII を規格どおりに解釈する。
fn exif_capture_time_bytes(bytes: &[u8]) -> Option<CaptureTime> {
    let exif = Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let offset =
        ascii_field(&exif, Tag::OffsetTimeOriginal).or_else(|| ascii_field(&exif, Tag::OffsetTime));
    for (tag, source) in [
        (Tag::DateTimeOriginal, TimestampSource::ExifOriginal),
        (Tag::DateTime, TimestampSource::ExifDateTime),
    ] {
        let Some(raw) = ascii_field(&exif, tag) else {
            continue;
        };
        if let Some(at) = exif_timestamp_ms(&raw, offset.as_deref()) {
            return Some(CaptureTime { at, source });
        }
    }
    None
}

/// `YYYY:MM:DD HH:MM:SS` と `+09:00` 形式のオフセットを UTC のミリ秒にする。
/// `DateTime::from_ascii` は範囲を検証しない（13月を返しうる）ので、
/// 妥当性の確認は `civil_timestamp_ms` 側で必ず行う。
fn exif_timestamp_ms(datetime: &[u8], offset: Option<&[u8]>) -> Option<i64> {
    let mut parsed = exif::DateTime::from_ascii(datetime).ok()?;
    if let Some(offset) = offset {
        // オフセットが壊れていても日時そのものは使う。
        let _ = parsed.parse_offset(offset);
    }
    let local = civil_timestamp_ms(
        parsed.year as i64,
        parsed.month as i64,
        parsed.day as i64,
        parsed.hour as i64,
        parsed.minute as i64,
        parsed.second as i64,
    )?;
    // オフセットが無い EXIF は「現地時刻だが地域は不明」。連写判定は差分しか
    // 見ないため、UTC とみなしても同一フォルダ内の相対関係は壊れない。
    Some(local - i64::from(parsed.offset.unwrap_or(0)) * 60_000)
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// 暦日から Unix epoch までの日数（Howard Hinnant の days_from_civil）。
/// 以前の自前計算はうるう年の加算が 1〜2月でずれていた。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_timestamp_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<i64> {
    if !(1900..=2999).contains(&year) {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // うるう秒で 60 を書く機材があるため秒だけ 60 を許す。
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
}

fn digit_groups(value: &str) -> Vec<&str> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_fixed(text: &str, widths: &[usize]) -> Option<Vec<i64>> {
    let mut rest = text;
    let mut parts = Vec::with_capacity(widths.len());
    for width in widths {
        if rest.len() < *width {
            return None;
        }
        let (head, tail) = rest.split_at(*width);
        parts.push(head.parse().ok()?);
        rest = tail;
    }
    Some(parts)
}

fn timestamp_from_groups(groups: &[&str]) -> Option<i64> {
    let lengths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
    let parts = match lengths.as_slice() {
        // 20260630181932
        [14, ..] => split_fixed(groups[0], &[4, 2, 2, 2, 2, 2])?,
        // IMG_20260630_181932
        [8, 6, ..] => {
            let mut parts = split_fixed(groups[0], &[4, 2, 2])?;
            parts.extend(split_fixed(groups[1], &[2, 2, 2])?);
            parts
        }
        // 2026-06-30_18-19-32
        [4, 2, 2, 2, 2, 2, ..] => groups[..6]
            .iter()
            .map(|group| group.parse().ok())
            .collect::<Option<Vec<i64>>>()?,
        _ => return None,
    };
    civil_timestamp_ms(parts[0], parts[1], parts[2], parts[3], parts[4], parts[5])
}

/// ファイル名から撮影時刻らしい並びを読む。`2026-06-30_18-19-32` /
/// `20260630_181932` / `IMG_20260630_181932` に対応する。
/// 数字の並びとして成立していても暦として不正なら採らない。
fn filename_capture_time_of(stem: &str) -> Option<i64> {
    let groups = digit_groups(stem);
    (0..groups.len()).find_map(|start| timestamp_from_groups(&groups[start..]))
}

/// パスから拡張子を落として上に渡すだけ。本番の経路は `PhotoSource::name()`
/// から名前を受け取るので、こちらはテストの読みやすさのために残している。
#[cfg(test)]
fn filename_capture_time(path: &Path) -> Option<i64> {
    filename_capture_time_of(path.file_stem()?.to_str()?)
}

// ---------------------------------------------------------------------------
// デコードとサムネイル
// ---------------------------------------------------------------------------

/// dHash のもとになる画素をどこから取ったか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeSource {
    ExifThumbnail,
    JpegScaled,
    FullDecode,
}

impl DecodeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExifThumbnail => "exif_thumbnail",
            Self::JpegScaled => "jpeg_scaled",
            Self::FullDecode => "full_decode",
        }
    }
}

/// 既に読み込んだ EXIF のフィールド列から、指定した IFD の Orientation を取る。
fn orientation_field(fields: &[exif::Field], ifd: In) -> Option<u16> {
    fields
        .iter()
        .find(|field| field.tag == Tag::Orientation && field.ifd_num == ifd)
        .and_then(|field| match &field.value {
            Value::Short(values) => values.first().copied(),
            _ => None,
        })
}

/// 原本の EXIF Orientation。読めなければ 1（無変換）を返す。
///
/// IFD0 は TIFF ブロックの先頭近くにあるので、先頭だけ読めていれば足りる。
fn exif_orientation_bytes(bytes: &[u8]) -> u16 {
    let Ok(exif) = Reader::new().read_from_container(&mut std::io::Cursor::new(bytes)) else {
        return 1;
    };
    match exif.get_field(Tag::Orientation, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Short(values)) => values.first().copied().unwrap_or(1),
        _ => 1,
    }
}

/// EXIF Orientation を画素に焼き込んで、見たままの向きにする。
///
/// **これをしないと一覧だけが横倒しになる。** 一覧はここで作ったサムネイルを
/// 表示し、選別・拡大は原本を `<img>` に渡す。WebView は原本の Orientation を
/// 自動で適用するので、焼き込まないほうだけが回転しない状態になる。
///
/// 値の意味は EXIF 規格のとおり。`rotate90` は時計回り。
fn apply_orientation(image: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        // 1（無変換）と、規格外の値。壊れた EXIF で画像を回さない。
        _ => image,
    }
}

/// EXIF の APP1 に埋め込まれたサムネイル JPEG を取り出してデコードする。
/// IFD1 の JPEGInterchangeFormat（オフセット）と同 Length が実体を指す。
/// 読むのは APP1 セグメントまでで、本体の画素には一切触れない。
///
/// Orientation も一緒に返す。**IFD1 のものを優先する。** 埋め込みサムネイルを
/// 既に正立させて保存するカメラがあり、そこで IFD0 の値を当てると二重に回る。
/// IFD1 に無ければ本体（IFD0）の値に従う。
fn exif_thumbnail_image_bytes(bytes: &[u8]) -> Option<(DynamicImage, u16)> {
    let tiff = exif::get_exif_attr_from_jpeg(&mut std::io::Cursor::new(bytes)).ok()?;
    let (fields, _) = exif::parse_exif(&tiff).ok()?;
    let find = |tag: Tag| -> Option<usize> {
        fields
            .iter()
            .find(|field| field.tag == tag && field.ifd_num == In::THUMBNAIL)
            .and_then(|field| match &field.value {
                Value::Long(values) => values.first().map(|value| *value as usize),
                Value::Short(values) => values.first().map(|value| *value as usize),
                _ => None,
            })
    };
    let offset = find(Tag::JPEGInterchangeFormat)?;
    let length = find(Tag::JPEGInterchangeFormatLength)?;
    if length == 0 {
        return None;
    }
    let end = offset.checked_add(length)?;
    if end > tiff.len() {
        return None;
    }
    let image =
        image::load_from_memory_with_format(&tiff[offset..end], image::ImageFormat::Jpeg).ok()?;
    let orientation = orientation_field(&fields, In::THUMBNAIL)
        .or_else(|| orientation_field(&fields, In::PRIMARY))
        .unwrap_or(1);
    // 極端に小さいサムネイルは dHash も表示も成立しない。次の経路へ落とす。
    (image.width().max(image.height()) >= MIN_EXIF_THUMBNAIL_EDGE)
        .then_some((image, orientation))
}

/// jpeg-decoder の IDCT スケーリングで 1/8 相当まで小さくデコードする。
/// `scale()` は 1/8・1/4・1/2・1/1 のうち要求以上で最小のものを選ぶ。
/// image 0.25 のバックエンド zune-jpeg は 1/8 デコードを提供しないため、
/// この経路のためだけに jpeg-decoder を併用している。
fn scaled_jpeg_decode_bytes(bytes: &[u8]) -> Option<DynamicImage> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    decoder
        .scale(info.width.div_ceil(8), info.height.div_ceil(8))
        .ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}

/// dHash とサムネイルのもとになる画像を、**必要な画素数だけ**取り出す。
///
/// dHash が要るのは 64bit だけなのに、以前は 24Mpx を丸ごと展開したうえで
/// `resize_exact` が全画素を舐めていた。Routine 1 の実測（実画像100枚）:
///
/// | 方式 | ms/枚 | フル版とのペア判定の反転 |
/// |---|---:|---:|
/// | ① フルデコード + resize_exact | 132.27 | ―（基準）|
/// | ② EXIF サムネイル | **1.72** | 4/99 |
/// | ③ JPEG scaled decode (1/8) | 39.80 | 4/99 |
///
/// ②は③より 23 倍速く、判定の壊れ方は同じだったので②を主経路に置く。
/// ③は JPEG 専用なので、PNG/WebP とサムネイル非搭載 JPEG は①に落ちる。
///
/// どの経路を通っても、返す時点で **Orientation は焼き込み済み**。
///
/// **①は先頭 64KB しか読まない。**②③に落ちたときだけ全体を取る。
/// 実データでは 97.4% が①なので、読む量は 1 枚あたり 26KB で収まる。
fn decode_hash_source_from(source: &dyn PhotoSource) -> Option<(DynamicImage, DecodeSource)> {
    let head = source.head(EXIF_HEAD_PROBE)?;
    // 先頭が上限いっぱいなら、APP1 がまだ続いている可能性がある。
    let maybe_truncated = head.len() >= EXIF_HEAD_PROBE;
    let mut full: Option<Vec<u8>> = None;

    if let Some((image, orientation)) = exif_thumbnail_image_bytes(&head) {
        return Some((
            apply_orientation(image, orientation),
            DecodeSource::ExifThumbnail,
        ));
    }
    if maybe_truncated {
        // APP1 が 64KB に収まらないカメラ。全体を読み直して一度だけ試す。
        full = source.all();
        if let Some((image, orientation)) = full.as_deref().and_then(exif_thumbnail_image_bytes) {
            return Some((
                apply_orientation(image, orientation),
                DecodeSource::ExifThumbnail,
            ));
        }
    }

    // ここから先は生の画素なので、本体（IFD0）の Orientation をそのまま当てる。
    // IFD0 は TIFF ブロックの先頭近くなので、先頭だけで読める。
    let orientation = exif_orientation_bytes(&head);
    let bytes = match full {
        Some(bytes) => bytes,
        None => source.all()?,
    };
    if let Some(image) = scaled_jpeg_decode_bytes(&bytes) {
        return Some((
            apply_orientation(image, orientation),
            DecodeSource::JpegScaled,
        ));
    }
    image::load_from_memory(&bytes)
        .ok()
        .map(|image| (apply_orientation(image, orientation), DecodeSource::FullDecode))
}

fn decode_hash_source(path: &Path) -> Option<(DynamicImage, DecodeSource)> {
    decode_hash_source_from(&LocalPhoto(path))
}

/// 選べる長辺に丸める。設定ファイルや古いセッションから変な値が来ても、
/// 生成する画像の大きさが暴れないようにする。
fn normalize_display_edge(edge: i64) -> u32 {
    let edge = edge.clamp(0, u32::MAX as i64) as u32;
    DISPLAY_EDGES
        .into_iter()
        .find(|candidate| *candidate == edge)
        .unwrap_or(DISPLAY_EDGE_DEFAULT)
}

fn display_file(dir: &Path, photo_id: &str) -> PathBuf {
    dir.join(format!("{photo_id}.jpg"))
}

/// 表示用画像を作る。長辺を `edge` に収め、原本より大きくはしない。
fn encode_display(image: &DynamicImage, edge: u32) -> Option<Vec<u8>> {
    let scaled = if image.width().max(image.height()) <= edge {
        image.clone()
    } else {
        image.thumbnail(edge, edge)
    };
    let rgb = scaled.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, DISPLAY_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(bytes)
}

/// 表示用画像を 1 枚ぶん用意する。
///
/// **要点は「下げるときは原本に戻らない」こと。** 既に保存してある表示用画像が
/// 要求より大きければ、それを縮めれば足りる。1536 → 1024 の切り替えで
/// 2,000 枚ぶん 13.4GB を読み直すのは無駄でしかない。
/// 逆に上げるときは、小さい画像から大きい画像は作れないので原本へ戻る。
fn build_display(
    source: &dyn PhotoSource,
    edge: u32,
    existing: Option<(&Path, u32)>,
) -> Option<Vec<u8>> {
    if let Some((path, stored_edge)) = existing {
        if stored_edge >= edge && path.is_file() {
            if let Some(image) = fs::read(path)
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
            {
                return encode_display(&image, edge);
            }
        }
    }
    // 原本から作る。**ここだけが全体を読む。**
    // Orientation は decode_hash_source_from と同じ規則で焼き込む。
    let bytes = source.all()?;
    let orientation = source
        .head(EXIF_HEAD_PROBE)
        .map(|head| exif_orientation_bytes(&head))
        .unwrap_or(1);
    let image = image::load_from_memory(&bytes).ok()?;
    encode_display(&apply_orientation(image, orientation), edge)
}

fn scale_for_thumbnail(image: &DynamicImage) -> DynamicImage {
    if image.width().max(image.height()) <= THUMBNAIL_MAX_EDGE {
        // EXIF サムネイルは 160x120 前後。引き伸ばしても情報は増えない。
        return image.clone();
    }
    image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
}

fn encode_thumbnail(image: &DynamicImage) -> Option<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(bytes)
}

fn d_hash_of(image: &DynamicImage) -> String {
    let small = image.resize_exact(9, 8, FilterType::Triangle).grayscale();
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            if small.get_pixel(x, y).0[0] > small.get_pixel(x + 1, y).0[0] {
                bits |= 1 << (y * 8 + x);
            }
        }
    }
    format!("{bits:016x}")
}

/// dHash は**必ず保存するサムネイルのバイト列から**計算する。
/// 生成直後とキャッシュヒット時で必ず同じ値になることを保証するため、
/// JPEG 符号化の往復を挟んだあとの画素を使う。
fn hash_thumbnail_bytes(bytes: &[u8]) -> Option<String> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .ok()
        .map(|image| d_hash_of(&image))
}

/// キャッシュを使わずに1枚ぶんのハッシュを出す。計測ハーネス用。
/// キャッシュ経路（`analyse_photo`）と同じ値を返す。
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
fn d_hash(path: &Path) -> Option<String> {
    let (image, _) = decode_hash_source(path)?;
    hash_thumbnail_bytes(&encode_thumbnail(&scale_for_thumbnail(&image))?)
}

/// DB に保存済みの解析結果。キャッシュの有効性判定に使う。
#[derive(Default, Clone, Debug)]
pub struct CachedAnalysis {
    pub d_hash: Option<String>,
    pub d_hash_version: Option<i64>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_mtime: Option<i64>,
    pub thumbnail_size: Option<i64>,
    pub thumbnail_version: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailState {
    /// 生成済みのサムネイルをそのまま使えた。
    Hit,
    /// 作り直した。どの経路でデコードしたかを持つ。
    Generated(DecodeSource),
    /// 壊れた画像・非対応形式・権限エラーなど。1枚失敗しても解析は続く。
    Failed,
}

impl ThumbnailState {
    /// 作り直したときだけ、使ったデコード経路の名前を返す。
    /// キャッシュヒットでは経路が分からないので、DB 側の値を据え置く。
    fn decode_source(self) -> Option<&'static str> {
        match self {
            Self::Generated(source) => Some(source.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisOutcome {
    pub d_hash: Option<String>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_state: ThumbnailState,
    /// DB に書き戻す必要すらなかった（ハッシュもサムネイルも据え置き）。
    pub hash_reused: bool,
}

fn thumbnail_file(dir: &Path, photo_id: &str) -> PathBuf {
    dir.join(format!("{photo_id}.jpg"))
}

/// 1枚ぶんのサムネイル生成と dHash。
///
/// サムネイルは一度だけ作って使い回す。無効化は Session 0 で直した
/// fingerprint（mtime/size）で判定する。ファイルが読めず fingerprint が
/// 取れない場合はキャッシュを信用しない（読めないファイル同士が同じ
/// fingerprint に見えて誤ヒットするのを避けるため）。
fn analyse_photo(
    thumbnail_dir: &Path,
    photo_id: &str,
    source: &dyn PhotoSource,
    current: Option<(i64, i64)>,
    cached: &CachedAnalysis,
) -> AnalysisOutcome {
    let file = thumbnail_file(thumbnail_dir, photo_id);
    let stored = file.to_string_lossy().to_string();
    let failed = || AnalysisOutcome {
        d_hash: None,
        thumbnail_path: None,
        thumbnail_state: ThumbnailState::Failed,
        hash_reused: false,
    };

    let usable = match current {
        Some((mtime, size)) => {
            cached.thumbnail_mtime == Some(mtime)
                && cached.thumbnail_size == Some(size)
                && cached.thumbnail_path.as_deref() == Some(stored.as_str())
                // 生成方式が変わっていたら、ファイルが残っていても使わない。
                // fingerprint は「原本が変わっていない」ことしか見ておらず、
                // こちら側の作り方の変更を検知できない。
                && cached.thumbnail_version == Some(THUMBNAIL_VERSION)
                && file.is_file()
        }
        None => false,
    };

    if usable {
        // 版が合う d_hash があればデコードすら不要。
        if cached.d_hash.is_some() && cached.d_hash_version == Some(D_HASH_VERSION) {
            return AnalysisOutcome {
                d_hash: cached.d_hash.clone(),
                thumbnail_path: Some(stored),
                thumbnail_state: ThumbnailState::Hit,
                hash_reused: true,
            };
        }
        // サムネイルは使えるがハッシュが旧方式。原本には戻らず作り直す。
        if let Some(hash) = fs::read(&file)
            .ok()
            .as_deref()
            .and_then(hash_thumbnail_bytes)
        {
            return AnalysisOutcome {
                d_hash: Some(hash),
                thumbnail_path: Some(stored),
                thumbnail_state: ThumbnailState::Hit,
                hash_reused: false,
            };
        }
    }

    let Some((image, decode_source)) = decode_hash_source_from(source) else {
        return failed();
    };
    let Some(bytes) = encode_thumbnail(&scale_for_thumbnail(&image)) else {
        return failed();
    };
    // サムネイルを保存できなくてもハッシュは出せる。次回また作り直すだけで、
    // 解析全体を止める理由にはならない。
    if fs::create_dir_all(thumbnail_dir).is_err() || fs::write(&file, &bytes).is_err() {
        return AnalysisOutcome {
            d_hash: hash_thumbnail_bytes(&bytes),
            thumbnail_path: None,
            thumbnail_state: ThumbnailState::Failed,
            hash_reused: false,
        };
    }
    AnalysisOutcome {
        d_hash: hash_thumbnail_bytes(&bytes),
        thumbnail_path: Some(stored),
        thumbnail_state: ThumbnailState::Generated(decode_source),
        hash_reused: false,
    }
}

fn hash_distance(left: &str, right: &str) -> u32 {
    u64::from_str_radix(left, 16)
        .ok()
        .zip(u64::from_str_radix(right, 16).ok())
        .map(|(a, b)| (a ^ b).count_ones())
        .unwrap_or(64)
}

// ---------------------------------------------------------------------------
// 連写候補の絞り込み
// ---------------------------------------------------------------------------

/// 候補判定に必要な最小限。撮影時刻順に並んでいることが前提。
#[derive(Clone, Debug)]
pub struct CandidateInput {
    pub id: String,
    pub captured_at: i64,
    pub source: TimestampSource,
}

#[derive(Debug)]
pub struct CandidateSelection {
    pub ids: HashSet<String>,
    /// 実際に使った時間窓。自動縮小が働くと `BURST_WINDOW_MS` より小さくなる。
    pub window_ms: i64,
    pub ratio: f64,
    pub narrowed: bool,
    /// 弱い根拠（mtime / 不明）同士だったために除外したペアの数。
    pub weak_pairs_skipped: usize,
}

/// 隣接ペアが連写候補になりうるか。時間と根拠の強さだけで決め、構図は見ない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairEligibility {
    Eligible,
    OutOfWindow,
    /// 撮影時刻の根拠が両側とも弱い。時間が近いだけでは連写とみなさない。
    WeakEvidence,
}

/// 候補判定の一次審査。`candidates_within`（解析対象の絞り込み）と
/// `build_burst_pairs`（閾値学習の出題元）が同じ規則を使うために切り出してある。
/// 二重実装すると、片方だけ直したときに学習と本番でズレる。
fn pair_eligibility(
    left: &CandidateInput,
    right: &CandidateInput,
    window_ms: i64,
) -> PairEligibility {
    let delta = right.captured_at - left.captured_at;
    if !(0..=window_ms).contains(&delta) {
        return PairEligibility::OutOfWindow;
    }
    // mtime はコピーやダウンロードで簡単に揃う。ここを通すと、一括ダウンロード
    // したフォルダが丸ごと候補になる。
    if left.source.is_weak() && right.source.is_weak() {
        return PairEligibility::WeakEvidence;
    }
    PairEligibility::Eligible
}

fn candidates_within(records: &[CandidateInput], window_ms: i64) -> (HashSet<String>, usize) {
    let mut ids = HashSet::new();
    let mut weak_pairs_skipped = 0usize;
    for pair in records.windows(2) {
        let [left, right] = pair else { continue };
        match pair_eligibility(left, right, window_ms) {
            PairEligibility::OutOfWindow => continue,
            PairEligibility::WeakEvidence => {
                weak_pairs_skipped += 1;
                continue;
            }
            PairEligibility::Eligible => {}
        }
        ids.insert(left.id.clone());
        ids.insert(right.id.clone());
    }
    (ids, weak_pairs_skipped)
}

/// 連写候補を選ぶ。候補が**多すぎる**うえに候補率も高いときだけ、時間窓を
/// 半分ずつ詰める。
///
/// 「時間が近い写真だけハッシュする」最適化は、撮影間隔がほぼ全て窓の内側に
/// 収まるフォルダでは原理的に効かない（実データ271枚では 98.5% が候補）。
///
/// Routine 2 は候補率だけで縮小を判断していたが、それでは密に撮影された正常な
/// データにも発火し、実測で連写グループを 52 → 16 に減らしていた。得たものは
/// 194 ms、失ったものは 36 グループで、Step 4 が 1枚 1.66 ms を実現した今では
/// 割に合わない。`CANDIDATE_COUNT_LIMIT` を併せて課し、**実コストが本当に
/// 問題になる規模でだけ**縮小するようにしてある。
/// mtime だけを根拠にした爆発は `candidates_within` の弱ペア除外が既に
/// 潰しているので（合成5000枚で候補 0%）、この変更で退行はしない。
fn select_burst_candidates(records: &[CandidateInput]) -> CandidateSelection {
    let total = records.len();
    let ratio_of = |ids: &HashSet<String>| {
        if total == 0 {
            0.0
        } else {
            ids.len() as f64 / total as f64
        }
    };
    let too_many = |ids: &HashSet<String>| {
        ids.len() > CANDIDATE_COUNT_LIMIT && ratio_of(ids) > CANDIDATE_RATIO_LIMIT
    };
    let mut window_ms = BURST_WINDOW_MS;
    let (mut ids, mut weak_pairs_skipped) = candidates_within(records, window_ms);
    let mut narrowed = false;
    while too_many(&ids) && window_ms > MIN_BURST_WINDOW_MS {
        window_ms = (window_ms / 2).max(MIN_BURST_WINDOW_MS);
        narrowed = true;
        let next = candidates_within(records, window_ms);
        ids = next.0;
        weak_pairs_skipped = next.1;
    }
    CandidateSelection {
        ratio: ratio_of(&ids),
        ids,
        window_ms,
        narrowed,
        weak_pairs_skipped,
    }
}

fn fingerprint(path: &Path) -> Option<(i64, i64)> {
    let metadata = fs::metadata(path).ok()?;
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((mtime, metadata.len() as i64))
}

fn is_supported(path: &Path) -> bool {
    matches!(path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()), Some(ext) if ["jpg", "jpeg", "png", "webp"].contains(&ext.as_str()))
}

fn photo_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Photo> {
    Ok(Photo {
        id: row.get(0)?,
        project_id: row.get(1)?,
        path: row.get(2)?,
        relative_path: row.get(3)?,
        name: row.get(4)?,
        captured_at: row.get(5)?,
        d_hash: row.get(6)?,
        rating: row.get(7)?,
        thumbnail_path: row.get(8)?,
        display_path: row.get(9)?,
        analysis_error: row.get(10)?,
    })
}

const PHOTO_COLUMNS: &str =
    "id,project_id,path,relative_path,name,captured_at,d_hash,rating,thumbnail_path,display_path,analysis_error";
/// 星の上限。1ラウンド通過ごとに +1 で、ここで頭打ちになる。「確定」も同じ値。
const MAX_RATING: i64 = 5;

// scan が見つけた1枚を登録・更新する。
// 比較に `=` ではなく `IS` を使うのが要点。SQL では `NULL = NULL` も
// `NULL = 値` も真にならないため、`=` だと fingerprint が NULL の行で
// captured_at と d_hash が再 scan のたびに NULL へ巻き戻されていた。
fn upsert_photo(
    conn: &Connection,
    project_id: &str,
    absolute: &str,
    relative: &str,
    name: &str,
    mtime: Option<i64>,
    size: Option<i64>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
         VALUES (?1,?2,?3,?4,?5,NULL,NULL,0,?6,?7,0)
         ON CONFLICT(project_id,path) DO UPDATE SET
           relative_path=excluded.relative_path, name=excluded.name, is_missing=0,
           captured_at=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.captured_at ELSE NULL END,
           timestamp_source=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.timestamp_source ELSE NULL END,
           d_hash=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash ELSE NULL END,
           d_hash_version=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash_version ELSE NULL END,
           thumbnail_path=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_path ELSE NULL END,
           thumbnail_mtime=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_mtime ELSE NULL END,
           thumbnail_size=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_size ELSE NULL END,
           thumbnail_source=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_source ELSE NULL END,
           thumbnail_version=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_version ELSE NULL END,
           analysis_error=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error ELSE NULL END,
           analysis_error_at=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_at ELSE NULL END,
           fingerprint_mtime=excluded.fingerprint_mtime, fingerprint_size=excluded.fingerprint_size",
        params![Uuid::new_v4().to_string(), project_id, absolute, relative, name, mtime, size],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn project_folder(app: &AppHandle, project_id: &str) -> Result<String, String> {
    connection(app)?
        .query_row(
            "SELECT folder_path FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|_| "Project was not found.".to_string())
}

/// 走査で見つけた 1 枚。**デスクトップと Android で同じ形にする。**
/// `path` は DB の `photos.path` にそのまま入る（絶対パス、または content:// URI）。
struct ScanEntry {
    path: String,
    relative_path: String,
    name: String,
    fingerprint: Option<(i64, i64)>,
}

/// **Android の `mediastore://<bucketId>` はフォルダではない。**
/// 走査の入口をここで分け、以降は同じ形を扱う。
const MEDIASTORE_PREFIX: &str = "mediastore://";

/// 走査元が「この端末のフォルダ」を指しているか。
///
/// Android では `mediastore://` や `smb://` のような形になるので、
/// **ここをディレクトリだと決めつけると必ず「見つかりません」になる。**
///
/// 前置きを並べるのではなく `://` の有無で見るのは、出所が増えたときに
/// ここを直し忘れないため。フォルダのパスに `://` は現れない。
fn is_local_folder(folder: &str) -> bool {
    !folder.contains("://")
}

fn collect_scan_entries(folder: &str) -> Result<Vec<ScanEntry>, String> {
    if let Some(bucket) = folder.strip_prefix(MEDIASTORE_PREFIX) {
        return collect_mediastore_entries(bucket);
    }
    if folder.starts_with(SMB_PREFIX) {
        return collect_smb_entries(folder);
    }
    Ok(WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && is_supported(entry.path()))
        .map(|entry| {
            let path = entry.into_path();
            let relative_path = path
                .strip_prefix(folder)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("photo")
                .to_owned();
            ScanEntry {
                // metadata が読めない場合に (0,0) を入れると、読めない
                // ファイル同士が「同じ fingerprint」に見えてしまう。
                fingerprint: fingerprint(&path),
                path: path.to_string_lossy().to_string(),
                relative_path,
                name,
            }
        })
        .collect())
}

#[cfg(target_os = "android")]
fn collect_mediastore_entries(bucket: &str) -> Result<Vec<ScanEntry>, String> {
    Ok(android_photos::list_photos(bucket)?
        .into_iter()
        .map(|photo| ScanEntry {
            path: photo.uri,
            relative_path: photo.relative_path,
            name: photo.name,
            fingerprint: Some((photo.modified_at, photo.size)),
        })
        .collect())
}

#[cfg(not(target_os = "android"))]
fn collect_mediastore_entries(_bucket: &str) -> Result<Vec<ScanEntry>, String> {
    Err("端末の写真はこの環境では読めません。".into())
}

#[cfg(target_os = "android")]
fn collect_smb_entries(folder: &str) -> Result<Vec<ScanEntry>, String> {
    Ok(android_smb::list_photos(folder)?
        .into_iter()
        .map(|photo| ScanEntry {
            path: photo.uri,
            relative_path: photo.relative_path,
            name: photo.name,
            fingerprint: Some((photo.modified_at, photo.size)),
        })
        .collect())
}

#[cfg(not(target_os = "android"))]
fn collect_smb_entries(_folder: &str) -> Result<Vec<ScanEntry>, String> {
    Err("NAS はこの環境では読めません。".into())
}

fn run_scan(app: AppHandle, registry: &TaskRegistry, project_id: String) -> Result<(), String> {
    let task_key = format!("scan:{project_id}");
    let folder = project_folder(&app, &project_id)?;
    let entries = collect_scan_entries(&folder)?;
    let total = entries.len();
    progress(
        &app,
        &project_id,
        "scan",
        "indexing",
        0,
        total,
        "Scanning photo files…",
    );

    let conn = connection(&app)?;
    conn.execute(
        "UPDATE projects SET status='scanning', updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE photos SET is_missing=1 WHERE project_id=?1",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for (index, entry) in entries.iter().enumerate() {
        if registry.is_cancelled(&task_key) {
            transaction.rollback().map_err(|error| error.to_string())?;
            connection(&app)?
                .execute(
                    "UPDATE projects SET status='ready', updated_at=?1 WHERE id=?2",
                    params![now(), project_id],
                )
                .map_err(|error| error.to_string())?;
            progress(
                &app,
                &project_id,
                "scan",
                "cancelled",
                index,
                total,
                "Scanning was cancelled.",
            );
            return Ok(());
        }
        let relative = entry.relative_path.clone();
        let name = entry.name.clone();
        let absolute = entry.path.clone();
        let (mtime, size) = match entry.fingerprint {
            Some((mtime, size)) => (Some(mtime), Some(size)),
            None => (None, None),
        };
        upsert_photo(
            &transaction,
            &project_id,
            &absolute,
            &relative,
            &name,
            mtime,
            size,
        )?;
        if (index + 1) % 250 == 0 || index + 1 == total {
            progress(
                &app,
                &project_id,
                "scan",
                "indexing",
                index + 1,
                total,
                "Recording photo locations…",
            );
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE projects SET photo_count=?1,status='ready',updated_at=?2 WHERE id=?3",
        params![count, now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    progress(
        &app,
        &project_id,
        "scan",
        "complete",
        total,
        total,
        "写真の読み込みが完了しました。",
    );
    // ここから先はアイドル時の事前生成。利用者が設定画面を触っている間に
    // 撮影時刻・サムネイル・dHash を少しずつ作っておく。「選別を開始」を
    // 押した時点では、出来ているぶんをそのまま使って即座に始められる。
    let handle = app.clone();
    let background_project = project_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let _ = spawn_analysis(
            handle.clone(),
            &registry,
            background_project,
            AnalysisMode::Background,
        );
    });
    Ok(())
}

/// worker 1本ぶんの仕事。1枚を読み、サムネイルと dHash を作る。
/// **DB には触れない。**書き込みは writer が `flush_results` でまとめて行う。
fn hash_one(thumbnails: &Path, index: usize, record: &HashRecord) -> PhotoWork {
    let mut result = PhotoWork::new(index, &record.id);
    // ローカルのパスでも content:// でも、ここから先は同じ経路を通る。
    let reference = PhotoRef::parse(&record.path);
    let source = reference.source();
    let current = source.fingerprint();
    let analysed = analyse_photo(thumbnails, &record.id, source.as_ref(), current, &record.cached);
    result.fingerprint = current;
    result.d_hash = analysed.d_hash;
    result.thumbnail_path = analysed.thumbnail_path;
    result.thumbnail_source = analysed.thumbnail_state.decode_source();
    result.hash_reused = analysed.hash_reused;
    if result.d_hash.is_none() {
        result.error = Some(match current {
            // ファイルは在るのに読めない = 壊れている / 非対応形式。
            Some(_) => "画像を読み取れませんでした（破損または非対応の形式）。".into(),
            None => "ファイルを開けませんでした（移動・削除・権限）。".into(),
        });
    }
    result
}

/// 解析できなかった写真の件数。UI へそのまま渡す。
fn failed_photo_count(conn: &Connection, project_id: &str) -> Result<usize, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM photos
         WHERE project_id=?1 AND is_missing=0 AND analysis_error IS NOT NULL",
        params![project_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as usize)
    .map_err(|error| error.to_string())
}

/// 解析 1 枚ぶんを DB へ書き戻す。**クロージャから出してあるのはテストのため。**
///
/// ここは「読めなかったときに前の結果を消さない」という約束を持っている。
/// その約束が破れるのは原本が一時的に読めないときだけで、実機の NAS でしか
/// 起きない。単体で突けるようにしておかないと、また気づかずに壊す。
fn apply_analysis(tx: &Connection, item: &PhotoWork) -> Result<(), String> {
    // 据え置きで済んだ1枚は、書き込む理由が無い。
    if item.hash_reused {
        return Ok(());
    }
    // metadata が読めない場合は fingerprint を NULL のままにする。(0,0) を
    // 入れると、読めないファイル同士が同じ fingerprint に見えてキャッシュが
    // 誤ヒットする。
    let (mtime, size) = match item.fingerprint {
        Some((mtime, size)) => (Some(mtime), Some(size)),
        None => (None, None),
    };
    // サムネイルを保存できたときだけ、それを作った時点の fingerprint を
    // 控える。次回の無効化判定はこの一致で行う。
    let (thumb_mtime, thumb_size) = match item.thumbnail_path {
        Some(_) => (mtime, size),
        None => (None, None),
    };
    // **読めなかったときは、前に作れていたものを消さない。**
    //
    // 原本が一瞬読めないことは普通に起きる（NAS が落ちた、Wi-Fi が切れた、
    // SMB のセッションが切れた）。そのたびに解析済みのサムネイルと dHash を
    // NULL で塗り潰すと、**作り終えた結果が失われて連写のまとまりも消える。**
    // 実機で 187 枚中 172 枚がこれで消えた。ファイルは残っているのに DB
    // からは消えている、という状態になる。
    //
    // 成功したときは上書きが正しい（原本が変わっていれば作り直すべき）。
    // なので COALESCE ではなく「失敗したときだけ据え置く」と書く。
    // 失敗は analysis_error に残るので、利用者には見えなくならない。
    let keep = item.d_hash.is_none() && item.thumbnail_path.is_none();
    tx.execute(
        "UPDATE photos SET
           d_hash=CASE WHEN ?13 THEN d_hash ELSE ?1 END,
           d_hash_version=CASE WHEN ?13 THEN d_hash_version ELSE ?2 END,
           thumbnail_path=CASE WHEN ?13 THEN thumbnail_path ELSE ?3 END,
           thumbnail_mtime=CASE WHEN ?13 THEN thumbnail_mtime ELSE ?4 END,
           thumbnail_size=CASE WHEN ?13 THEN thumbnail_size ELSE ?5 END,
           thumbnail_source=COALESCE(?6,thumbnail_source),
           thumbnail_version=CASE WHEN ?13 THEN thumbnail_version ELSE ?7 END,
           fingerprint_mtime=CASE WHEN ?13 THEN fingerprint_mtime ELSE ?8 END,
           fingerprint_size=CASE WHEN ?13 THEN fingerprint_size ELSE ?9 END,
           analysis_error=?10,analysis_error_at=?11
         WHERE id=?12",
        params![
            item.d_hash,
            item.d_hash.as_ref().map(|_| D_HASH_VERSION),
            item.thumbnail_path,
            thumb_mtime,
            thumb_size,
            item.thumbnail_source,
            // 版はサムネイルを保存できたときだけ立てる。パスが NULL のまま
            // 版だけ残ると、次回「使える」と誤判定する。
            item.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
            mtime,
            size,
            item.error,
            item.error.as_ref().map(|_| now()),
            item.photo_id,
            keep
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// `photo://` に来た URI から、写真の指し先を取り出す。
///
/// `convertFileSrc` は指し先を **encodeURIComponent** して URL に載せる。
/// つまり `/` も `:` も `%XX` になり、URL のパスは
/// 「区切りの `/` ひとつ ＋ 符号化された指し先」だけになる。
/// **外すのはその 1 つだけ。** 減らし過ぎると Unix の絶対パスが相対になり、
/// 増やし過ぎると Windows のドライブ文字の前に `/` が残る。
fn photo_scheme_target(path: &str, query: Option<&str>) -> String {
    let raw = query
        .and_then(|query| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("path="))
        })
        .unwrap_or_else(|| path.strip_prefix('/').unwrap_or(path));
    percent_decode(raw)
}

/// %XX を戻す。URI に載る写真のパスは日本語も含むので必要。
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 原本のバイト列を返す。**読めなければ None**（呼び出し側が 404 にする）。
fn photo_scheme_response(target: &str) -> Option<tauri::http::Response<Vec<u8>>> {
    let reference = PhotoRef::parse(target);
    let bytes = reference.source().all()?;
    let mime = match target.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    tauri::http::Response::builder()
        .header("Content-Type", mime)
        // 同じ写真を何度も開くので、画面側に持たせる。
        .header("Cache-Control", "max-age=3600")
        .body(bytes)
        .ok()
}

fn run_burst_analysis(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
    mode: AnalysisMode,
) -> Result<(), String> {
    let task = mode.task();
    let task_key = format!("{task}:{project_id}");
    // 事前生成は前面の解析に道を譲る。同じ行を両方が触っても結果は同じだが、
    // ディスクを取り合って前面を遅くする意味が無い。
    let foreground_key = format!("{}:{project_id}", AnalysisMode::Foreground.task());
    let should_stop = || {
        registry.is_cancelled(&task_key)
            || (mode == AnalysisMode::Background && registry.is_running(&foreground_key))
    };
    let timeout = Duration::from_millis(PHOTO_TIMEOUT_MS);
    let thumbnails = thumbnail_dir(&app)?;
    let conn = connection(&app)?;
    let records: Vec<MetadataRecord> = {
        let mut statement = conn
            .prepare("SELECT id,path,captured_at,timestamp_source FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY path")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    // 旧ビルドの行は captured_at はあっても timestamp_source が無い。経路が
    // 分からないままだと連写判定で信用度を測れないので、その場合も読み直す
    // （EXIF 読取は 0.14 ms/枚）。既に両方ある行は仕事そのものを作らない。
    let metadata_jobs: Vec<MetadataJob> = records
        .iter()
        .filter(|(_, _, captured_at, source)| captured_at.is_none() || source.is_none())
        .map(|(id, path, _, _)| MetadataJob {
            id: id.clone(),
            path: path.clone(),
        })
        .collect();
    let metadata_total = metadata_jobs.len();
    let mut failed = failed_photo_count(&conn, &project_id)?;

    if metadata_total > 0 {
        progress(
            &app,
            &project_id,
            task,
            "metadata",
            0,
            metadata_total,
            "撮影時刻を読み取っています…",
        );
        let conn = connection(&app)?;
        let interval = progress_interval(metadata_total);
        let mut pending: Vec<PhotoWork> = Vec::with_capacity(ANALYSIS_CHUNK_SIZE);
        let mut committed = 0usize;
        let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
            tx.execute(
                "UPDATE photos SET captured_at=?1,timestamp_source=?2,
                   analysis_error=?3,analysis_error_at=?4
                 WHERE id=?5",
                params![
                    item.captured_at,
                    item.timestamp_source
                        .unwrap_or(TimestampSource::Unknown)
                        .as_str(),
                    item.error,
                    item.error.as_ref().map(|_| now()),
                    item.photo_id
                ],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        };
        let outcome = run_in_parallel(
            Arc::new(metadata_jobs),
            mode.workers(),
            timeout,
            &should_stop,
            // worker はファイルを読むだけ。DB には触れない。
            |index, job: &MetadataJob| {
                let mut result = PhotoWork::new(index, &job.id);
                // ローカルのパスでも content:// でも、ここから先は同じ経路を通る。
                let reference = PhotoRef::parse(&job.path);
                match read_capture_time_from(reference.source().as_ref()) {
                    Some(capture) => {
                        result.captured_at = Some(capture.at);
                        result.timestamp_source = Some(capture.source);
                    }
                    None => {
                        result.error = Some("撮影時刻を読み取れませんでした。".into());
                    }
                }
                result
            },
            &mut |item| {
                if item.error.is_some() {
                    failed += 1;
                }
                pending.push(item);
                if pending.len() >= ANALYSIS_CHUNK_SIZE {
                    committed += flush_results(&conn, &mut pending, &apply)?;
                    progress_note(
                        &app,
                        &project_id,
                        task,
                        "metadata",
                        committed,
                        metadata_total,
                        "撮影時刻を読み取っています…",
                        ProgressNote {
                            warning: None,
                            failed,
                        },
                    );
                    if mode == AnalysisMode::Background {
                        std::thread::sleep(Duration::from_millis(BACKGROUND_PAUSE_MS));
                    }
                } else if (committed + pending.len()) % interval == 0 {
                    progress_note(
                        &app,
                        &project_id,
                        task,
                        "metadata",
                        committed + pending.len(),
                        metadata_total,
                        "撮影時刻を読み取っています…",
                        ProgressNote {
                            warning: None,
                            failed,
                        },
                    );
                }
                Ok(())
            },
        )?;
        // キャンセルされていても、読み終わっているぶんは書いてから抜ける。
        committed += flush_results(&conn, &mut pending, &apply)?;
        if outcome.cancelled {
            progress_note(
                &app,
                &project_id,
                task,
                "cancelled",
                committed,
                metadata_total,
                format!("解析を中断しました。{committed} 件まで保存済みです。"),
                ProgressNote {
                    warning: None,
                    failed,
                },
            );
            return Ok(());
        }
    }

    let candidates: Vec<HashRecord> = {
        let conn = connection(&app)?;
        let mut statement = conn
            .prepare(
                "SELECT id,path,captured_at,timestamp_source,d_hash,d_hash_version,
                        thumbnail_path,thumbnail_mtime,thumbnail_size,thumbnail_version
                 FROM photos
                 WHERE project_id=?1 AND is_missing=0 AND captured_at IS NOT NULL
                 ORDER BY captured_at",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                let source: Option<String> = row.get(3)?;
                Ok(HashRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    captured_at: row.get(2)?,
                    source: TimestampSource::parse(source.as_deref()),
                    cached: CachedAnalysis {
                        d_hash: row.get(4)?,
                        d_hash_version: row.get(5)?,
                        thumbnail_path: row.get(6)?,
                        thumbnail_mtime: row.get(7)?,
                        thumbnail_size: row.get(8)?,
                        thumbnail_version: row.get(9)?,
                    },
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    // 時間が近いものだけがハッシュを必要とする。ただし mtime しか根拠が無い
    // ペアは連写とみなさず、候補が多すぎるときは時間窓そのものを詰める。
    let selection = select_burst_candidates(
        &candidates
            .iter()
            .map(|record| CandidateInput {
                id: record.id.clone(),
                captured_at: record.captured_at,
                source: record.source,
            })
            .collect::<Vec<_>>(),
    );
    let needed_records: Vec<_> = candidates
        .iter()
        .filter(|record| selection.ids.contains(&record.id))
        .cloned()
        .collect();
    let needed_total = needed_records.len();
    let warning = selection.narrowed.then(|| {
        format!(
            "撮影間隔が近い写真が多すぎたため、連写とみなす時間の幅を {:.1} 秒に狭めました（候補 {} / {} 枚）。",
            selection.window_ms as f64 / 1000.0,
            needed_total,
            candidates.len()
        )
    });
    progress_note(
        &app,
        &project_id,
        task,
        "hashing",
        0,
        needed_total,
        "近い構図を比較しています…",
        ProgressNote { warning, failed },
    );

    let conn = connection(&app)?;
    let interval = progress_interval(needed_total.max(1));
    let mut pending: Vec<PhotoWork> = Vec::with_capacity(ANALYSIS_CHUNK_SIZE);
    let mut committed = 0usize;
    let apply = apply_analysis;
    let thumbnails_for_workers = thumbnails.clone();
    let outcome = run_in_parallel(
        Arc::new(needed_records),
        mode.workers(),
        timeout,
        &should_stop,
        // worker はファイルを読んでサムネイルを書くだけ。DB には触れない。
        move |index, record: &HashRecord| hash_one(&thumbnails_for_workers, index, record),
        &mut |item| {
            if item.error.is_some() {
                failed += 1;
            }
            pending.push(item);
            if pending.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending, &apply)?;
                progress_note(
                    &app,
                    &project_id,
                    task,
                    "hashing",
                    committed,
                    needed_total,
                    "近い構図を比較しています…",
                    ProgressNote {
                        warning: None,
                        failed,
                    },
                );
                if mode == AnalysisMode::Background {
                    std::thread::sleep(Duration::from_millis(BACKGROUND_PAUSE_MS));
                }
            } else if (committed + pending.len()) % interval == 0 {
                progress_note(
                    &app,
                    &project_id,
                    task,
                    "hashing",
                    committed + pending.len(),
                    needed_total,
                    "近い構図を比較しています…",
                    ProgressNote {
                        warning: None,
                        failed,
                    },
                );
            }
            Ok(())
        },
    )?;
    committed += flush_results(&conn, &mut pending, &apply)?;
    if outcome.cancelled {
        progress_note(
            &app,
            &project_id,
            task,
            "cancelled",
            committed,
            needed_total,
            format!(
                "解析を中断しました。{committed} 件まで保存済みです。次回はここから再開します。"
            ),
            ProgressNote {
                warning: None,
                failed,
            },
        );
        return Ok(());
    }
    progress_note(
        &app,
        &project_id,
        task,
        "complete",
        needed_total,
        needed_total,
        "連写候補の準備ができました。",
        ProgressNote {
            warning: None,
            failed,
        },
    );
    Ok(())
}

impl Project {
    /// 保存済みの種類とラベルを入れる。無ければ `folder_path` から導く。
    fn with_source(mut self, kind: Option<String>, label: Option<String>) -> Self {
        let (kind, label) = source_of(&self.folder_path, kind, label);
        self.source_kind = kind;
        self.source_label = label;
        self
    }
}

/// 保存されていない古い行のために、`folder_path` から出所を導く。
///
/// **既存の行を書き換えないための逃げ道。** 新しく作る行には作成時に
/// 種類とラベルを入れるので、ここを通るのは今までに作られた行だけ。
fn source_of(
    folder_path: &str,
    kind: Option<String>,
    label: Option<String>,
) -> (String, String) {
    let kind = kind.unwrap_or_else(|| {
        if folder_path.starts_with(SMB_PREFIX) {
            "nas".into()
        } else if folder_path.starts_with(MEDIASTORE_PREFIX) {
            "album".into()
        } else if folder_path.is_empty() {
            "imported".into()
        } else {
            "folder".into()
        }
    });
    let label = label.unwrap_or_else(|| {
        // 末尾の名前だけ拾う。アルバムは ID しか無いので、それがそのまま出る。
        // 新しく作る行には作成時に本当の名前が入るので、ここは古い行の保険。
        folder_path
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(folder_path)
            .to_string()
    });
    (kind, label)
}

/// 裏で進む 1 本ぶんの状態。**画面は done / total だけを出す。**
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepStage {
    /// idle | running | done | error
    state: String,
    done: i64,
    /// 走査中は枚数が確定しないので None。画面は「120 / ?」と出す。
    total: Option<i64>,
}

/// プロジェクトの準備状況。走査・撮影時刻とサムネイル・表示用画像の 3 本。
///
/// **1 か所にまとめて返す。** 以前は走査の進捗イベントと表示用画像の残数が
/// 別々の仕組みで出ていて、画面の 2 か所に違う進捗が並び、しかも片方は
/// プロジェクトを切り替えても前の値が残っていた。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prep {
    project_id: String,
    scan: PrepStage,
    meta: PrepStage,
    preview: PrepStage,
    /// いま作っている表示用画像の長辺。
    preview_edge: u32,
}

#[tauri::command]
fn get_project_prep(
    app: AppHandle,
    registry: State<'_, TaskRegistry>,
    project_id: String,
) -> Result<Prep, String> {
    let scanning = registry.is_running(&format!("scan:{project_id}"));
    let analysing = registry.is_running(&format!("burst:{project_id}"))
        || registry.is_running(&format!("background:{project_id}"));
    let generating = registry.is_running(&format!("display:{project_id}"));
    let edge = resolve_display_edge(&app, &project_id)?;
    let conn = connection(&app)?;

    let count = |sql: &str| -> Result<i64, String> {
        conn.query_row(sql, params![project_id], |row| row.get(0))
            .map_err(|error| error.to_string())
    };
    let total = count("SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0")?;
    let with_meta = count(
        "SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0
           AND captured_at IS NOT NULL AND thumbnail_path IS NOT NULL",
    )?;
    let with_preview = conn
        .query_row(
            "SELECT COUNT(*) FROM photos
             WHERE project_id=?1 AND is_missing=0
               AND display_path IS NOT NULL AND display_edge=?2",
            params![project_id, edge as i64],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    // 走っていないのに揃っていなければ idle。「止まっている」ことが分かる。
    let settle = |running: bool, done: i64, total: i64| PrepStage {
        state: if running {
            "running".into()
        } else if total > 0 && done >= total {
            "done".into()
        } else {
            "idle".into()
        },
        done,
        total: Some(total),
    };

    Ok(Prep {
        scan: PrepStage {
            state: if scanning {
                "running".into()
            } else if total > 0 {
                "done".into()
            } else {
                "idle".into()
            },
            done: total,
            // 走査中は「これから何枚見つかるか」が分からない。
            total: if scanning { None } else { Some(total) },
        },
        meta: settle(analysing, with_meta, total),
        preview: settle(generating, with_preview, total),
        preview_edge: edge,
        project_id,
    })
}

#[tauri::command]
fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let conn = connection(&app)?;
    let mut statement = conn
        .prepare("SELECT id,name,folder_path,photo_count,status,created_at,updated_at,burst_threshold,burst_threshold_learned_at,source_kind,source_label FROM projects ORDER BY updated_at DESC")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                folder_path: row.get(2)?,
                photo_count: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                burst_threshold: row.get(7)?,
                burst_threshold_learned_at: row.get(8)?,
                source_kind: String::new(),
                source_label: String::new(),
            }
            .with_source(row.get(9)?, row.get(10)?))
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_project(
    app: AppHandle,
    name: String,
    folder_path: String,
    source_kind: Option<String>,
    source_label: Option<String>,
) -> Result<Project, String> {
    // 端末のフォルダのときだけ実在を確かめる。アルバムや NAS は
    // パスではないので、ここで弾いてはいけない。
    if is_local_folder(&folder_path) && !Path::new(&folder_path).is_dir() {
        return Err(
            "選択した写真フォルダが見つかりません。フォルダの場所を確認してください。".into(),
        );
    }
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name,
        folder_path,
        photo_count: 0,
        status: "new".into(),
        created_at: now(),
        updated_at: now(),
        burst_threshold: None,
        burst_threshold_learned_at: None,
        source_kind: String::new(),
        source_label: String::new(),
    }
    .with_source(source_kind, source_label);
    connection(&app)?
        .execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at,source_kind,source_label)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                project.id, project.name, project.folder_path, project.photo_count,
                project.status, project.created_at, project.updated_at,
                project.source_kind, project.source_label
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(project)
}

/// まだ解析が必要な写真の枚数。0 なら事前生成を起動する意味がない。
///
/// これが無かったため、プロジェクトを開くたびに無条件で事前生成を起動しており、
/// 実際には何もすることが無くても進捗イベントだけが飛んで、UI に解析中の帯が
/// 一瞬出ていた。
#[tauri::command]
fn get_analysis_backlog(app: AppHandle, project_id: String) -> Result<i64, String> {
    connection(&app)?
        .query_row(
            "SELECT COUNT(*) FROM photos
             WHERE project_id=?1 AND is_missing=0
               AND (captured_at IS NULL
                 OR timestamp_source IS NULL
                 OR d_hash IS NULL
                 OR d_hash_version IS NULL
                 OR d_hash_version <> ?2
                 OR thumbnail_path IS NULL
                 OR thumbnail_version IS NULL
                 OR thumbnail_version <> ?3)",
            params![project_id, D_HASH_VERSION, THUMBNAIL_VERSION],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

/// プロジェクトを削除する。**写真原本には一切触れない。**
/// 消すのは DB の行と、このアプリが `app_data_dir` 配下に作ったサムネイルだけ。
#[tauri::command]
fn delete_project(app: AppHandle, project_id: String) -> Result<(), String> {
    let conn = connection(&app)?;

    // 先にサムネイルの実体を消す。DB を消してからでは対象が分からなくなる。
    let thumbnails: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT thumbnail_path FROM photos WHERE project_id=?1 AND thumbnail_path IS NOT NULL")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    let mut removed = 0usize;
    for path in &thumbnails {
        // 取りこぼしても致命的ではない。消せた数だけ数えて先へ進む。
        if fs::remove_file(Path::new(path)).is_ok() {
            removed += 1;
        }
    }

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM project_states WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM photos WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    let deleted = transaction
        .execute("DELETE FROM projects WHERE id=?1", params![project_id])
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;

    if deleted == 0 {
        return Err("プロジェクトが見つかりませんでした。".into());
    }
    eprintln!("削除: プロジェクト {project_id} / サムネイル {removed} 件");
    Ok(())
}

fn photo_sort_clause(sort: Option<&str>) -> &'static str {
    match sort {
        // 星の高い順。同点はファイル順にして並びを安定させる。
        Some("rating") => " ORDER BY rating DESC, relative_path",
        _ => " ORDER BY relative_path",
    }
}

/// 一覧・選別対象の絞り込みは**星ちょうど一致**だけ。星が選別状態そのものなので、
/// これ以外の区分を持たない。`rating` が `None` なら全件。
///
/// SQL と bind 値を必ず一緒に返す。以前は句だけを組み立てて bind は別の場所で
/// 作っていたため、`rating` が `None` のときにプレースホルダ 3 個へ 4 個渡して
/// 実行時に落ちていた。
fn photo_page_query(
    project_id: &str,
    offset: i64,
    limit: i64,
    rating: Option<i64>,
    sort: Option<&str>,
) -> (String, String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value;
    let filter = if rating.is_some() {
        " AND rating=?2"
    } else {
        ""
    };
    let count_sql =
        format!("SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0{filter}");

    // ページ側は LIMIT/OFFSET が先に並ぶので、星は ?4 になる。
    let page_filter = if rating.is_some() {
        " AND rating=?4"
    } else {
        ""
    };
    let page_sql = format!(
        "SELECT {PHOTO_COLUMNS} FROM photos WHERE project_id=?1 AND is_missing=0{page_filter}{} LIMIT ?2 OFFSET ?3",
        photo_sort_clause(sort)
    );

    let mut binds = vec![
        Value::from(project_id.to_string()),
        Value::from(limit.clamp(1, PAGE_SIZE_LIMIT)),
        Value::from(offset.max(0)),
    ];
    if let Some(value) = rating {
        binds.push(Value::from(value));
    }
    (count_sql, page_sql, binds)
}

#[tauri::command]
fn get_project_photo_page(
    app: AppHandle,
    project_id: String,
    offset: i64,
    limit: i64,
    rating: Option<i64>,
    sort: Option<String>,
) -> Result<PhotoPage, String> {
    let conn = connection(&app)?;
    let (count_sql, page_sql, binds) =
        photo_page_query(&project_id, offset, limit, rating, sort.as_deref());

    // 件数は project_id と（あれば）星だけ。LIMIT/OFFSET は使わない。
    let count_binds: Vec<rusqlite::types::Value> = std::iter::once(binds[0].clone())
        .chain(binds.get(3).cloned())
        .collect();
    let total = conn
        .query_row(&count_sql, rusqlite::params_from_iter(count_binds), |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())?;

    let mut statement = conn.prepare(&page_sql).map_err(|error| error.to_string())?;
    let photos = statement
        .query_map(rusqlite::params_from_iter(binds), photo_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(PhotoPage { photos, total })
}

/// 1グループぶんの判定結果。星だけを送る。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionResult {
    id: String,
    rating: i64,
}

/// 星ごとの枚数。`counts[n]` が★n の枚数（0〜5）。
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SelectionSummary {
    counts: Vec<i64>,
    total: i64,
}

/// 判定結果を書き込む。全件ではなく**判定したグループぶんだけ**を受け取る前提。
/// 全件を毎回送ると 5,000 行の IPC が毎クリック発生する。
#[tauri::command]
fn save_selection_results(
    app: AppHandle,
    project_id: String,
    entries: Vec<SelectionResult>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let conn = connection(&app)?;
    commit_in_chunks(
        &conn,
        &entries,
        &|| false,
        &mut |tx: &Connection, entry: &SelectionResult, _index: usize| {
            tx.execute(
                "UPDATE photos SET rating=?1 WHERE id=?2 AND project_id=?3",
                params![entry.rating.clamp(0, MAX_RATING), entry.id, project_id],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        },
    )?;
    Ok(())
}

/// 一時テーブルに入れる id の1回ぶん。SQLite の変数上限（既定 999）より十分小さく取る。
const ID_BIND_CHUNK: usize = 500;

/// ある星の写真をまとめて別の星へ移す本体。
///
/// `include_ids` が `Some` ならその id だけ、`None` なら `exclude_ids` を除いた
/// **その星の全件**が対象。既定を「除外指定」にしてあるのは、5,000 枚に対して
/// 「全選択」が既定の UI だから。全件の id を毎回 IPC で送らずに済む。
///
/// id は `IN (?, ?, …)` ではなく**一時テーブル経由**で渡す。
/// 数千件を並べると SQLite の変数上限に当たるため。
fn move_rating_in(
    conn: &Connection,
    project_id: &str,
    from_rating: i64,
    to_rating: i64,
    include_ids: Option<Vec<String>>,
    exclude_ids: &[String],
) -> Result<i64, String> {
    let valid = |rating: i64| (0..=MAX_RATING).contains(&rating);
    if !valid(from_rating) || !valid(to_rating) {
        return Err(format!(
            "レートは 0〜{MAX_RATING} の範囲で指定してください。"
        ));
    }
    // 同じ星への移動は操作として意味が無い。エラーにはせず、何もしない。
    if from_rating == to_rating {
        return Ok(0);
    }

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;

    // 対象 id の絞り込みは常に「その星・そのプロジェクト・欠損していない」が前提。
    // 一時テーブルに他プロジェクトの id が混じっても、この条件で弾かれる。
    let base = "project_id=?1 AND rating=?2 AND is_missing=0";
    let updated = match include_ids {
        Some(ids) if ids.is_empty() => 0,
        Some(ids) => {
            fill_id_table(&transaction, &ids)?;
            transaction
                .execute(
                    &format!(
                        "UPDATE photos SET rating=?3
                         WHERE {base} AND id IN (SELECT id FROM _rating_move)"
                    ),
                    params![project_id, from_rating, to_rating],
                )
                .map_err(|error| error.to_string())?
        }
        None if exclude_ids.is_empty() => transaction
            .execute(
                &format!("UPDATE photos SET rating=?3 WHERE {base}"),
                params![project_id, from_rating, to_rating],
            )
            .map_err(|error| error.to_string())?,
        None => {
            fill_id_table(&transaction, exclude_ids)?;
            transaction
                .execute(
                    &format!(
                        "UPDATE photos SET rating=?3
                         WHERE {base} AND id NOT IN (SELECT id FROM _rating_move)"
                    ),
                    params![project_id, from_rating, to_rating],
                )
                .map_err(|error| error.to_string())?
        }
    };

    // 一時テーブルは接続に紐づくので、次の呼び出しに残さない。
    transaction
        .execute("DROP TABLE IF EXISTS temp._rating_move", [])
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(updated as i64)
}

/// 対象 id を一時テーブルへ入れ直す。前回の中身は必ず捨てる。
fn fill_id_table(conn: &Connection, ids: &[String]) -> Result<(), String> {
    conn.execute("DROP TABLE IF EXISTS temp._rating_move", [])
        .map_err(|error| error.to_string())?;
    conn.execute("CREATE TEMP TABLE _rating_move (id TEXT PRIMARY KEY)", [])
        .map_err(|error| error.to_string())?;
    for chunk in ids.chunks(ID_BIND_CHUNK) {
        let placeholders = vec!["(?)"; chunk.len()].join(",");
        conn.execute(
            &format!("INSERT OR IGNORE INTO _rating_move (id) VALUES {placeholders}"),
            rusqlite::params_from_iter(chunk.iter()),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 選別結果の画面から、ある星の写真をまとめて別の星へ移す。
/// 移した枚数を返す。
#[tauri::command]
fn move_rating(
    app: AppHandle,
    project_id: String,
    from_rating: i64,
    to_rating: i64,
    include_ids: Option<Vec<String>>,
    exclude_ids: Vec<String>,
) -> Result<i64, String> {
    let conn = connection(&app)?;
    move_rating_in(
        &conn,
        &project_id,
        from_rating,
        to_rating,
        include_ids,
        &exclude_ids,
    )
}

/// 星を全部 0 に戻す。解析結果（d_hash やサムネイル）には触れないので、
/// やり直しても解析のやり直しにはならない。
#[tauri::command]
fn reset_selection_results(app: AppHandle, project_id: String) -> Result<(), String> {
    connection(&app)?
        .execute(
            "UPDATE photos SET rating=0 WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_selection_summary(app: AppHandle, project_id: String) -> Result<SelectionSummary, String> {
    let conn = connection(&app)?;
    let mut counts = vec![0i64; (MAX_RATING + 1) as usize];
    let mut statement = conn
        .prepare(
            "SELECT rating, COUNT(*) FROM photos
             WHERE project_id=?1 AND is_missing=0 GROUP BY rating",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut total = 0i64;
    for row in rows {
        let (rating, count) = row.map_err(|error| error.to_string())?;
        total += count;
        if let Some(slot) = counts.get_mut(rating.clamp(0, MAX_RATING) as usize) {
            *slot += count;
        }
    }
    Ok(SelectionSummary { counts, total })
}

// ---------------------------------------------------------------------------
// 書き出し（原本に触れる唯一の領域）
// ---------------------------------------------------------------------------

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExportReport {
    processed: usize,
    skipped: usize,
    failed: usize,
    /// 失敗と、その理由。全部は返さず先頭だけ。
    errors: Vec<String>,
}

impl ExportReport {
    fn fail(&mut self, path: &str, reason: impl std::fmt::Display) {
        self.failed += 1;
        if self.errors.len() < 20 {
            self.errors.push(format!("{path}: {reason}"));
        }
    }
}

/// 対象の写真を星ごとに取り出す。`ratings` が空なら全部。
fn photos_for_export(
    conn: &Connection,
    project_id: &str,
    ratings: &[i64],
) -> Result<Vec<(String, i64)>, String> {
    let mut statement = conn
        .prepare("SELECT path,rating FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY relative_path")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let all = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(if ratings.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|(_, rating)| ratings.contains(rating))
            .collect()
    })
}

/// 出力先に同名があるとき、`name (2).jpg` のように連番を付ける。
/// 既にあるファイルを上書きしない。
fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
    let candidate = directory.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("photo");
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
    for index in 2..10_000 {
        let name = if extension.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{extension}")
        };
        let next = directory.join(name);
        if !next.exists() {
            return next;
        }
    }
    candidate
}

/// 星ごとのフォルダへ書き出す。`star-5` `star-4` … を出力先に作る。
///
/// `move_files` が false ならコピー。true なら移動で、**原本フォルダから写真が
/// 消える**。移動は「コピーしてから元を消す」順で行い、コピーに失敗したら
/// 元は残す。同じボリュームなら rename を試し、失敗したらコピーへ落とす。
#[tauri::command]
fn export_by_rating(
    app: AppHandle,
    project_id: String,
    destination: String,
    ratings: Vec<i64>,
    move_files: bool,
) -> Result<ExportReport, String> {
    let root = PathBuf::from(&destination);
    if !root.is_dir() {
        return Err("出力先フォルダが見つかりません。".into());
    }
    // 出力先が写真フォルダの中だと、書き出した先をまた読んでしまう。
    let folder = project_folder(&app, &project_id)?;
    if root.starts_with(Path::new(&folder)) {
        return Err("出力先には、写真フォルダの外を指定してください。".into());
    }

    let conn = connection(&app)?;
    let targets = photos_for_export(&conn, &project_id, &ratings)?;
    let mut report = ExportReport::default();

    for (path, rating) in &targets {
        let source = Path::new(path);
        if !source.is_file() {
            report.skipped += 1;
            continue;
        }
        let directory = root.join(format!("star-{rating}"));
        if let Err(error) = fs::create_dir_all(&directory) {
            report.fail(path, error);
            continue;
        }
        let file_name = source
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("photo.jpg");
        let target = unique_destination(&directory, file_name);

        if move_files {
            // 同じボリュームなら rename が速くて安全。失敗したらコピー＋削除。
            if fs::rename(source, &target).is_ok() {
                report.processed += 1;
                continue;
            }
            match fs::copy(source, &target) {
                Ok(_) => match fs::remove_file(source) {
                    Ok(()) => report.processed += 1,
                    // コピーは済んでいるので写真は失われない。元が残るだけ。
                    Err(error) => report.fail(path, format!("複製後に元を削除できません: {error}")),
                },
                Err(error) => report.fail(path, error),
            }
        } else {
            match fs::copy(source, &target) {
                Ok(_) => report.processed += 1,
                Err(error) => report.fail(path, error),
            }
        }
    }

    // 移動したなら DB の場所が古くなる。次の scan で拾い直させる。
    if move_files && report.processed > 0 {
        conn.execute(
            "UPDATE photos SET is_missing=1 WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(report)
}

/// JPEG の XMP パケットを組み立てる。`xmp:Rating` は 0〜5 をそのまま持てる。
fn xmp_packet(rating: i64) -> Vec<u8> {
    let body = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    );
    let mut packet = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    packet.extend_from_slice(body.as_bytes());
    packet
}

/// JPEG のセグメント列を組み直し、XMP の APP1 を差し替える（無ければ挿入）。
///
/// 画像本体（SOS 以降）には一切触れない。既存の EXIF セグメントも
/// そのまま残すので、撮影情報は失われない。
fn jpeg_with_rating(original: &[u8], rating: i64) -> Result<Vec<u8>, String> {
    if original.len() < 4 || original[0] != 0xFF || original[1] != 0xD8 {
        return Err("JPEG ではありません".into());
    }
    let packet = xmp_packet(rating);
    if packet.len() + 2 > 0xFFFF {
        return Err("XMP が大きすぎます".into());
    }

    // 走査と組み立てを分ける。1パスで「既存を置換」と「無ければ挿入」を両方
    // やろうとすると、XMP が既にある JPEG で置換と挿入が二重に走り、
    // 書くたびに APP1 が増えていく。
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut insert_after = 0usize; // 何番目のセグメントの後ろに XMP を置くか
    let mut index = 2usize;
    while index + 4 <= original.len() {
        if original[index] != 0xFF {
            break;
        }
        let marker = original[index + 1];
        // SOS 以降は画像本体。ここから先はそのまま写す。
        if marker == 0xDA {
            break;
        }
        let length = u16::from_be_bytes([original[index + 2], original[index + 3]]) as usize;
        if length < 2 || index + 2 + length > original.len() {
            return Err("JPEG の構造が壊れています".into());
        }
        let payload = &original[index + 4..index + 2 + length];
        let is_xmp = marker == 0xE1 && payload.starts_with(b"http://ns.adobe.com/xap/1.0/\0");
        if !is_xmp {
            // 既存の XMP だけを落とす。EXIF などは順番ごと残す。
            kept.push(&original[index..index + 2 + length]);
            if marker == 0xE1 {
                insert_after = kept.len();
            }
        }
        index += 2 + length;
    }

    let mut out = Vec::with_capacity(original.len() + packet.len() + 4);
    out.extend_from_slice(&original[0..2]); // SOI
    let push_packet = |out: &mut Vec<u8>| {
        out.push(0xFF);
        out.push(0xE1);
        out.extend_from_slice(&((packet.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&packet);
    };
    for (position, segment) in kept.iter().enumerate() {
        if position == insert_after {
            push_packet(&mut out);
        }
        out.extend_from_slice(segment);
    }
    // EXIF が無い、またはすべてのセグメントの後ろに置く場合。
    if insert_after >= kept.len() {
        push_packet(&mut out);
    }
    out.extend_from_slice(&original[index..]);

    if out.len() < 4 || out[out.len() - 2] != 0xFF || out[out.len() - 1] != 0xD9 {
        return Err("書き出した JPEG の終端が不正です".into());
    }
    Ok(out)
}

/// 星を写真本体の XMP に書き込む。**原本を書き換える。**
///
/// 一時ファイルへ書いてから中身を検証し、問題なければ置き換える。
/// 途中で失敗しても原本はそのまま残る。JPEG 以外は触らない。
#[tauri::command]
fn write_ratings_to_files(
    app: AppHandle,
    project_id: String,
    ratings: Vec<i64>,
) -> Result<ExportReport, String> {
    let conn = connection(&app)?;
    let targets = photos_for_export(&conn, &project_id, &ratings)?;
    let mut report = ExportReport::default();

    for (path, rating) in &targets {
        let source = Path::new(path);
        let is_jpeg = matches!(
            source
                .extension()
                .and_then(|v| v.to_str())
                .map(|v| v.to_ascii_lowercase())
                .as_deref(),
            Some("jpg") | Some("jpeg")
        );
        if !is_jpeg || !source.is_file() {
            report.skipped += 1;
            continue;
        }
        let original = match fs::read(source) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.fail(path, error);
                continue;
            }
        };
        let updated = match jpeg_with_rating(&original, *rating) {
            Ok(bytes) => bytes,
            Err(reason) => {
                report.fail(path, reason);
                continue;
            }
        };

        // 同じフォルダに一時ファイルを作る。別ボリュームだと置換が原子的で
        // なくなるため、必ず隣に置く。
        let temporary = source.with_extension("photocurator-tmp");
        if let Err(error) = fs::write(&temporary, &updated) {
            let _ = fs::remove_file(&temporary);
            report.fail(path, error);
            continue;
        }
        // 置き換える前に、書いたものが画像として開けるか確かめる。
        if let Err(error) = image::ImageReader::open(&temporary)
            .map_err(|e| e.to_string())
            .and_then(|reader| reader.into_dimensions().map_err(|e| e.to_string()))
        {
            let _ = fs::remove_file(&temporary);
            report.fail(
                path,
                format!("検証に失敗したため原本は変更していません: {error}"),
            );
            continue;
        }
        match fs::rename(&temporary, source) {
            Ok(()) => report.processed += 1,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                report.fail(path, error);
            }
        }
    }
    Ok(report)
}

#[tauri::command]
fn get_photos_by_ids(
    app: AppHandle,
    project_id: String,
    photo_ids: Vec<String>,
) -> Result<Vec<Photo>, String> {
    if photo_ids.is_empty() {
        return Ok(Vec::new());
    }
    let conn = connection(&app)?;
    let mut statement = conn
        .prepare(&format!(
            "SELECT {PHOTO_COLUMNS} FROM photos WHERE project_id=?1 AND id=?2 AND is_missing=0"
        ))
        .map_err(|error| error.to_string())?;
    let mut found = std::collections::HashMap::new();
    for id in &photo_ids {
        if let Ok(photo) = statement.query_row(params![project_id, id], photo_from_row) {
            found.insert(id.clone(), photo);
        }
    }
    Ok(photo_ids
        .into_iter()
        .filter_map(|id| found.remove(&id))
        .collect())
}

/// 選別の並び順。**ここが返す順序が、そのまま選別画面の並びになる。**
///
/// 似た構図は撮影時刻が近いので、撮影順に並べると「その中の1枚を選ぶ」比較が
/// 同じ場面どうしになる。以前は `ORDER BY id`（= UUID なので実質ランダム）で、
/// さらにフロント側でシャッフルしていた。
/// 撮影日時の無い写真は末尾へ回し、その中はファイル順で安定させる。
const SELECTION_SEED_ORDER: &str = " ORDER BY captured_at IS NULL, captured_at, relative_path";

/// 選別に出す写真。`rating` を指定するとその星の写真だけを返す。
/// 「★3 の 120 枚を選別する」という使い方をするので、対象は星で決まる。
#[tauri::command]
fn get_selection_seed(
    app: AppHandle,
    project_id: String,
    rating: Option<i64>,
) -> Result<Vec<SelectionSeed>, String> {
    let conn = connection(&app)?;
    let where_extra = if rating.is_some() {
        " AND rating=?2"
    } else {
        ""
    };
    let mut statement = conn
        .prepare(&format!(
            "SELECT id,rating FROM photos WHERE project_id=?1 AND is_missing=0{where_extra}{SELECTION_SEED_ORDER}"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(
                std::iter::once(rusqlite::types::Value::from(project_id.clone()))
                    .chain(rating.map(rusqlite::types::Value::from)),
            ),
            |row| {
                Ok(SelectionSeed {
                    id: row.get(0)?,
                    rating: row.get(1)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// 閾値の学習に使う、隣り合う2枚。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BurstPair {
    id: String,
    left_photo_id: String,
    right_photo_id: String,
    /// dHash のハミング距離。小さいほど構図が近い。
    distance: u32,
    gap_ms: i64,
}

/// 撮影時刻順に並んだ写真。`captured_at` と `d_hash` が揃っているものだけ。
type BurstEntry = (String, i64, String, TimestampSource);

fn load_burst_entries(app: &AppHandle, project_id: &str) -> Result<Vec<BurstEntry>, String> {
    let conn = connection(app)?;
    let mut statement = conn
        .prepare("SELECT id,captured_at,d_hash,timestamp_source FROM photos WHERE project_id=?1 AND is_missing=0 AND captured_at IS NOT NULL AND d_hash IS NOT NULL ORDER BY captured_at")
        .map_err(|error| error.to_string())?;
    let entries = statement
        .query_map(params![project_id], |row| {
            let source: Option<String> = row.get(3)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                TimestampSource::parse(source.as_deref()),
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(entries)
}

/// 閾値の学習に出題する候補ペア。構図の距離は付けるが、閾値による足切りは
/// しない。どこで切るかを決めるのがこの後の学習なので、ここで絞ってはいけない。
#[tauri::command]
fn get_burst_pairs(app: AppHandle, project_id: String) -> Result<Vec<BurstPair>, String> {
    Ok(build_burst_pairs(&load_burst_entries(&app, &project_id)?))
}

fn build_burst_pairs(entries: &[BurstEntry]) -> Vec<BurstPair> {
    let mut pairs = Vec::new();
    for window in entries.windows(2) {
        let [left, right] = window else { continue };
        let left_input = CandidateInput {
            id: left.0.clone(),
            captured_at: left.1,
            source: left.3,
        };
        let right_input = CandidateInput {
            id: right.0.clone(),
            captured_at: right.1,
            source: right.3,
        };
        if pair_eligibility(&left_input, &right_input, BURST_WINDOW_MS) != PairEligibility::Eligible
        {
            continue;
        }
        pairs.push(BurstPair {
            id: format!("{}:{}", left.0, right.0),
            left_photo_id: left.0.clone(),
            right_photo_id: right.0.clone(),
            distance: hash_distance(&left.2, &right.2),
            gap_ms: right.1 - left.1,
        });
    }
    pairs
}

/// `threshold` が `None` のときは、プロジェクトに保存された学習値、
/// それも無ければ既定の `HASH_DISTANCE_LIMIT` を使う。
#[tauri::command]
fn get_burst_groups(
    app: AppHandle,
    project_id: String,
    threshold: Option<u32>,
) -> Result<Vec<BurstGroup>, String> {
    let entries = load_burst_entries(&app, &project_id)?;
    let threshold = match threshold {
        Some(value) => value,
        None => load_burst_threshold(&app, &project_id)?.unwrap_or(HASH_DISTANCE_LIMIT),
    };
    let overrides = load_pair_overrides(&app, &project_id)?;
    Ok(build_burst_groups(
        entries
            .into_iter()
            .map(|(id, captured_at, hash, _)| (id, captured_at, hash))
            .collect(),
        threshold,
        &overrides,
    ))
}

fn load_pair_overrides(app: &AppHandle, project_id: &str) -> Result<PairOverrides, String> {
    let conn = connection(app)?;
    let mut statement = conn
        .prepare(
            "SELECT left_photo_id,right_photo_id,decision FROM burst_pair_overrides WHERE project_id=?1",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let left: String = row.get(0)?;
            let right: String = row.get(1)?;
            let decision: String = row.get(2)?;
            Ok(((left, right), decision == "join"))
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<PairOverrides, _>>()
        .map_err(|error| error.to_string())
}

/// まとまりを見直すための「1続きの写真」。指定した写真の前後 `window_ms` に入る
/// ものを撮影順で返す。まとめの判定に使う写真だけを対象にするので、
/// `load_burst_entries` と同じ絞り込み（撮影時刻とハッシュがある・欠損でない）にする。
#[tauri::command]
fn get_burst_neighborhood(
    app: AppHandle,
    project_id: String,
    photo_ids: Vec<String>,
    window_ms: Option<i64>,
) -> Result<Vec<Photo>, String> {
    if photo_ids.is_empty() {
        return Ok(Vec::new());
    }
    let conn = connection(&app)?;
    let window = window_ms.unwrap_or(BURST_WINDOW_MS).max(0);

    // 与えられた写真が占める時間の幅を求め、そこから前後へ広げる。
    let mut span: Option<(i64, i64)> = None;
    {
        let mut statement = conn
            .prepare("SELECT captured_at FROM photos WHERE project_id=?1 AND id=?2")
            .map_err(|error| error.to_string())?;
        for id in &photo_ids {
            let at: Option<i64> = statement
                .query_row(params![project_id, id], |row| row.get(0))
                .unwrap_or(None);
            let Some(at) = at else { continue };
            span = Some(match span {
                Some((low, high)) => (low.min(at), high.max(at)),
                None => (at, at),
            });
        }
    }
    let Some((low, high)) = span else {
        return Ok(Vec::new());
    };

    let mut statement = conn
        .prepare(&format!(
            "SELECT {PHOTO_COLUMNS} FROM photos
             WHERE project_id=?1 AND is_missing=0
               AND captured_at IS NOT NULL AND d_hash IS NOT NULL
               AND captured_at BETWEEN ?2 AND ?3
             ORDER BY captured_at"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, low - window, high + window], photo_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// 見直した結果のまとまりを保存する。
///
/// **受け取るのは「こう分かれていてほしい」という形だけ**で、例外そのものではない。
/// 閾値だけで出る素の判定と突き合わせ、**食い違うペアだけ**を例外として残し、
/// 一致するペアの例外は消す。こうすると、
/// - 切ってから元に戻したときに、無意味な例外が溜まらない
/// - 同じ形を何度保存しても結果が変わらない（冪等）
/// - 閾値を学習し直しても、利用者が触っていないペアは新しい閾値に従う
#[tauri::command]
fn save_burst_shape(
    app: AppHandle,
    project_id: String,
    ordered_photo_ids: Vec<String>,
    blocks: Vec<Vec<String>>,
) -> Result<(), String> {
    if ordered_photo_ids.len() < 2 {
        return Ok(());
    }
    let threshold = load_burst_threshold(&app, &project_id)?.unwrap_or(HASH_DISTANCE_LIMIT);
    let entries = load_burst_entries(&app, &project_id)?;
    let by_id: std::collections::HashMap<&str, (i64, &str)> = entries
        .iter()
        .map(|(id, at, hash, _)| (id.as_str(), (*at, hash.as_str())))
        .collect();
    let block_of: std::collections::HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .flat_map(|(index, block)| block.iter().map(move |id| (id.as_str(), index)))
        .collect();

    let mut conn = connection(&app)?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let now = now();
    for window in ordered_photo_ids.windows(2) {
        let [left, right] = window else { continue };
        let (Some(left_entry), Some(right_entry)) =
            (by_id.get(left.as_str()), by_id.get(right.as_str()))
        else {
            continue;
        };
        let raw = pair_joins_by_threshold(*left_entry, *right_entry, threshold);
        // 同じ塊に居るなら繋がっていてほしい、という意味。
        let wanted = match (block_of.get(left.as_str()), block_of.get(right.as_str())) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if wanted == raw {
            tx.execute(
                "DELETE FROM burst_pair_overrides WHERE project_id=?1 AND left_photo_id=?2 AND right_photo_id=?3",
                params![project_id, left, right],
            )
            .map_err(|error| error.to_string())?;
        } else {
            tx.execute(
                "INSERT INTO burst_pair_overrides (project_id,left_photo_id,right_photo_id,decision,updated_at)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(project_id,left_photo_id,right_photo_id)
                 DO UPDATE SET decision=excluded.decision, updated_at=excluded.updated_at",
                params![project_id, left, right, if wanted { "join" } else { "split" }, now],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 端末の写真（Android）
// ---------------------------------------------------------------------------

/// 選べる写真の出所。デスクトップは「フォルダを選ぶ」だが、Android には
/// 走査できるフォルダが無いので、**アルバム（MediaStore の bucket）を選ぶ**。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoAlbum {
    /// `create_project` にそのまま渡せる形。`mediastore://<bucketId>`
    path: String,
    name: String,
    count: i64,
}

/// NAS へ繋ぐ。**認証情報は保存しない。** メモリにしか置かないので、
/// アプリを終了すると消え、次に開くときにもう一度入れてもらう。
/// 「NAS に繋がっているときだけ使えればよい」という方針なので、保存する理由がない。
/// 前回の繋ぎ先。**パスワードだけは別の置き場から来る。**
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NasSettings {
    host: String,
    share: String,
    user: String,
    remember_password: bool,
    /// 覚えていれば入っている。覚えていなければ空。
    password: String,
    /// この環境でパスワードを預かれるか。**駄目ならトグルを出さない。**
    can_remember_password: bool,
}

/// 繋ぎ先を読み出す。ダイアログを開くたびに呼ぶ。
///
/// ホスト・共有名・利用者名は毎回入れ直すのが煩わしいだけで、秘密ではない。
/// **パスワードだけは扱いが違う**ので、覚えると選んだときに限り、
/// 端末の鍵で包んで別に預ける（SecretStore.kt）。
#[tauri::command]
fn get_nas_settings(app: AppHandle) -> Result<NasSettings, String> {
    let conn = connection(&app)?;
    let remember = read_setting(&conn, SETTING_NAS_REMEMBER).as_deref() == Some("1");
    Ok(NasSettings {
        host: read_setting(&conn, SETTING_NAS_HOST).unwrap_or_default(),
        share: read_setting(&conn, SETTING_NAS_SHARE).unwrap_or_default(),
        user: read_setting(&conn, SETTING_NAS_USER).unwrap_or_default(),
        remember_password: remember,
        password: if remember { load_nas_password() } else { String::new() },
        can_remember_password: cfg!(target_os = "android"),
    })
}

/// 繋ぎ先を覚える。remember_password が false なら**預けてあるものも消す。**
#[tauri::command]
fn save_nas_settings(
    app: AppHandle,
    host: String,
    share: String,
    user: String,
    remember_password: bool,
    password: String,
) -> Result<(), String> {
    let conn = connection(&app)?;
    for (key, value) in [
        (SETTING_NAS_HOST, host),
        (SETTING_NAS_SHARE, share),
        (SETTING_NAS_USER, user),
        (
            SETTING_NAS_REMEMBER,
            if remember_password { "1" } else { "0" }.to_string(),
        ),
    ] {
        conn.execute(
            "INSERT INTO app_settings (key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )
        .map_err(|error| error.to_string())?;
    }
    // 覚えないときは空文字を渡す。預けてあるものは SecretStore 側で消える。
    save_nas_password(if remember_password { &password } else { "" })
}

#[cfg(target_os = "android")]
fn load_nas_password() -> String {
    android_secret::load_password().unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
fn load_nas_password() -> String {
    String::new()
}

#[cfg(target_os = "android")]
fn save_nas_password(password: &str) -> Result<(), String> {
    android_secret::save_password(password)
}

#[cfg(not(target_os = "android"))]
fn save_nas_password(_password: &str) -> Result<(), String> {
    // この環境では NAS に直接繋がない（OS が共有をマウントする）。
    Ok(())
}

/// いま繋いでいる NAS のフォルダ一覧。**繋ぎ直さずに取り直せる。**
///
/// これが無かったため、画面は `connect_nas` の戻り値を持ち回るしかなく、
/// 作成ダイアログを開き直すと端末のアルバムで上書きされていた。
#[tauri::command]
fn list_nas_folders() -> Result<Vec<PhotoAlbum>, String> {
    #[cfg(target_os = "android")]
    {
        Ok(android_smb::list_folders("")?
            .into_iter()
            .map(|folder| PhotoAlbum {
                path: folder.path,
                name: folder.name,
                // 枚数は数えると全走査になる。選ぶ時点では出さない。
                count: -1,
            })
            .collect())
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("NAS はこの環境では読めません。".into())
    }
}

#[tauri::command]
fn connect_nas(
    host: String,
    share: String,
    user: String,
    password: String,
) -> Result<Vec<PhotoAlbum>, String> {
    #[cfg(target_os = "android")]
    {
        android_smb::connect(&host, &share, &user, &password)?;
        // 繋がったら、共有の直下にあるフォルダを「選べる出所」として返す。
        Ok(android_smb::list_folders("")?
            .into_iter()
            .map(|folder| PhotoAlbum {
                path: folder.path,
                name: folder.name,
                // 枚数は数えると全走査になる。選ぶ時点では出さない。
                count: -1,
            })
            .collect())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (host, share, user, password);
        Err("NAS への接続はこの環境では行えません。".into())
    }
}

#[tauri::command]
fn disconnect_nas() {
    #[cfg(target_os = "android")]
    android_smb::disconnect();
}

/// この端末で選べるアルバム。デスクトップでは常に空で、画面はフォルダ選択を出す。
#[tauri::command]
fn list_photo_albums() -> Result<Vec<PhotoAlbum>, String> {
    #[cfg(target_os = "android")]
    {
        Ok(android_photos::list_albums()?
            .into_iter()
            .map(|album| PhotoAlbum {
                path: format!("{MEDIASTORE_PREFIX}{}", album.id),
                name: album.name,
                count: album.count,
            })
            .collect())
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// 表示用画像の設定と生成
// ---------------------------------------------------------------------------

const SETTING_DISPLAY_EDGE: &str = "display_edge";

// NAS の繋ぎ先。**パスワードはここに入れない。**
const SETTING_NAS_HOST: &str = "nas_host";
const SETTING_NAS_SHARE: &str = "nas_share";
const SETTING_NAS_USER: &str = "nas_user";
const SETTING_NAS_REMEMBER: &str = "nas_remember_password";

fn read_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key=?1",
        params![key],
        |row| row.get(0),
    )
    .ok()
}

/// このプロジェクトで使う表示用の長辺。
/// **プロジェクトの上書き → 全体の既定 → 組み込みの既定**、の順に見る。
fn resolve_display_edge(app: &AppHandle, project_id: &str) -> Result<u32, String> {
    let conn = connection(app)?;
    let per_project: Option<i64> = conn
        .query_row(
            "SELECT display_edge FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(None);
    if let Some(edge) = per_project {
        return Ok(normalize_display_edge(edge));
    }
    let global = read_setting(&conn, SETTING_DISPLAY_EDGE).and_then(|v| v.parse::<i64>().ok());
    Ok(global.map_or(DISPLAY_EDGE_DEFAULT, normalize_display_edge))
}

/// 全体の既定と、選べる値。設定画面がそのまま使う。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplaySettings {
    edge: u32,
    choices: Vec<u32>,
    default_edge: u32,
    large_edge: u32,
}

#[tauri::command]
fn get_display_settings(app: AppHandle) -> Result<DisplaySettings, String> {
    let conn = connection(&app)?;
    let edge = read_setting(&conn, SETTING_DISPLAY_EDGE)
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(DISPLAY_EDGE_DEFAULT, normalize_display_edge);
    Ok(DisplaySettings {
        edge,
        choices: DISPLAY_EDGES.to_vec(),
        default_edge: DISPLAY_EDGE_DEFAULT,
        large_edge: DISPLAY_EDGE_LARGE,
    })
}

#[tauri::command]
fn save_display_edge(app: AppHandle, edge: u32) -> Result<u32, String> {
    let normalized = normalize_display_edge(edge as i64);
    connection(&app)?
        .execute(
            "INSERT INTO app_settings (key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![SETTING_DISPLAY_EDGE, normalized.to_string()],
        )
        .map_err(|error| error.to_string())?;
    Ok(normalized)
}

/// プロジェクト単位の上書き。`None` を渡すと全体の設定に戻す。
#[tauri::command]
fn save_project_display_edge(
    app: AppHandle,
    project_id: String,
    edge: Option<u32>,
) -> Result<u32, String> {
    let normalized = edge.map(|value| normalize_display_edge(value as i64));
    connection(&app)?
        .execute(
            "UPDATE projects SET display_edge=?1, updated_at=?2 WHERE id=?3",
            params![normalized.map(|v| v as i64), now(), project_id],
        )
        .map_err(|error| error.to_string())?;
    resolve_display_edge(&app, &project_id)
}

/// まだ表示用画像が要る枚数。0 なら生成を起動しない
/// （`get_analysis_backlog` と同じ考え方）。
#[tauri::command]
fn get_display_backlog(app: AppHandle, project_id: String) -> Result<i64, String> {
    let edge = resolve_display_edge(&app, &project_id)?;
    connection(&app)?
        .query_row(
            "SELECT COUNT(*) FROM photos
             WHERE project_id=?1 AND is_missing=0
               AND (display_path IS NULL OR display_edge IS NULL OR display_edge <> ?2)",
            params![project_id, edge as i64],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

/// 表示用画像をまとめて作る。**走査とは分ける。**
///
/// 表示用は原本を全部読むので、走査に混ぜると解析が桁で遅くなる
/// （Routine 1 実測: EXIF サムネイル経路 1.72ms/枚 に対しフルデコード 132ms/枚）。
/// 走査が終われば dHash は揃うので、連写のまとめと閾値学習はすぐ始められる。
/// こちらは裏で溜めていく。
fn run_display_generation(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
) -> Result<(), String> {
    let task_key = format!("display:{project_id}");
    let edge = resolve_display_edge(&app, &project_id)?;
    let dir = display_dir(&app)?;

    let pending: Vec<(String, String, Option<String>, Option<i64>)> = {
        let conn = connection(&app)?;
        let mut statement = conn
            .prepare(
                "SELECT id,path,display_path,display_edge FROM photos
                 WHERE project_id=?1 AND is_missing=0
                   AND (display_path IS NULL OR display_edge IS NULL OR display_edge <> ?2)
                 ORDER BY captured_at IS NULL, captured_at",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id, edge as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    let total = pending.len();
    progress(
        &app,
        &project_id,
        "display",
        "hashing",
        0,
        total,
        "選別用の画像を作っています…",
    );

    let mut done = 0usize;
    for (photo_id, path, stored_path, stored_edge) in pending {
        if registry.is_cancelled(&task_key) {
            progress_note(
                &app,
                &project_id,
                "display",
                "cancelled",
                done,
                total,
                format!("中断しました。{done} 件まで作成済みです。"),
                ProgressNote {
                    warning: None,
                    failed: 0,
                },
            );
            return Ok(());
        }
        let existing = match (stored_path.as_deref(), stored_edge) {
            (Some(p), Some(e)) if e > 0 => Some((Path::new(p), e as u32)),
            _ => None,
        };
        // `existing` は保存済みの表示用画像で、そちらは常にローカル。
        // **原本の方は content:// や smb:// のことがある。**
        let reference = PhotoRef::parse(&path);
        let built = build_display(reference.source().as_ref(), edge, existing);
        let file = display_file(&dir, &photo_id);
        let saved = built.and_then(|bytes| fs::write(&file, &bytes).ok().map(|_| ()));
        {
            let conn = connection(&app)?;
            if saved.is_some() {
                conn.execute(
                    "UPDATE photos SET display_path=?1, display_edge=?2 WHERE id=?3",
                    params![file.to_string_lossy().to_string(), edge as i64, photo_id],
                )
                .map_err(|error| error.to_string())?;
            } else {
                // 作れなかった写真は次回また拾えるよう、印を残さない。
                conn.execute(
                    "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                    params![photo_id],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        done += 1;
        if done % 10 == 0 || done == total {
            progress(
                &app,
                &project_id,
                "display",
                "hashing",
                done,
                total,
                "選別用の画像を作っています…",
            );
        }
    }

    progress(
        &app,
        &project_id,
        "display",
        "complete",
        total,
        total,
        "選別用の画像がそろいました。",
    );
    Ok(())
}

#[tauri::command]
fn start_display_generation(
    app: AppHandle,
    registry: State<'_, TaskRegistry>,
    project_id: String,
) -> Result<(), String> {
    let task_key = format!("display:{project_id}");
    // 既に走っていれば黙って何もしない。ボタンを二度押しても壊れないように。
    if registry.start(&task_key).is_err() {
        return Ok(());
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let result = run_display_generation(handle.clone(), &registry, project_id.clone());
        if let Err(message) = result {
            progress(&handle, &project_id, "display", "error", 0, 0, message);
        }
        registry.finish(&task_key);
    });
    Ok(())
}

/// 表示用画像を作り直す。設定を変えたときと、利用者が明示的に押したとき。
#[tauri::command]
fn reset_display_images(app: AppHandle, project_id: String) -> Result<(), String> {
    connection(&app)?
        .execute(
            "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn load_burst_threshold(app: &AppHandle, project_id: &str) -> Result<Option<u32>, String> {
    let conn = connection(app)?;
    let stored: Option<i64> = conn
        .query_row(
            "SELECT burst_threshold FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(stored.map(|value| value.clamp(0, 64) as u32))
}

#[tauri::command]
fn save_burst_threshold(app: AppHandle, project_id: String, threshold: u32) -> Result<(), String> {
    connection(&app)?
        .execute(
            "UPDATE projects SET burst_threshold=?1,burst_threshold_learned_at=?2,updated_at=?2 WHERE id=?3",
            params![threshold.min(64) as i64, now(), project_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn clear_burst_threshold(app: AppHandle, project_id: String) -> Result<(), String> {
    connection(&app)?
        .execute(
            "UPDATE projects SET burst_threshold=NULL,burst_threshold_learned_at=NULL,updated_at=?1 WHERE id=?2",
            params![now(), project_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

// グルーピング本体。DB アクセスと分けてあるのは計測ハーネス（feature = "bench"）
// から実コードそのものを呼べるようにするため。挙動は分離前と同一。
/// 手で直したまとめ。隣り合うペアに対する例外だけを持つ。
/// キーは (左の写真, 右の写真) で、**撮影順の左→右**。
pub type PairOverrides = std::collections::HashMap<(String, String), bool>;

/// 隣り合う 2 枚を、閾値だけで見たときに繋ぐか。**例外を当てる前の素の判定。**
/// `build_burst_groups` と `save_burst_shape` が同じ規則を使うよう、1 か所に出す。
fn pair_joins_by_threshold(
    left: (i64, &str),
    right: (i64, &str),
    threshold: u32,
) -> bool {
    right.0 - left.0 <= BURST_WINDOW_MS && hash_distance(right.1, left.1) <= threshold
}

fn build_burst_groups(
    entries: Vec<(String, i64, String)>,
    threshold: u32,
    overrides: &PairOverrides,
) -> Vec<BurstGroup> {
    let mut groups = Vec::new();
    let mut current: Vec<(String, i64, String)> = Vec::new();
    let mut push = |photos: &mut Vec<(String, i64, String)>| {
        if photos.len() < 2 {
            return;
        }
        let first = &photos[0];
        let last = photos.last().expect("group has a first item");
        let average = photos
            .iter()
            .skip(1)
            .map(|photo| hash_distance(&first.2, &photo.2) as f64)
            .sum::<f64>()
            / (photos.len() - 1) as f64;
        groups.push(BurstGroup {
            id: Uuid::new_v4().to_string(),
            photo_ids: photos.iter().map(|photo| photo.0.clone()).collect(),
            captured_span_ms: last.1 - first.1,
            similarity: ((1.0 - average / 64.0).max(0.0) * 100.0).round() as i64,
            accepted: None,
        });
    };
    for entry in entries {
        let is_near = current
            .last()
            .map(|previous| {
                // 利用者が手で決めた境目があれば、閾値より優先する。
                overrides
                    .get(&(previous.0.clone(), entry.0.clone()))
                    .copied()
                    .unwrap_or_else(|| {
                        pair_joins_by_threshold(
                            (previous.1, &previous.2),
                            (entry.1, &entry.2),
                            threshold,
                        )
                    })
            })
            .unwrap_or(false);
        if !current.is_empty() && !is_near {
            push(&mut current);
            current.clear();
        }
        current.push(entry);
    }
    push(&mut current);
    groups
}

#[tauri::command]
fn start_project_scan(
    app: AppHandle,
    registry: State<'_, TaskRegistry>,
    project_id: String,
) -> Result<(), String> {
    let task_key = format!("scan:{project_id}");
    registry.start(&task_key)?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let result = run_scan(handle.clone(), &registry, project_id.clone());
        if let Err(message) = result {
            progress(&handle, &project_id, "scan", "error", 0, 0, message);
        }
        registry.finish(&task_key);
    });
    Ok(())
}

#[tauri::command]
fn start_burst_analysis(
    app: AppHandle,
    registry: State<'_, TaskRegistry>,
    project_id: String,
) -> Result<(), String> {
    // 事前生成が走っていたら先に降りてもらう。`run_burst_analysis` の
    // `should_stop` が 100ms 周期で見ているので、待たずに始めてよい。
    registry.cancel(&format!("background:{project_id}"));
    spawn_analysis(app, &registry, project_id, AnalysisMode::Foreground)
}

/// scan 完了後に走らせる事前生成。「選別を開始」を押した瞬間に全件解析が
/// 始まるせいで必ず待たされる、という体験をここで消す。
/// 既に走っていれば何もしない（`TaskRegistry::start` が二重起動を弾く）。
#[tauri::command]
fn start_background_analysis(
    app: AppHandle,
    registry: State<'_, TaskRegistry>,
    project_id: String,
) -> Result<(), String> {
    match spawn_analysis(app, &registry, project_id, AnalysisMode::Background) {
        // 二重起動は事前生成では正常。呼び出し側にエラーを見せない。
        Err(_) => Ok(()),
        ok => ok,
    }
}

fn spawn_analysis(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
    mode: AnalysisMode,
) -> Result<(), String> {
    let task = mode.task();
    let task_key = format!("{task}:{project_id}");
    registry.start(&task_key)?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let result = run_burst_analysis(handle.clone(), &registry, project_id.clone(), mode);
        if let Err(message) = result {
            progress(&handle, &project_id, task, "error", 0, 0, message);
        }
        registry.finish(&task_key);
    });
    Ok(())
}

#[tauri::command]
fn cancel_project_task(registry: State<'_, TaskRegistry>, project_id: String, task: String) {
    registry.cancel(&format!("{task}:{project_id}"));
}

#[tauri::command]
fn save_project_state(
    app: AppHandle,
    project_id: String,
    state_json: String,
) -> Result<(), String> {
    connection(&app)?.execute(
        "INSERT INTO project_states (project_id,state_json,updated_at) VALUES (?1,?2,?3) ON CONFLICT(project_id) DO UPDATE SET state_json=excluded.state_json,updated_at=excluded.updated_at",
        params![project_id, state_json, now()],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn load_project_state(app: AppHandle, project_id: String) -> Result<Option<String>, String> {
    let conn = connection(&app)?;
    match conn.query_row(
        "SELECT state_json FROM project_states WHERE project_id=?1",
        params![project_id],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

// 計測ハーネス（`--features bench` の bin ターゲット）だけが使う入口。
// 既定ビルドではモジュールごと消えるので、アプリ本体の挙動には一切影響しない。
//
// ここは薄い委譲に徹していて、ロジックは一切持たない。計測対象は必ず本物の
// 関数そのものであり、そうしないと「計測したコード」と「動いているコード」が
// 別物になってしまう。private な項目は `pub use` で再公開できないため、
// 委譲関数の形をとっている。
#[cfg(feature = "bench")]
pub mod bench_api {
    use super::*;

    pub use super::{
        AnalysisOutcome, CachedAnalysis, CandidateInput, CandidateSelection, DecodeSource,
        ThumbnailState, TimestampSource,
    };

    pub const BURST_WINDOW_MS: i64 = super::BURST_WINDOW_MS;
    pub const HASH_DISTANCE_LIMIT: u32 = super::HASH_DISTANCE_LIMIT;
    pub const D_HASH_VERSION: i64 = super::D_HASH_VERSION;

    pub fn open_database(path: &Path) -> Result<Connection, String> {
        super::open_database(path)
    }

    pub fn read_captured_at(path: &Path) -> Option<i64> {
        super::read_capture_time(path).map(|capture| capture.at)
    }

    /// 撮影時刻とその出所。`timestamp_source` 列に入るのと同じ文字列を返す。
    pub fn capture_time(path: &Path) -> Option<(i64, &'static str)> {
        super::read_capture_time(path).map(|capture| (capture.at, capture.source.as_str()))
    }

    pub fn d_hash(path: &Path) -> Option<String> {
        super::d_hash(path)
    }

    /// アプリ本体と同じ、サムネイルキャッシュ込みの1枚ぶんの解析。
    pub fn analyse_photo(
        thumbnail_dir: &Path,
        photo_id: &str,
        source: &Path,
        current: Option<(i64, i64)>,
        cached: &CachedAnalysis,
    ) -> AnalysisOutcome {
        super::analyse_photo(
            thumbnail_dir,
            photo_id,
            &super::LocalPhoto(source),
            current,
            cached,
        )
    }

    /// どのデコード経路が使われるかだけを調べる（サムネイルは書かない）。
    pub fn decode_source(path: &Path) -> Option<&'static str> {
        super::decode_hash_source(path).map(|(_, source)| source.as_str())
    }

    pub fn select_burst_candidates(records: &[CandidateInput]) -> CandidateSelection {
        super::select_burst_candidates(records)
    }

    pub fn analysis_worker_count() -> usize {
        super::analysis_worker_count()
    }

    /// 本物の並列エンジン（`run_in_parallel`）で、指定した worker 数だけ使って
    /// 全枚数を解析する。worker が呼ぶのはアプリ本体と**同じ** `hash_one`。
    /// 返すのは (確定件数, 失敗件数, timeout件数)。
    pub fn analyse_in_parallel(
        thumbnail_dir: &Path,
        photos: &[(String, String)],
        workers: usize,
    ) -> (usize, usize, usize) {
        let jobs: Vec<super::HashRecord> = photos
            .iter()
            .map(|(id, path)| super::HashRecord {
                id: id.clone(),
                path: path.clone(),
                captured_at: 0,
                source: TimestampSource::ExifOriginal,
                cached: CachedAnalysis::default(),
            })
            .collect();
        let thumbnails = thumbnail_dir.to_path_buf();
        let mut failed = 0usize;
        let outcome = super::run_in_parallel(
            std::sync::Arc::new(jobs),
            workers,
            std::time::Duration::from_millis(super::PHOTO_TIMEOUT_MS),
            &|| false,
            move |index, record: &super::HashRecord| super::hash_one(&thumbnails, index, record),
            &mut |item| {
                if item.error.is_some() {
                    failed += 1;
                }
                Ok(())
            },
        )
        .expect("run analysis workers");
        (outcome.completed, failed, outcome.timed_out)
    }

    pub fn fingerprint(path: &Path) -> Option<(i64, i64)> {
        super::fingerprint(path)
    }

    pub fn hash_distance(left: &str, right: &str) -> u32 {
        super::hash_distance(left, right)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_photo(
        conn: &Connection,
        project_id: &str,
        absolute: &str,
        relative: &str,
        name: &str,
        mtime: Option<i64>,
        size: Option<i64>,
    ) -> Result<(), String> {
        super::upsert_photo(conn, project_id, absolute, relative, name, mtime, size)
    }

    /// 本物のグルーピングを走らせ、グループごとの photo_ids だけを返す。
    /// `BurstGroup` の各フィールドを公開せずに件数と規模を測れる。
    pub fn build_burst_groups(
        entries: Vec<(String, i64, String)>,
        threshold: u32,
    ) -> Vec<Vec<String>> {
        // 計測では手の入った例外を当てない。素の閾値だけを測る。
        super::build_burst_groups(entries, threshold, &super::PairOverrides::new())
            .into_iter()
            .map(|group| group.photo_ids)
            .collect()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(TaskRegistry::default())
        // 原本を画面へ渡す口。**convertFileSrc ではファイルしか渡せない。**
        //
        // 拡大表示は原本を出すが、Android では原本が content:// や smb:// で、
        // ファイルパスではない。asset プロトコルに渡しても解決できず、画面には
        // 壊れた画像が出る（実機で確認）。ここを通せば、指し先の種類に関わらず
        // PhotoSource が読んでくれる。
        .register_asynchronous_uri_scheme_protocol("photo", |_app, request, responder| {
            let target = photo_scheme_target(request.uri().path(), request.uri().query());
            std::thread::spawn(move || {
                responder.respond(match photo_scheme_response(&target) {
                    Some(response) => response,
                    None => tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new())
                        .expect("404 は必ず組める"),
                });
            });
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            delete_project,
            get_analysis_backlog,
            get_project_photo_page,
            get_photos_by_ids,
            get_selection_seed,
            save_selection_results,
            reset_selection_results,
            move_rating,
            get_selection_summary,
            export_by_rating,
            write_ratings_to_files,
            list_photo_albums,
            connect_nas,
            list_nas_folders,
            get_project_prep,
            get_nas_settings,
            save_nas_settings,
            disconnect_nas,
            get_display_settings,
            save_display_edge,
            save_project_display_edge,
            get_display_backlog,
            start_display_generation,
            reset_display_images,
            get_burst_groups,
            get_burst_neighborhood,
            save_burst_shape,
            get_burst_pairs,
            save_burst_threshold,
            clear_burst_threshold,
            start_project_scan,
            start_burst_analysis,
            start_background_analysis,
            cancel_project_task,
            save_project_state,
            load_project_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running Photo Curator");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_the_legacy_photo_schema_before_creating_indexes() {
        let directory =
            std::env::temp_dir().join(format!("photo-curator-schema-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("legacy.sqlite3");

        let legacy = Connection::open(&path).expect("open legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE projects (
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                   photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                   created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE photos (
                   id TEXT PRIMARY KEY, project_id TEXT NOT NULL, path TEXT NOT NULL,
                   relative_path TEXT NOT NULL, name TEXT NOT NULL, captured_at INTEGER,
                   d_hash TEXT, rating INTEGER NOT NULL DEFAULT 0,
                   UNIQUE(project_id, path)
                 );",
            )
            .expect("create legacy schema");
        drop(legacy);

        let migrated = open_database(&path).expect("migrate legacy database");
        assert!(has_column(&migrated, "photos", "fingerprint_mtime").unwrap());
        assert!(has_column(&migrated, "photos", "fingerprint_size").unwrap());
        assert!(has_column(&migrated, "photos", "is_missing").unwrap());
        let visible_index: i64 = migrated
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='photos_project_visible'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(visible_index, 1);
        drop(migrated);

        // Running migrations on every command must remain safe and idempotent.
        open_database(&path).expect("reopen migrated database");
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    fn test_directory(label: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("photo-curator-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        directory
    }

    // -----------------------------------------------------------------------
    // 画像・EXIF のフィクスチャ
    //
    // デコード経路の分岐は「EXIF サムネイルが実在するか」で決まる。合成画像を
    // 置くだけでは②の経路を一度も通らないので、APP1 に本物の TIFF ブロックを
    // 組み立てて埋め込む。
    // -----------------------------------------------------------------------

    fn synthetic_image(width: u32, height: u32, seed: u8) -> DynamicImage {
        let mut buffer = image::RgbImage::new(width, height);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            // 一様な塗りだと resize 後に全画素が同値になり dHash が縮退する。
            let value = ((x * 7 + y * 13) % 256) as u8;
            *pixel = image::Rgb([value, value.wrapping_add(seed), 255 - value]);
        }
        DynamicImage::ImageRgb8(buffer)
    }

    fn jpeg_bytes(width: u32, height: u32, seed: u8) -> Vec<u8> {
        encode_thumbnail(&synthetic_image(width, height, seed)).expect("encode fixture jpeg")
    }

    enum Val {
        Ascii(String),
        /// SHORT 1個。4バイトの値欄に収まるので inline に置く。
        Short(u16),
        ExifPointer,
        ThumbOffset,
        ThumbLength,
    }

    fn ifd_size(entries: usize) -> usize {
        2 + 12 * entries + 4
    }

    fn resolve_entries(
        entries: &[(u16, Val)],
        data: &mut Vec<u8>,
        data_off: usize,
        exif_off: u32,
        thumb: (u32, u32),
    ) -> Vec<(u16, u16, u32, u32)> {
        entries
            .iter()
            .map(|(tag, value)| match value {
                Val::ExifPointer => (*tag, 4u16, 1u32, exif_off),
                Val::Short(value) => (*tag, 3, 1, u32::from(*value)),
                Val::ThumbOffset => (*tag, 4, 1, thumb.0),
                Val::ThumbLength => (*tag, 4, 1, thumb.1),
                Val::Ascii(text) => {
                    let mut bytes = text.as_bytes().to_vec();
                    bytes.push(0);
                    assert!(bytes.len() > 4, "4バイト以下の ASCII は inline 格納になる");
                    let offset = (data_off + data.len()) as u32;
                    data.extend_from_slice(&bytes);
                    if data.len() % 2 == 1 {
                        data.push(0);
                    }
                    (*tag, 2, bytes.len() as u32, offset)
                }
            })
            .collect()
    }

    fn write_ifd(out: &mut Vec<u8>, entries: &[(u16, u16, u32, u32)], next: u32) {
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&next.to_le_bytes());
    }

    /// IFD0（Orientation・DateTime）/ Exif サブIFD（DateTimeOriginal・
    /// OffsetTimeOriginal）/ IFD1（Orientation・サムネイル）を持つ TIFF ブロックを
    /// 組む。リトルエンディアン。**IFD の項目はタグの昇順**に並べる（規格の要求）。
    fn tiff_block(
        datetime: Option<&str>,
        datetime_original: Option<&str>,
        offset_original: Option<&str>,
        orientation: Option<u16>,
        thumbnail_orientation: Option<u16>,
        thumbnail: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut exif_entries = Vec::new();
        if let Some(value) = datetime_original {
            exif_entries.push((0x9003, Val::Ascii(value.to_owned())));
        }
        if let Some(value) = offset_original {
            exif_entries.push((0x9011, Val::Ascii(value.to_owned())));
        }
        let mut ifd0_entries = Vec::new();
        if let Some(value) = orientation {
            ifd0_entries.push((0x0112, Val::Short(value)));
        }
        if let Some(value) = datetime {
            ifd0_entries.push((0x0132, Val::Ascii(value.to_owned())));
        }
        if !exif_entries.is_empty() {
            ifd0_entries.push((0x8769, Val::ExifPointer));
        }
        let mut ifd1_entries = Vec::new();
        if let Some(value) = thumbnail_orientation {
            ifd1_entries.push((0x0112, Val::Short(value)));
        }
        if thumbnail.is_some() {
            ifd1_entries.push((0x0201, Val::ThumbOffset));
            ifd1_entries.push((0x0202, Val::ThumbLength));
        }

        let ifd0_off = 8usize;
        let exif_off = ifd0_off + ifd_size(ifd0_entries.len());
        let ifd1_off = exif_off
            + if exif_entries.is_empty() {
                0
            } else {
                ifd_size(exif_entries.len())
            };
        let data_off = ifd1_off
            + if ifd1_entries.is_empty() {
                0
            } else {
                ifd_size(ifd1_entries.len())
            };

        // ASCII を先に敷き、そのあとにサムネイル本体を置く。
        let mut data = Vec::new();
        let ifd0 = resolve_entries(&ifd0_entries, &mut data, data_off, exif_off as u32, (0, 0));
        let exif = resolve_entries(&exif_entries, &mut data, data_off, exif_off as u32, (0, 0));
        let thumb = match thumbnail {
            Some(bytes) => {
                let offset = (data_off + data.len()) as u32;
                data.extend_from_slice(bytes);
                (offset, bytes.len() as u32)
            }
            None => (0, 0),
        };
        let ifd1 = resolve_entries(&ifd1_entries, &mut data, data_off, exif_off as u32, thumb);

        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&(ifd0_off as u32).to_le_bytes());
        write_ifd(
            &mut out,
            &ifd0,
            if ifd1_entries.is_empty() {
                0
            } else {
                ifd1_off as u32
            },
        );
        if !exif_entries.is_empty() {
            write_ifd(&mut out, &exif, 0);
        }
        if !ifd1_entries.is_empty() {
            write_ifd(&mut out, &ifd1, 0);
        }
        assert_eq!(
            out.len(),
            data_off,
            "IFD の配置と計算した offset が一致する"
        );
        out.extend_from_slice(&data);
        out
    }

    /// SOI の直後に APP1(Exif) を差し込んだ JPEG を書き出す。
    fn write_jpeg_with_exif(path: &Path, body: &[u8], tiff: Option<&[u8]>) {
        let mut out = Vec::new();
        out.extend_from_slice(&body[..2]); // SOI
        if let Some(tiff) = tiff {
            out.extend_from_slice(&[0xFF, 0xE1]);
            out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
            out.extend_from_slice(b"Exif\0\0");
            out.extend_from_slice(tiff);
        }
        out.extend_from_slice(&body[2..]);
        fs::write(path, out).expect("write jpeg fixture");
    }

    struct Fixture {
        datetime: Option<String>,
        datetime_original: Option<String>,
        offset_original: Option<String>,
        /// IFD0 の Orientation。本体の画素に対する向きの指定。
        orientation: Option<u16>,
        /// IFD1 の Orientation。埋め込みサムネイルを既に正立させて保存する
        /// カメラを再現するために、IFD0 とは別に指定できる。
        thumbnail_orientation: Option<u16>,
        thumbnail: Option<(u32, u32)>,
        size: (u32, u32),
        seed: u8,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                datetime: None,
                datetime_original: None,
                offset_original: None,
                orientation: None,
                thumbnail_orientation: None,
                thumbnail: None,
                size: (600, 400),
                seed: 0,
            }
        }
    }

    impl Fixture {
        fn write(&self, path: &Path) {
            let body = jpeg_bytes(self.size.0, self.size.1, self.seed);
            let thumbnail = self
                .thumbnail
                .map(|(width, height)| jpeg_bytes(width, height, self.seed));
            let has_exif = self.datetime.is_some()
                || self.datetime_original.is_some()
                || self.orientation.is_some()
                || thumbnail.is_some();
            let tiff = has_exif.then(|| {
                tiff_block(
                    self.datetime.as_deref(),
                    self.datetime_original.as_deref(),
                    self.offset_original.as_deref(),
                    self.orientation,
                    self.thumbnail_orientation,
                    thumbnail.as_deref(),
                )
            });
            write_jpeg_with_exif(path, &body, tiff.as_deref());
        }
    }

    // 旧ビルドの行は fingerprint が NULL で、そのままではハッシュキャッシュが
    // 一切効かない。補完のついでに解析結果を壊してはいけない。
    #[test]
    fn backfills_missing_fingerprints_without_discarding_analysis() {
        let directory = test_directory("backfill");
        let photo = directory.join("photo.jpg");
        fs::write(&photo, b"stand-in for a real photo").expect("write photo");
        let database = directory.join("legacy.sqlite3");

        {
            let conn = open_database(&database).expect("open database");
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES ('photo-1','project-1',?1,'photo.jpg','photo.jpg',1234567890,'aabbccddeeff0011',3,NULL,NULL,0)",
                params![photo.to_string_lossy().to_string()],
            )
            .expect("insert legacy row");
        }

        // 再接続のたびに backfill が走る。
        let conn = open_database(&database).expect("reopen database");
        let (captured_at, hash, rating, mtime, size): (
            Option<i64>,
            Option<String>,
            i64,
            Option<i64>,
            Option<i64>,
        ) = conn
            .query_row(
                "SELECT captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size FROM photos WHERE id='photo-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("read migrated row");

        assert_eq!(captured_at, Some(1_234_567_890), "captured_at を失わない");
        assert_eq!(
            hash.as_deref(),
            Some("aabbccddeeff0011"),
            "d_hash を失わない"
        );
        assert_eq!(rating, 3, "rating を失わない");
        assert!(mtime.is_some(), "fingerprint_mtime が補完される");
        assert_eq!(
            size,
            Some(fs::metadata(&photo).expect("photo metadata").len() as i64),
            "fingerprint_size が実ファイルと一致する"
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 以前は fingerprint の比較に `=` を使っていたため、NULL が絡むと常に偽と
    // なり、再 scan のたびに captured_at と d_hash が消えていた。
    #[test]
    fn rescanning_an_unchanged_photo_keeps_its_analysis() {
        let directory = test_directory("rescan");
        let photo = directory.join("photo.jpg");
        fs::write(&photo, b"original contents").expect("write photo");
        let database = directory.join("scan.sqlite3");
        let conn = open_database(&database).expect("open database");
        let absolute = photo.to_string_lossy().to_string();
        let (mtime, size) = fingerprint(&photo).expect("fingerprint");

        let scan = |mtime: Option<i64>, size: Option<i64>| {
            upsert_photo(
                &conn,
                "project-1",
                &absolute,
                "photo.jpg",
                "photo.jpg",
                mtime,
                size,
            )
            .expect("scan photo");
        };
        let analysis = || -> (Option<i64>, Option<String>) {
            conn.query_row(
                "SELECT captured_at,d_hash FROM photos WHERE path=?1",
                params![absolute],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read analysis")
        };

        scan(Some(mtime), Some(size));
        conn.execute(
            "UPDATE photos SET captured_at=111,d_hash='0f0f0f0f0f0f0f0f' WHERE path=?1",
            params![absolute],
        )
        .expect("record analysis");

        // 中身が変わっていない写真の解析結果は保持される。
        scan(Some(mtime), Some(size));
        let (captured_at, hash) = analysis();
        assert_eq!(captured_at, Some(111), "再 scan で captured_at を失わない");
        assert_eq!(
            hash.as_deref(),
            Some("0f0f0f0f0f0f0f0f"),
            "再 scan で d_hash を失わない"
        );

        // ファイルが変わったときは解析結果を破棄する。
        scan(Some(mtime), Some(size + 1));
        let (captured_at, hash) = analysis();
        assert_eq!(captured_at, None, "内容が変われば captured_at を破棄する");
        assert_eq!(hash, None, "内容が変われば d_hash を破棄する");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // metadata が読めない写真は fingerprint が両方 NULL になる。SQL の `=` は
    // `NULL = NULL` すら真にしないため、以前はこの手の写真の解析結果が再 scan の
    // たびに破棄されていた。`IS` に変えたことを直接検証する。
    #[test]
    fn rescanning_a_photo_without_readable_metadata_keeps_its_analysis() {
        let directory = test_directory("rescan-null");
        let database = directory.join("scan.sqlite3");
        let conn = open_database(&database).expect("open database");
        let absolute = "C:/photos/unreadable.jpg";

        upsert_photo(
            &conn,
            "project-1",
            absolute,
            "unreadable.jpg",
            "unreadable.jpg",
            None,
            None,
        )
        .expect("first scan");
        conn.execute(
            "UPDATE photos SET captured_at=222,d_hash='1122334455667788' WHERE path=?1",
            params![absolute],
        )
        .expect("record analysis");

        upsert_photo(
            &conn,
            "project-1",
            absolute,
            "unreadable.jpg",
            "unreadable.jpg",
            None,
            None,
        )
        .expect("second scan");

        let (captured_at, hash): (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT captured_at,d_hash FROM photos WHERE path=?1",
                params![absolute],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read analysis");
        assert_eq!(captured_at, Some(222), "NULL 同士でも captured_at を保つ");
        assert_eq!(
            hash.as_deref(),
            Some("1122334455667788"),
            "NULL 同士でも d_hash を保つ"
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 解析全体をひとつのトランザクションで囲んでいた頃は、キャンセルすると
    // すべて rollback され、何度やり直しても d_hash が1件も残らなかった。
    #[test]
    fn cancelling_keeps_the_chunks_that_were_already_committed() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let directory = test_directory("chunks");
        let database = directory.join("chunks.sqlite3");
        let conn = open_database(&database).expect("open database");

        let total = ANALYSIS_CHUNK_SIZE * 2 + 50;
        let ids: Vec<String> = (0..total).map(|index| format!("photo-{index}")).collect();
        for (index, id) in ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES (?1,'project-1',?2,?3,?3,NULL,NULL,0,NULL,NULL,0)",
                params![id, format!("C:/photos/{index}.jpg"), format!("{index}.jpg")],
            )
            .expect("insert photo");
        }

        // チャンク境界(100)を越えた 150 件目の直前でキャンセルする。
        let cancel_after = ANALYSIS_CHUNK_SIZE + 50;
        let checks = AtomicUsize::new(0);
        let outcome = commit_in_chunks(
            &conn,
            &ids,
            &|| checks.fetch_add(1, Ordering::SeqCst) >= cancel_after,
            &mut |tx: &Connection, id: &String, _index: usize| {
                tx.execute(
                    "UPDATE photos SET d_hash='ffffffffffffffff' WHERE id=?1",
                    params![id],
                )
                .map_err(|error| error.to_string())?;
                Ok(())
            },
        )
        .expect("run chunked processing");

        assert!(outcome.cancelled, "キャンセルが伝わる");
        assert_eq!(
            outcome.committed, ANALYSIS_CHUNK_SIZE,
            "確定済みのチャンクだけが残る"
        );

        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("count hashed photos");
        assert_eq!(
            stored, ANALYSIS_CHUNK_SIZE as i64,
            "commit 済みチャンクの成果は残り、処理中チャンクだけが破棄される"
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // Step 5: EXIF 撮影時刻の解釈
    // -----------------------------------------------------------------------

    // 基準値は `date -u -d "..." +%s` で外部から取ったもの。
    const JUNE_30_2026_UTC_MS: i64 = 1_782_843_572_000; // 2026-06-30 18:19:32Z
    const FEB_29_2024_UTC_MS: i64 = 1_709_208_000_000; // 2024-02-29 12:00:00Z
    const JAN_15_2026_UTC_MS: i64 = 1_768_435_200_000; // 2026-01-15 00:00:00Z

    #[test]
    fn parses_representative_exif_timestamps() {
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32", None),
            Some(JUNE_30_2026_UTC_MS),
            "代表フォーマットを UTC の実値として解釈する"
        );
        // 旧実装は 1970 年以降のすべての日付が1日ぶん手前にずれていた
        // （`-(year-1901)/100` が 2100 年以外でも 1 を引いていたため）。
        assert_eq!(
            exif_timestamp_ms(b"2026:01:15 00:00:00", None),
            Some(JAN_15_2026_UTC_MS),
            "年始の日付でも1日ずれない"
        );
        assert_eq!(
            exif_timestamp_ms(b"2024:02:29 12:00:00", None),
            Some(FEB_29_2024_UTC_MS),
            "うるう日を正しく扱う"
        );
        // サブ秒つき（19バイトより長い）も先頭19バイトで解釈できる。
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32.500", None),
            Some(JUNE_30_2026_UTC_MS)
        );
    }

    #[test]
    fn applies_the_exif_time_zone_offset() {
        let naive = exif_timestamp_ms(b"2026:06:30 18:19:32", None).expect("naive");
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"+09:00")),
            Some(naive - 9 * 3_600_000),
            "JST は UTC より9時間進んでいる"
        );
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"-05:00")),
            Some(naive + 5 * 3_600_000),
            "負のオフセットも符号どおりに効く"
        );
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"      ")),
            Some(naive),
            "空欄のオフセットは無視して日時だけ使う"
        );
        assert_eq!(
            exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"junk!!")),
            Some(naive),
            "壊れたオフセットで日時まで捨てない"
        );
    }

    #[test]
    fn rejects_broken_exif_timestamps() {
        // 旧実装は数字を拾うだけで範囲を見なかったため、これらが
        // 「それらしい値」に化けるか、黙って mtime fallback を誘発していた。
        for broken in [
            &b"2026:13:30 18:19:32"[..], // 13月
            &b"2026:02:30 18:19:32"[..], // 2月30日
            &b"2025:02:29 18:19:32"[..], // 平年の2月29日
            &b"2026:06:00 18:19:32"[..], // 0日
            &b"2026:06:30 25:19:32"[..], // 25時
            &b"2026:06:30 18:60:32"[..], // 60分
            &b"1899:06:30 18:19:32"[..], // 範囲外の年
            &b"    :  :     :  :  "[..], // 空欄
            &b"2026-06-30 18:19:32"[..], // 区切りが規格外
            &b"garbage"[..],             // 論外
            &b""[..],
        ] {
            assert_eq!(
                exif_timestamp_ms(broken, None),
                None,
                "壊れた値を受け入れてしまった: {:?}",
                String::from_utf8_lossy(broken)
            );
        }
        // うるう秒を書く機材があるので 60 秒だけは通す。
        assert!(exif_timestamp_ms(b"2026:06:30 18:19:60", None).is_some());
    }

    #[test]
    fn infers_capture_time_from_common_filename_shapes() {
        let expect = Some(JUNE_30_2026_UTC_MS);
        for name in [
            "2026-06-30_18-19-32.jpg",
            "20260630_181932.jpg",
            "IMG_20260630_181932.jpg",
            "20260630181932.jpg",
            "2026-06-30 18.19.32.jpg",
            "photo_2026-06-30_18-19-32_1.jpg",
        ] {
            assert_eq!(
                filename_capture_time(Path::new(name)),
                expect,
                "ファイル名から読めなかった: {name}"
            );
        }
        for name in [
            "DSC_0001.jpg",
            "photo.jpg",
            "2026.jpg",
            "9999-99-99_99-99-99.jpg", // 数字は並んでいるが暦として不正
            "IMG_20261340_181932.jpg", // 13月40日
        ] {
            assert_eq!(
                filename_capture_time(Path::new(name)),
                None,
                "撮影時刻でないものを読んでしまった: {name}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step 5: timestamp_source の分類
    // -----------------------------------------------------------------------

    #[test]
    fn classifies_the_source_of_every_capture_time() {
        let directory = test_directory("timestamp-source");

        let original = directory.join("original.jpg");
        Fixture {
            datetime_original: Some("2026:06:30 18:19:32".into()),
            offset_original: Some("+09:00".into()),
            ..Default::default()
        }
        .write(&original);
        let capture = read_capture_time(&original).expect("read original");
        assert_eq!(capture.source, TimestampSource::ExifOriginal);
        assert_eq!(capture.at, JUNE_30_2026_UTC_MS - 9 * 3_600_000);

        // DateTimeOriginal が無く DateTime だけの写真は経路が変わる。
        let fallback = directory.join("datetime-only.jpg");
        Fixture {
            datetime: Some("2026:06:30 18:19:32".into()),
            ..Default::default()
        }
        .write(&fallback);
        let capture = read_capture_time(&fallback).expect("read datetime-only");
        assert_eq!(capture.source, TimestampSource::ExifDateTime);
        assert_eq!(capture.at, JUNE_30_2026_UTC_MS);

        // EXIF が無くてもファイル名が残っていれば mtime よりは強い。
        let named = directory.join("2026-06-30_18-19-32.jpg");
        Fixture::default().write(&named);
        let capture = read_capture_time(&named).expect("read named");
        assert_eq!(capture.source, TimestampSource::FilenameInferred);
        assert_eq!(capture.at, JUNE_30_2026_UTC_MS);

        // 何の手がかりも無ければ最後の手段として mtime。
        let bare = directory.join("DSC_0001.jpg");
        Fixture::default().write(&bare);
        let capture = read_capture_time(&bare).expect("read bare");
        assert_eq!(capture.source, TimestampSource::FilesystemMtime);
        assert_eq!(capture.at, fingerprint(&bare).expect("fingerprint").0);

        // 壊れた EXIF は「読めなかった」として次の経路へ落ちる。
        let broken = directory.join("broken-exif.jpg");
        Fixture {
            datetime_original: Some("2026:13:40 99:99:99".into()),
            ..Default::default()
        }
        .write(&broken);
        assert_eq!(
            read_capture_time(&broken).expect("read broken").source,
            TimestampSource::FilesystemMtime
        );

        assert!(
            read_capture_time(&directory.join("missing.jpg")).is_none(),
            "存在しないファイルでは何も返さない"
        );

        // 保存した文字列がそのまま読み戻せる（DB 往復の形）。
        for source in [
            TimestampSource::ExifOriginal,
            TimestampSource::ExifDateTime,
            TimestampSource::FilenameInferred,
            TimestampSource::FilesystemMtime,
            TimestampSource::Unknown,
        ] {
            assert_eq!(TimestampSource::parse(Some(source.as_str())), source);
        }
        assert_eq!(TimestampSource::parse(None), TimestampSource::Unknown);
        assert_eq!(
            TimestampSource::parse(Some("将来の値")),
            TimestampSource::Unknown
        );

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // Step 5: 候補爆発の抑制
    // -----------------------------------------------------------------------

    fn candidates(entries: &[(i64, TimestampSource)]) -> Vec<CandidateInput> {
        entries
            .iter()
            .enumerate()
            .map(|(index, (captured_at, source))| CandidateInput {
                id: format!("photo-{index}"),
                captured_at: *captured_at,
                source: *source,
            })
            .collect()
    }

    // コピーやダウンロードで数千枚が同一 mtime になるのは普通のことで、
    // それを連写の根拠にすると全件が候補になり、全件デコードが走る。
    #[test]
    fn an_mtime_flood_does_not_explode_the_candidate_set() {
        let flood = candidates(
            &(0..500)
                .map(|_| (1_700_000_000_000, TimestampSource::FilesystemMtime))
                .collect::<Vec<_>>(),
        );
        let selection = select_burst_candidates(&flood);
        assert!(
            selection.ids.is_empty(),
            "mtime 由来だけで 500 枚が候補になった: {}",
            selection.ids.len()
        );
        assert_eq!(selection.weak_pairs_skipped, 499);
        assert!(!selection.narrowed, "除外で足りるので窓を狭める必要はない");

        // 経路が不明な行も同じ扱い。旧ビルドの行が一斉に候補化しない。
        let unknown = candidates(
            &(0..500)
                .map(|index| (1_700_000_000_000 + index * 10, TimestampSource::Unknown))
                .collect::<Vec<_>>(),
        );
        assert!(select_burst_candidates(&unknown).ids.is_empty());

        // 片側が EXIF なら根拠として成立するので候補に残す。
        let mixed = candidates(&[
            (0, TimestampSource::ExifOriginal),
            (1_000, TimestampSource::FilesystemMtime),
            (50_000, TimestampSource::FilesystemMtime),
            (50_100, TimestampSource::FilesystemMtime),
        ]);
        let selection = select_burst_candidates(&mixed);
        assert_eq!(
            selection.ids,
            HashSet::from(["photo-0".to_owned(), "photo-1".to_owned()]),
            "EXIF と隣り合う mtime だけが候補になる"
        );
    }

    #[test]
    fn narrows_the_window_only_when_the_candidate_set_is_actually_expensive() {
        // 3秒間隔（4秒窓では全部が隣接）を、実コストが問題になる規模まで並べる。
        let bulk = CANDIDATE_COUNT_LIMIT + 400;
        let mut entries: Vec<(i64, TimestampSource)> = (0..bulk as i64)
            .map(|index| (index * 3_000, TimestampSource::ExifOriginal))
            .collect();
        // 本物の連写。窓をどこまで詰めても残るべき2枚。
        let far = (bulk as i64 + 100) * 3_000;
        entries.push((far, TimestampSource::ExifOriginal));
        entries.push((far + 100, TimestampSource::ExifOriginal));
        let records = candidates(&entries);

        let wide = candidates_within(&records, BURST_WINDOW_MS).0;
        assert_eq!(wide.len(), bulk + 2, "4秒窓では全枚数が候補になる（前提）");

        let selection = select_burst_candidates(&records);
        assert!(selection.narrowed, "候補が {bulk} 件でも窓が狭まらなかった");
        assert!(selection.window_ms < BURST_WINDOW_MS);
        assert_eq!(
            selection.ids,
            HashSet::from([format!("photo-{bulk}"), format!("photo-{}", bulk + 1)]),
            "本当に近い2枚だけが残る"
        );
    }

    #[test]
    fn a_dense_small_project_keeps_its_full_window() {
        // Routine 2 の直接的な回帰テスト。実データ271枚は撮影間隔がほぼ全て
        // 1〜4秒で候補率が 98.5% になる。比率だけで縮小を判断していた頃は
        // ここで発火し、連写グループを 52 → 16 に減らしていた。
        // 数百枚ぶんのデコードは Step 4 後で 1秒未満。窓を詰める理由が無い。
        let records = candidates(
            &(0..271)
                .map(|index| (index * 2_000, TimestampSource::ExifOriginal))
                .collect::<Vec<_>>(),
        );
        let selection = select_burst_candidates(&records);
        assert!(
            selection.ratio > CANDIDATE_RATIO_LIMIT,
            "候補率が高い状況であることの確認: {}",
            selection.ratio
        );
        assert!(
            !selection.narrowed,
            "密に撮影された数百枚で窓が狭まってしまった"
        );
        assert_eq!(selection.window_ms, BURST_WINDOW_MS);
        assert_eq!(selection.ids.len(), 271, "連写候補が1枚も失われない");
    }

    #[test]
    fn window_narrowing_stops_at_the_floor() {
        // 全枚数が 0.1 秒間隔。どこまで詰めても候補率も件数も下がらない。
        let total = CANDIDATE_COUNT_LIMIT + 400;
        let records = candidates(
            &(0..total as i64)
                .map(|index| (index * 100, TimestampSource::ExifOriginal))
                .collect::<Vec<_>>(),
        );
        let selection = select_burst_candidates(&records);
        assert_eq!(
            selection.window_ms, MIN_BURST_WINDOW_MS,
            "下限で止まる（無限に詰め続けない）"
        );
        assert!(selection.narrowed);
        assert_eq!(selection.ids.len(), total, "本物の連写は候補のまま残る");
    }

    #[test]
    fn an_empty_or_single_photo_project_selects_nothing() {
        assert!(select_burst_candidates(&[]).ids.is_empty());
        let single = candidates(&[(0, TimestampSource::ExifOriginal)]);
        let selection = select_burst_candidates(&single);
        assert!(selection.ids.is_empty());
        assert!(!selection.narrowed);
    }

    // -----------------------------------------------------------------------
    // Step 4: デコード経路とサムネイルキャッシュ
    // -----------------------------------------------------------------------

    #[test]
    fn picks_the_cheapest_decode_path_that_works() {
        let directory = test_directory("decode-path");

        // ① EXIF サムネイルがあればそれで済ませる。
        let with_thumbnail = directory.join("with-thumbnail.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            ..Default::default()
        }
        .write(&with_thumbnail);
        assert_eq!(
            decode_hash_source(&with_thumbnail).map(|(_, source)| source),
            Some(DecodeSource::ExifThumbnail)
        );

        // ② サムネイルが無い JPEG は 1/8 デコードに落ちる。
        let plain = directory.join("plain.jpg");
        Fixture::default().write(&plain);
        assert_eq!(
            decode_hash_source(&plain).map(|(_, source)| source),
            Some(DecodeSource::JpegScaled)
        );

        // ③ 小さすぎるサムネイルは使わず、次の経路へ落とす。
        let tiny = directory.join("tiny-thumbnail.jpg");
        Fixture {
            thumbnail: Some((32, 24)),
            ..Default::default()
        }
        .write(&tiny);
        assert_eq!(
            decode_hash_source(&tiny).map(|(_, source)| source),
            Some(DecodeSource::JpegScaled)
        );

        // ④ JPEG でなければフルデコードしかない。
        let png = directory.join("plain.png");
        synthetic_image(300, 200, 3).save(&png).expect("write png");
        assert_eq!(
            decode_hash_source(&png).map(|(_, source)| source),
            Some(DecodeSource::FullDecode)
        );

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // Routine 10: 解析の入口をバイト列にする
    // -----------------------------------------------------------------------

    /// 読んだ量を数える `PhotoSource`。**部分読みが効いているか**を測るためだけの実装。
    struct CountingSource {
        bytes: Vec<u8>,
        name: String,
        served: std::cell::Cell<usize>,
        all_calls: std::cell::Cell<usize>,
    }

    impl CountingSource {
        fn new(bytes: Vec<u8>, name: &str) -> Self {
            Self {
                bytes,
                name: name.to_owned(),
                served: std::cell::Cell::new(0),
                all_calls: std::cell::Cell::new(0),
            }
        }
    }

    impl PhotoSource for CountingSource {
        fn head(&self, want: usize) -> Option<Vec<u8>> {
            let end = want.min(self.bytes.len());
            self.served.set(self.served.get() + end);
            Some(self.bytes[..end].to_vec())
        }
        fn all(&self) -> Option<Vec<u8>> {
            self.all_calls.set(self.all_calls.get() + 1);
            self.served.set(self.served.get() + self.bytes.len());
            Some(self.bytes.clone())
        }
        fn fingerprint(&self) -> Option<(i64, i64)> {
            Some((1_700_000_000_000, self.bytes.len() as i64))
        }
        fn name(&self) -> Option<String> {
            Some(self.name.clone())
        }
    }

    /// **これが Android で NAS を読めるかどうかの分かれ目。**
    /// EXIF サムネイルがある写真は、原本の全体を一度も要求してはいけない。
    #[test]
    fn the_exif_thumbnail_path_never_asks_for_the_whole_file() {
        let directory = test_directory("partial-read");

        let with_thumbnail = directory.join("with-thumbnail.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            size: (4000, 3000),
            ..Default::default()
        }
        .write(&with_thumbnail);
        let bytes = fs::read(&with_thumbnail).expect("read fixture");
        let total = bytes.len();
        let source = CountingSource::new(bytes, "with-thumbnail.jpg");

        let (_, decode) = decode_hash_source_from(&source).expect("decode");
        assert_eq!(decode, DecodeSource::ExifThumbnail);
        assert_eq!(
            source.all_calls.get(),
            0,
            "EXIF サムネイルで済むのに全体を読んでいる"
        );
        assert!(
            source.served.get() <= EXIF_HEAD_PROBE,
            "先頭 {EXIF_HEAD_PROBE} バイトを超えて読んでいる（{} / 全体 {total}）",
            source.served.get()
        );

        // 逆に、EXIF サムネイルが無ければ全体が要る。ここを読まないと画像にならない。
        let plain = directory.join("plain.jpg");
        Fixture {
            size: (4000, 3000),
            ..Default::default()
        }
        .write(&plain);
        let plain_source = CountingSource::new(fs::read(&plain).expect("read"), "plain.jpg");
        let (_, decode) = decode_hash_source_from(&plain_source).expect("decode");
        assert_eq!(decode, DecodeSource::JpegScaled);
        assert_eq!(plain_source.all_calls.get(), 1, "全体を 1 回だけ読む");

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    /// EXIF が無いときに、`PhotoSource::name()` が撮影時刻の手がかりになること。
    /// Android の `content://` はパスを持たないので、名前を別途もらう必要がある。
    #[test]
    fn the_capture_time_falls_back_to_the_name_then_the_fingerprint() {
        let bytes = jpeg_bytes(64, 48, 0);

        let named = CountingSource::new(bytes.clone(), "2026-06-30_18-19-32.jpg");
        let capture = read_capture_time_from(&named).expect("capture time");
        assert_eq!(capture.source, TimestampSource::FilenameInferred);

        // 名前も手がかりにならなければ fingerprint（mtime）へ落ちる。
        let plain = CountingSource::new(bytes, "photo.jpg");
        let capture = read_capture_time_from(&plain).expect("capture time");
        assert_eq!(capture.source, TimestampSource::FilesystemMtime);
        assert_eq!(capture.at, 1_700_000_000_000);
    }

    // -----------------------------------------------------------------------
    // Routine 11: 写真の指し先
    // -----------------------------------------------------------------------

    /// 走査元は「フォルダ」だけではない。
    ///
    /// **実機で最初に踏んだ穴がこれ。** `create_project` が `folder_path` を
    /// ディレクトリだと決めつけて `is_dir()` で検査しており、Android で
    /// アルバムを選ぶと必ず「写真フォルダが見つかりません」になっていた。
    /// 走査側は前置きを見分けていたのに、入口の検査だけが取り残されていた。
    #[test]
    fn a_scan_source_is_not_always_a_directory_on_disk() {
        // 端末のフォルダ。ここは実在を確かめてよい。
        assert!(is_local_folder(r"C:UsersmiyajPictures"));
        assert!(is_local_folder("/home/user/photos"));
        // UNC も「フォルダ」。Windows が解決する。
        assert!(is_local_folder(r"\LS710DBD2Sharephotos"));

        // 以下はパスではないので、実在を確かめてはいけない。
        assert!(!is_local_folder("mediastore://12345"));
        assert!(!is_local_folder("smb://192.168.11.8/Share/photos"));
        // まだ無い出所（SAF）も、前置きを足さずに通る。
        assert!(!is_local_folder("tree://content%3A%2F%2Fcom.example"));
    }

    /// **既存の行を壊さないことが要点。** DB には絶対パスがそのまま入っており、
    /// 判別を間違えると全部が「読めない写真」になる。
    #[test]
    fn a_photo_reference_tells_local_paths_from_content_uris() {
        let local = |raw: &str| matches!(PhotoRef::parse(raw), PhotoRef::Local(_));
        let content = |raw: &str| matches!(PhotoRef::parse(raw), PhotoRef::Content(_));

        // 既存の行はすべてローカル扱い。
        assert!(local(r"C:\Users\miyaj\Pictures\a.jpg"));
        assert!(local("/home/user/photos/a.jpg"));
        assert!(local(r"\\nas\share\a.jpg"));
        // 紛らわしい名前でも、前置きが違えばローカル。
        assert!(local("contents://not-a-uri.jpg"));
        assert!(local("content:/single-slash.jpg"));

        assert!(content("content://media/external/images/media/1234"));

        let smb = |raw: &str| matches!(PhotoRef::parse(raw), PhotoRef::Smb(_));
        assert!(smb("smb://192.168.11.10/photos/2026/a.jpg"));
        // 紛らわしいが前置きが違う。
        assert!(local("smbx://host/share/a.jpg"));
        assert!(local(r"\\host\share\a.jpg"));

        // 往復して同じ文字列に戻る（DB へ書き戻すときに崩れない）。
        for raw in [
            r"C:\a\b.jpg",
            "content://media/external/images/media/1",
            "smb://192.168.11.10/photos/2026/a.jpg",
        ] {
            let restored = match PhotoRef::parse(raw) {
                PhotoRef::Local(path) => path.to_string_lossy().to_string(),
                PhotoRef::Content(uri) => uri,
                PhotoRef::Smb(url) => url,
            };
            assert_eq!(restored, raw);
        }
    }

    /// Android 以外で content:// が来ても、落ちずに「読めない」で済むこと。
    #[cfg(not(target_os = "android"))]
    #[test]
    fn a_content_uri_is_simply_unreadable_off_android() {
        let reference = PhotoRef::parse("content://media/external/images/media/1");
        let source = reference.source();
        assert!(source.head(1024).is_none());
        assert!(source.all().is_none());
        assert!(source.fingerprint().is_none());
        // 解析に掛けても panic せず、失敗として返る。
        assert!(decode_hash_source_from(source.as_ref()).is_none());
    }

    // -----------------------------------------------------------------------
    // Routine 10b: 表示用サイズ
    // -----------------------------------------------------------------------

    #[test]
    fn the_display_edge_is_clamped_to_the_offered_choices() {
        for edge in DISPLAY_EDGES {
            assert_eq!(normalize_display_edge(edge as i64), edge);
        }
        // 表に無い値・負・極端な値は既定へ。生成する画像が暴れないように。
        for broken in [0, -1, 999, 4096, i64::MAX] {
            assert_eq!(normalize_display_edge(broken), DISPLAY_EDGE_DEFAULT);
        }
    }

    /// **下げるときに原本を読み直さないこと。**
    /// 1536 → 1024 の切り替えで 2,000 枚ぶん 13.4GB を読むのは無駄でしかない。
    #[test]
    fn shrinking_the_display_size_reuses_the_saved_image() {
        let directory = test_directory("display-size");
        let photo = directory.join("photo.jpg");
        Fixture {
            size: (4000, 3000),
            ..Default::default()
        }
        .write(&photo);

        // まず大きい方を作る。ここは原本を読む。
        let source = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
        let large = build_display(&source, 1536, None).expect("build large");
        assert_eq!(source.all_calls.get(), 1, "初回は原本が要る");
        let stored = directory.join("display-1536.jpg");
        fs::write(&stored, &large).expect("write display");

        // 次に小さい方へ。**保存済みを縮めるだけで、原本には戻らない。**
        let shrink = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
        let small = build_display(&shrink, 1024, Some((&stored, 1536))).expect("build small");
        assert_eq!(
            shrink.all_calls.get(),
            0,
            "下げるだけなのに原本を読み直している"
        );
        let decoded = image::load_from_memory(&small).expect("decode");
        assert_eq!(decoded.width().max(decoded.height()), 1024);

        // 逆に上げるときは、小さい画像から大きい画像は作れないので原本へ戻る。
        let grow = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
        let bigger = build_display(&grow, 1536, Some((&stored, 1024))).expect("build bigger");
        assert_eq!(grow.all_calls.get(), 1, "上げるときは原本が要る");
        let decoded = image::load_from_memory(&bigger).expect("decode");
        assert_eq!(decoded.width().max(decoded.height()), 1536);

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    /// 原本より大きくは引き伸ばさない。情報は増えないのに容量だけ増える。
    #[test]
    fn the_display_image_never_upscales_the_original() {
        let small = synthetic_image(320, 240, 7);
        let bytes = encode_display(&small, 1536).expect("encode");
        let decoded = image::load_from_memory(&bytes).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (320, 240));
    }

    // -----------------------------------------------------------------------
    // Routine 5: EXIF Orientation
    // -----------------------------------------------------------------------

    /// 画素を行ごとに取り出す。回転や反転の結果を並びそのままで比べる。
    fn grid(image: &DynamicImage) -> Vec<Vec<u8>> {
        (0..image.height())
            .map(|y| {
                (0..image.width())
                    .map(|x| image.get_pixel(x, y).0[0])
                    .collect()
            })
            .collect()
    }

    /// 3x2 の非対称な画像。値はすべて異なるので、取り違えれば必ず落ちる。
    ///
    /// ```text
    /// 1 2 3
    /// 4 5 6
    /// ```
    fn asymmetric() -> DynamicImage {
        let mut image = image::RgbImage::new(3, 2);
        for y in 0..2u32 {
            for x in 0..3u32 {
                let value = (1 + x + y * 3) as u8;
                image.put_pixel(x, y, image::Rgb([value, value, value]));
            }
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn every_exif_orientation_maps_to_its_own_transform() {
        let expected: [(u16, Vec<Vec<u8>>); 8] = [
            (1, vec![vec![1, 2, 3], vec![4, 5, 6]]),
            (2, vec![vec![3, 2, 1], vec![6, 5, 4]]),
            (3, vec![vec![6, 5, 4], vec![3, 2, 1]]),
            (4, vec![vec![4, 5, 6], vec![1, 2, 3]]),
            (5, vec![vec![1, 4], vec![2, 5], vec![3, 6]]),
            (6, vec![vec![4, 1], vec![5, 2], vec![6, 3]]),
            (7, vec![vec![6, 3], vec![5, 2], vec![4, 1]]),
            (8, vec![vec![3, 6], vec![2, 5], vec![1, 4]]),
        ];
        for (orientation, want) in expected {
            assert_eq!(
                grid(&apply_orientation(asymmetric(), orientation)),
                want,
                "Orientation {orientation} の変換が違う"
            );
        }

        // 0 と 9 は規格外。壊れた EXIF で画像を回さない。
        for broken in [0u16, 9, 65535] {
            assert_eq!(
                grid(&apply_orientation(asymmetric(), broken)),
                grid(&asymmetric()),
                "規格外の値 {broken} で画像を回してしまった"
            );
        }
    }

    // 一覧が横倒しになっていた原因そのもの。サムネイルの生成経路で
    // Orientation を焼き込まないと、原本を直接見る選別画面とだけ向きがずれる。
    #[test]
    fn the_thumbnail_source_comes_back_upright() {
        let directory = test_directory("orientation");

        // ① EXIF サムネイルが無い JPEG（1/8 デコード経路）。IFD0 の指定に従う。
        let rotated = directory.join("rotated.jpg");
        Fixture {
            size: (600, 400),
            orientation: Some(6),
            ..Default::default()
        }
        .write(&rotated);
        let (image, source) = decode_hash_source(&rotated).expect("decode");
        assert_eq!(source, DecodeSource::JpegScaled);
        assert!(
            image.height() > image.width(),
            "横長のまま返っている（{}x{}）",
            image.width(),
            image.height()
        );

        // ② 同じ写真から Orientation を外すと、回らない。①が「たまたま縦長」
        //    ではないことの裏取り。
        let upright = directory.join("upright.jpg");
        Fixture {
            size: (600, 400),
            datetime: Some("2026:06:30 18:19:32".into()),
            ..Default::default()
        }
        .write(&upright);
        let (image, _) = decode_hash_source(&upright).expect("decode");
        assert!(image.width() > image.height(), "回すべきでない画像を回した");

        // ③ EXIF サムネイル経路でも焼き込む。
        let with_thumbnail = directory.join("with-thumbnail.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            orientation: Some(6),
            ..Default::default()
        }
        .write(&with_thumbnail);
        let (image, source) = decode_hash_source(&with_thumbnail).expect("decode");
        assert_eq!(source, DecodeSource::ExifThumbnail);
        assert_eq!(
            (image.width(), image.height()),
            (120, 160),
            "埋め込みサムネイルに Orientation が効いていない"
        );

        // ④ 埋め込みサムネイルを既に正立させて保存するカメラ。IFD1 が 1 なので、
        //    IFD0 が 6 でも回してはいけない（回すと二重になる）。
        let pre_rotated = directory.join("pre-rotated-thumbnail.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            orientation: Some(6),
            thumbnail_orientation: Some(1),
            ..Default::default()
        }
        .write(&pre_rotated);
        let (image, source) = decode_hash_source(&pre_rotated).expect("decode");
        assert_eq!(source, DecodeSource::ExifThumbnail);
        assert_eq!(
            (image.width(), image.height()),
            (160, 120),
            "IFD1 の指定を無視して二重に回している"
        );

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn reuses_the_cached_thumbnail_until_the_photo_changes() {
        let directory = test_directory("thumbnail-cache");
        let thumbnails = directory.join("thumbnails");
        let photo = directory.join("photo.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            ..Default::default()
        }
        .write(&photo);

        let run = |cached: &CachedAnalysis| {
            analyse_photo(&thumbnails, "photo-1", &LocalPhoto(&photo), fingerprint(&photo), cached)
        };
        let store = |outcome: &AnalysisOutcome, photo: &Path| {
            let (mtime, size) = fingerprint(photo).expect("fingerprint");
            CachedAnalysis {
                d_hash: outcome.d_hash.clone(),
                d_hash_version: outcome.d_hash.as_ref().map(|_| D_HASH_VERSION),
                thumbnail_path: outcome.thumbnail_path.clone(),
                thumbnail_mtime: outcome.thumbnail_path.as_ref().map(|_| mtime),
                thumbnail_size: outcome.thumbnail_path.as_ref().map(|_| size),
                thumbnail_version: outcome.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
            }
        };

        // --- ミス: 何も無い状態からは作る -----------------------------------
        let first = run(&CachedAnalysis::default());
        assert_eq!(
            first.thumbnail_state,
            ThumbnailState::Generated(DecodeSource::ExifThumbnail)
        );
        assert!(!first.hash_reused);
        let hash = first.d_hash.clone().expect("hash");
        let file = thumbnail_file(&thumbnails, "photo-1");
        assert!(file.is_file(), "サムネイルが保存されていない");
        assert_eq!(
            first.thumbnail_path.as_deref(),
            Some(&*file.to_string_lossy())
        );
        // dHash も表示も同じ1枚を使う。キャッシュ経路と単発計算が一致すること。
        assert_eq!(d_hash(&photo), Some(hash.clone()));

        // --- ヒット: 2回目はデコードしない -----------------------------------
        let cached = store(&first, &photo);
        let second = run(&cached);
        assert_eq!(second.thumbnail_state, ThumbnailState::Hit);
        assert!(second.hash_reused, "デコードし直している");
        assert_eq!(second.d_hash, Some(hash.clone()));

        // --- ハッシュ方式が変わったとき: 原本には戻らず、サムネイルから引き直す -
        let stale = CachedAnalysis {
            d_hash_version: Some(D_HASH_VERSION - 1),
            ..cached.clone()
        };
        let rehashed = run(&stale);
        assert_eq!(
            rehashed.thumbnail_state,
            ThumbnailState::Hit,
            "サムネイルは作り直さない"
        );
        assert!(!rehashed.hash_reused);
        assert_eq!(
            rehashed.d_hash,
            Some(hash.clone()),
            "同じサムネイルからは必ず同じハッシュが出る"
        );

        // --- 生成方式が変わったとき: 原本まで戻って作り直す --------------------
        // d_hash の版とは扱いが違う。あちらは保存済みのサムネイルから引き直せば
        // 足りるが、こちらは**サムネイルの中身そのもの**が古いので作り直す。
        let old_format = CachedAnalysis {
            thumbnail_version: Some(THUMBNAIL_VERSION - 1),
            ..cached.clone()
        };
        let rebuilt = run(&old_format);
        assert!(
            matches!(rebuilt.thumbnail_state, ThumbnailState::Generated(_)),
            "生成方式が変わったのに古いサムネイルを使い回している"
        );

        // --- 無効化: サムネイルのファイルが消えたら作り直す -------------------
        fs::remove_file(&file).expect("remove thumbnail");
        let regenerated = run(&cached);
        assert!(matches!(
            regenerated.thumbnail_state,
            ThumbnailState::Generated(_)
        ));
        assert_eq!(regenerated.d_hash, Some(hash.clone()));

        // --- 無効化: 写真そのものが差し替わったら fingerprint がずれる ---------
        Fixture {
            thumbnail: Some((160, 120)),
            seed: 90,
            size: (640, 480),
            ..Default::default()
        }
        .write(&photo);
        let changed = run(&cached);
        assert!(
            matches!(changed.thumbnail_state, ThumbnailState::Generated(_)),
            "写真が変わってもキャッシュを使い回してしまった"
        );
        assert_ne!(
            changed.d_hash,
            Some(hash),
            "別の写真なのに同じハッシュが出ている"
        );

        // --- fingerprint が読めないときはキャッシュを信用しない ---------------
        let unreadable = analyse_photo(&thumbnails, "photo-1", &LocalPhoto(&photo), None, &cached);
        assert!(matches!(
            unreadable.thumbnail_state,
            ThumbnailState::Generated(_)
        ));

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 1枚の失敗で解析全体が止まると、フォルダに壊れたファイルが1つ混ざった
    // だけでプロジェクトが使えなくなる。
    #[test]
    fn a_single_unreadable_photo_does_not_stop_the_others() {
        let directory = test_directory("decode-failures");
        let thumbnails = directory.join("thumbnails");

        let good = directory.join("good.jpg");
        Fixture {
            thumbnail: Some((160, 120)),
            ..Default::default()
        }
        .write(&good);

        // 拡張子は JPEG だが中身が壊れている。
        let truncated = directory.join("truncated.jpg");
        let mut bytes = jpeg_bytes(600, 400, 5);
        bytes.truncate(40);
        fs::write(&truncated, &bytes).expect("write truncated");

        // JPEG ですらない。
        let not_an_image = directory.join("notes.jpg");
        fs::write(&not_an_image, b"this is not an image at all").expect("write text");

        // 空ファイル。
        let empty = directory.join("empty.jpg");
        fs::write(&empty, b"").expect("write empty");

        // 存在しない・開けない（ディレクトリを写真として渡す）。
        let missing = directory.join("missing.jpg");
        let as_directory = directory.join("thumbnails");

        let mut succeeded = 0usize;
        let mut failed = 0usize;
        for path in [
            &good,
            &truncated,
            &not_an_image,
            &empty,
            &missing,
            &as_directory,
        ] {
            let outcome = analyse_photo(
                &thumbnails,
                &format!("id-{}", path.display()),
                &LocalPhoto(path),
                fingerprint(path),
                &CachedAnalysis::default(),
            );
            match outcome.d_hash {
                Some(_) => succeeded += 1,
                None => {
                    failed += 1;
                    assert_eq!(outcome.thumbnail_state, ThumbnailState::Failed);
                    assert!(outcome.thumbnail_path.is_none());
                }
            }
        }
        assert_eq!(succeeded, 1, "読める写真が処理されていない");
        assert_eq!(failed, 5, "読めない写真が成功扱いになっている");

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // Step 6: 並列化・timeout・エラー耐性・キャンセル・task registry
    // -----------------------------------------------------------------------

    /// テスト用の仕事。`work` に渡す 1 件ぶん。
    #[derive(Clone, Debug)]
    struct FakeJob {
        id: String,
        /// この仕事が居座る時間。timeout の検証に使う。
        delay: Duration,
    }

    impl PhotoJob for FakeJob {
        fn photo_id(&self) -> &str {
            &self.id
        }
    }

    fn fake_jobs(count: usize) -> Vec<FakeJob> {
        (0..count)
            .map(|index| FakeJob {
                id: format!("photo-{index}"),
                delay: Duration::ZERO,
            })
            .collect()
    }

    /// photos 行を count 件だけ用意した DB を作る。
    fn database_with_photos(directory: &Path, count: usize) -> Connection {
        let conn = open_database(&directory.join("test.sqlite3")).expect("open database");
        for index in 0..count {
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES (?1,'project-1',?2,?3,?3,NULL,NULL,0,NULL,NULL,0)",
                params![
                    format!("photo-{index}"),
                    format!("C:/photos/{index}.jpg"),
                    format!("{index}.jpg")
                ],
            )
            .expect("insert photo");
        }
        conn
    }

    // 並列に読んだ結果を単一の writer が書く構造が、重複も欠落も起こさないこと。
    // ここが崩れると「解析したはずの写真が消える」「同じ写真が二重に数えられる」
    // という、利用者からは再現できない壊れ方をする。
    #[test]
    fn parallel_workers_write_every_row_exactly_once() {
        let directory = test_directory("parallel-write");
        let total = ANALYSIS_CHUNK_SIZE * 3 + 37;
        let conn = database_with_photos(&directory, total);

        let mut pending: Vec<PhotoWork> = Vec::new();
        let mut committed = 0usize;
        let mut seen: Vec<String> = Vec::new();
        let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
            // d_hash に自分の index を入れる。取り違えがあれば後で分かる。
            tx.execute(
                "UPDATE photos SET d_hash=?1 WHERE id=?2",
                params![format!("{:016x}", item.index), item.photo_id],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        };

        let outcome = run_in_parallel(
            Arc::new(fake_jobs(total)),
            4,
            Duration::from_secs(30),
            &|| false,
            |index, job: &FakeJob| {
                // 一瞬で返る仕事だと worker が実際には重ならず、共有カーソルの
                // 競合が表に出ない。数十マイクロ秒だけ居座らせて、4本が本当に
                // 同時に次の番号を取りに来る状況を作る。
                let until = Instant::now() + Duration::from_micros(50);
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
                PhotoWork::new(index, &job.id)
            },
            &mut |item| {
                seen.push(item.photo_id.clone());
                pending.push(item);
                if pending.len() >= ANALYSIS_CHUNK_SIZE {
                    committed += flush_results(&conn, &mut pending, &apply)?;
                }
                Ok(())
            },
        )
        .expect("run workers");
        committed += flush_results(&conn, &mut pending, &apply).expect("final flush");

        assert!(!outcome.cancelled);
        assert_eq!(outcome.completed, total, "全件が writer に届く");
        assert_eq!(outcome.timed_out, 0);
        assert_eq!(committed, total, "全件が確定する");

        let unique: HashSet<&String> = seen.iter().collect();
        assert_eq!(unique.len(), total, "同じ写真が二度 writer に届いている");

        // DB 側でも、全行が「自分の index」を持っていること。
        let rows: Vec<(String, Option<String>)> = {
            let mut statement = conn
                .prepare("SELECT id,d_hash FROM photos")
                .expect("prepare verification");
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("query");
            rows.collect::<Result<Vec<_>, _>>().expect("collect")
        };
        assert_eq!(rows.len(), total);
        for (id, hash) in rows {
            let index: usize = id.trim_start_matches("photo-").parse().expect("parse id");
            assert_eq!(
                hash,
                Some(format!("{index:016x}")),
                "{id} の結果が別の写真のものになっている"
            );
        }

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 異常に遅い1枚で全体が停滞しないこと。Rust では走っているデコードを
    // 安全に中断できないので、遅い1枚は「失敗として確定させて先へ進む」。
    #[test]
    fn a_slow_photo_times_out_while_the_rest_finish() {
        let total = 12;
        let mut jobs = fake_jobs(total);
        // worker 数より多く居座らせて、全 worker が同時に詰まる状況も通す。
        for index in [3usize, 7] {
            jobs[index].delay = Duration::from_secs(30);
        }

        let mut results: Vec<PhotoWork> = Vec::new();
        let started = Instant::now();
        let outcome = run_in_parallel(
            Arc::new(jobs),
            4,
            Duration::from_millis(300),
            &|| false,
            |index, job: &FakeJob| {
                std::thread::sleep(job.delay);
                PhotoWork::new(index, &job.id)
            },
            &mut |item| {
                results.push(item);
                Ok(())
            },
        )
        .expect("run workers");

        assert_eq!(outcome.completed, total, "残りが完走していない");
        assert_eq!(
            outcome.timed_out, 2,
            "遅い2枚が timeout として確定していない"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "遅い1枚に全体が引きずられている: {:?}",
            started.elapsed()
        );

        let failed: Vec<&PhotoWork> = results.iter().filter(|r| r.error.is_some()).collect();
        assert_eq!(failed.len(), 2);
        for item in failed {
            assert!(
                item.error.as_deref().unwrap_or_default().contains("秒以内"),
                "timeout の理由が記録されていない: {:?}",
                item.error
            );
        }
        // 遅くない 10 枚は成功として届く。
        assert_eq!(results.iter().filter(|r| r.error.is_none()).count(), 10);
    }

    // 解析できなかった写真が DB に残り、その件数がそのまま UI へ渡ること。
    #[test]
    fn analysis_errors_are_recorded_and_counted() {
        let directory = test_directory("analysis-errors");
        let conn = database_with_photos(&directory, 6);

        let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
            tx.execute(
                "UPDATE photos SET d_hash=?1,analysis_error=?2,analysis_error_at=?3 WHERE id=?4",
                params![
                    item.d_hash,
                    item.error,
                    item.error.as_ref().map(|_| now()),
                    item.photo_id
                ],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        };
        let mut pending: Vec<PhotoWork> = Vec::new();
        let mut failed = 0usize;
        run_in_parallel(
            Arc::new(fake_jobs(6)),
            2,
            Duration::from_secs(30),
            &|| false,
            |index, job: &FakeJob| {
                let mut result = PhotoWork::new(index, &job.id);
                // 2枚に1枚を失敗させる。
                if index % 2 == 0 {
                    result.error = Some("画像を読み取れませんでした。".into());
                } else {
                    result.d_hash = Some("ffffffffffffffff".into());
                }
                result
            },
            &mut |item| {
                if item.error.is_some() {
                    failed += 1;
                }
                pending.push(item);
                Ok(())
            },
        )
        .expect("run workers");
        flush_results(&conn, &mut pending, &apply).expect("flush");

        assert_eq!(failed, 3, "失敗件数の集計が合っていない");
        assert_eq!(
            failed_photo_count(&conn, "project-1").expect("count failures"),
            3,
            "UI に渡す件数が DB の実態と食い違っている"
        );
        // 失敗しても他は普通に解析されている＝続行可能。
        let hashed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("count hashed");
        assert_eq!(hashed, 3, "失敗が他の写真の解析を巻き込んでいる");

        // 成功に転じたらエラーは消える（居座らない）。
        let recovered = vec![{
            let mut item = PhotoWork::new(0, "photo-0");
            item.d_hash = Some("0123456789abcdef".into());
            item
        }];
        let mut recovered = recovered;
        flush_results(&conn, &mut recovered, &apply).expect("flush recovery");
        assert_eq!(
            failed_photo_count(&conn, "project-1").expect("recount"),
            2,
            "解析に成功しても古いエラーが残り続けている"
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // キャンセルが素早く効き、かつそれまでに確定した分が残ること。
    #[test]
    fn cancelling_responds_quickly_and_keeps_committed_work() {
        let directory = test_directory("parallel-cancel");
        let total = 400;
        let conn = database_with_photos(&directory, total);

        let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
            tx.execute(
                "UPDATE photos SET d_hash='ffffffffffffffff' WHERE id=?1",
                params![item.photo_id],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        };

        let cancel = Arc::new(AtomicBool::new(false));
        let mut pending: Vec<PhotoWork> = Vec::new();
        let mut committed = 0usize;
        let watcher = cancel.clone();
        let requested = Mutex::new(None::<Instant>);

        let outcome = run_in_parallel(
            Arc::new(fake_jobs(total)),
            2,
            Duration::from_secs(30),
            &|| watcher.load(Ordering::SeqCst),
            |index, job: &FakeJob| {
                // 1枚 5ms。400枚で 2 秒ぶんの仕事。
                std::thread::sleep(Duration::from_millis(5));
                PhotoWork::new(index, &job.id)
            },
            &mut |item| {
                pending.push(item);
                if pending.len() >= ANALYSIS_CHUNK_SIZE {
                    committed += flush_results(&conn, &mut pending, &apply)?;
                    // 最初のチャンクが確定した直後にキャンセルする。
                    if committed == ANALYSIS_CHUNK_SIZE {
                        *requested.lock().expect("lock") = Some(Instant::now());
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
                Ok(())
            },
        )
        .expect("run workers");
        committed += flush_results(&conn, &mut pending, &apply).expect("final flush");

        let elapsed = requested
            .lock()
            .expect("lock")
            .expect("キャンセルが要求されていない")
            .elapsed();
        assert!(outcome.cancelled, "キャンセルが伝わっていない");
        assert!(
            elapsed < Duration::from_millis(500),
            "キャンセルの反応が遅い: {elapsed:?}"
        );
        assert!(
            committed >= ANALYSIS_CHUNK_SIZE,
            "確定済みの分が失われている"
        );
        assert!(committed < total, "キャンセルしたのに全件処理されている");

        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(
            stored as usize, committed,
            "DB の実態と「保存済み」の件数が食い違っている"
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 二重起動の防止と、複数スレッドから同時に start された場合の勝者が
    // ちょうど1つであること。
    #[test]
    fn the_task_registry_admits_exactly_one_runner() {
        let registry = Arc::new(TaskRegistry::default());
        let key = "burst:project-1";

        assert!(registry.start(key).is_ok());
        assert!(
            registry.start(key).is_err(),
            "同じプロジェクトの解析が二重に始まってしまう"
        );
        assert!(registry.is_running(key));
        // 別プロジェクトは独立している。
        assert!(registry.start("burst:project-2").is_ok());
        registry.finish(key);
        registry.finish("burst:project-2");
        assert!(!registry.is_running(key));

        // 32 スレッドから一斉に start しても、通るのは1つだけ。
        let winners = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(AtomicBool::new(false));
        let handles: Vec<_> = (0..32)
            .map(|_| {
                let registry = registry.clone();
                let winners = winners.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    while !gate.load(Ordering::SeqCst) {
                        std::hint::spin_loop();
                    }
                    if registry.start(key).is_ok() {
                        winners.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        gate.store(true, Ordering::SeqCst);
        for handle in handles {
            handle.join().expect("join");
        }
        assert_eq!(
            winners.load(Ordering::SeqCst),
            1,
            "同時 start の勝者が1つでない"
        );
    }

    // start より先に届いたキャンセルが、次の実行を巻き添えにしないこと。
    // ここが崩れると「開始した瞬間に中断される」という再現困難な不具合になる。
    #[test]
    fn a_stale_cancel_does_not_kill_the_next_run() {
        let registry = TaskRegistry::default();
        let key = "burst:project-1";

        registry.start(key).expect("start");
        registry.cancel(key);
        assert!(registry.is_cancelled(key));
        registry.finish(key);

        registry.start(key).expect("restart");
        assert!(
            !registry.is_cancelled(key),
            "前回のキャンセルが次の実行に持ち越されている"
        );
        registry.finish(key);

        // 走っていないタスクへのキャンセルも、次の start が拾い上げて消す。
        registry.cancel(key);
        registry.start(key).expect("start after stray cancel");
        assert!(!registry.is_cancelled(key));
        registry.finish(key);
    }

    // 事前生成は前面の解析に道を譲る。両方が同じ写真を取り合わないこと。
    #[test]
    fn the_background_pass_yields_to_the_foreground_one() {
        let registry = TaskRegistry::default();
        let background = format!("{}:project-1", AnalysisMode::Background.task());
        let foreground = format!("{}:project-1", AnalysisMode::Foreground.task());

        registry.start(&background).expect("start background");
        // run_burst_analysis の should_stop と同じ判定。
        let should_stop = |mode: AnalysisMode, key: &str| {
            registry.is_cancelled(key)
                || (mode == AnalysisMode::Background && registry.is_running(&foreground))
        };
        assert!(
            !should_stop(AnalysisMode::Background, &background),
            "前面が走っていないのに事前生成が止まっている"
        );

        registry.start(&foreground).expect("start foreground");
        assert!(
            should_stop(AnalysisMode::Background, &background),
            "前面が始まっても事前生成が居座っている"
        );
        assert!(
            !should_stop(AnalysisMode::Foreground, &foreground),
            "前面が自分自身を止めてしまっている"
        );

        registry.finish(&foreground);
        registry.finish(&background);
    }

    // worker 数の既定は控えめに、上書きは範囲内に丸める。
    #[test]
    fn the_worker_count_stays_within_its_bounds() {
        let workers = analysis_worker_count();
        assert!(
            (MIN_ANALYSIS_WORKERS..=MAX_ANALYSIS_WORKERS).contains(&workers),
            "既定の worker 数が範囲外: {workers}"
        );
        // 事前生成は必ず 1 本。前面の操作を邪魔しないため。
        assert_eq!(AnalysisMode::Background.workers(), 1);
    }

    // migration は利用者の実データに触れるため、合成データだけでなく実物の
    // コピーに対しても流して無傷を確かめられるようにしておく。
    // 環境変数 PHOTO_CURATOR_REAL_DB に既存DBのパスを渡すと実行される。
    //   PHOTO_CURATOR_REAL_DB=... cargo test --manifest-path src-tauri/Cargo.toml
    // 元のDBは読むだけで、書き込みは一時ディレクトリのコピーに対してのみ行う。
    #[test]
    fn migrating_a_real_database_copy_preserves_every_row() {
        let Ok(source) = std::env::var("PHOTO_CURATOR_REAL_DB") else {
            eprintln!("PHOTO_CURATOR_REAL_DB が未設定のため skip");
            return;
        };
        let source = PathBuf::from(source);
        assert!(source.is_file(), "指定されたDBが見つからない: {source:?}");

        let directory = test_directory("real-db");
        let copy = directory.join("copy.sqlite3");
        fs::copy(&source, &copy).expect("copy database");

        // migration 前のスナップショット。
        let before: Vec<(String, Option<i64>, Option<String>, i64)> = {
            let conn = Connection::open(&copy).expect("open copy");
            let mut statement = conn
                .prepare("SELECT id,captured_at,d_hash,rating FROM photos ORDER BY id")
                .expect("prepare snapshot");
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query snapshot");
            rows.collect::<Result<Vec<_>, _>>()
                .expect("collect snapshot")
        };

        // 本番と同じ経路で migration + backfill を走らせる。
        let conn = open_database(&copy).expect("migrate real database copy");

        let after: Vec<(String, Option<i64>, Option<String>, i64)> = {
            let mut statement = conn
                .prepare("SELECT id,captured_at,d_hash,rating FROM photos ORDER BY id")
                .expect("prepare verification");
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query verification");
            rows.collect::<Result<Vec<_>, _>>()
                .expect("collect verification")
        };

        assert_eq!(before.len(), after.len(), "行数が変わらない");
        assert_eq!(
            before, after,
            "captured_at / d_hash / rating が1件も書き換わらない"
        );

        let filled: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE fingerprint_mtime IS NOT NULL AND fingerprint_size IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("count fingerprints");
        eprintln!(
            "実DBコピー: {} 行 / fingerprint 補完済み {} 行",
            after.len(),
            filled
        );

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // 連写まとめの閾値学習
    // -----------------------------------------------------------------------

    fn burst_entry(id: &str, captured_at: i64, hash: &str, source: TimestampSource) -> BurstEntry {
        (id.to_string(), captured_at, hash.to_string(), source)
    }

    #[test]
    fn burst_pairs_only_include_adjacent_photos_with_strong_evidence() {
        let entries = vec![
            // 通常の連写。EXIF 由来で時間も近い。
            burst_entry("a", 0, "0000000000000000", TimestampSource::ExifOriginal),
            burst_entry(
                "b",
                1_000,
                "0000000000000003",
                TimestampSource::ExifOriginal,
            ),
            // 時間窓の外。
            burst_entry(
                "c",
                60_000,
                "0000000000000003",
                TimestampSource::ExifOriginal,
            ),
            // mtime 同士。時間が近くても連写の根拠にしない。
            burst_entry(
                "d",
                61_000,
                "0000000000000003",
                TimestampSource::FilesystemMtime,
            ),
            burst_entry(
                "e",
                61_500,
                "0000000000000003",
                TimestampSource::FilesystemMtime,
            ),
        ];
        let pairs = build_burst_pairs(&entries);

        let ids: Vec<&str> = pairs.iter().map(|pair| pair.id.as_str()).collect();
        assert_eq!(ids, vec!["a:b", "c:d"], "窓外と弱い根拠のペアを除外する");
        assert_eq!(pairs[0].distance, 2, "ハミング距離が入る");
        assert_eq!(pairs[0].gap_ms, 1_000);
    }

    #[test]
    fn burst_pairs_are_not_filtered_by_any_distance_threshold() {
        // どこで切るかを決めるのが学習なので、出題元を閾値で絞ってはいけない。
        // 既定の閾値 14 を大きく超える距離のペアも候補に残る必要がある。
        let entries = vec![
            burst_entry("a", 0, "0000000000000000", TimestampSource::ExifOriginal),
            burst_entry("b", 500, "ffffffffffffffff", TimestampSource::ExifOriginal),
        ];
        let pairs = build_burst_pairs(&entries);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].distance, 64);
        assert!(pairs[0].distance > HASH_DISTANCE_LIMIT);
    }

    #[test]
    fn grouping_follows_the_given_threshold() {
        let entries = vec![
            ("a".to_string(), 0, "0000000000000000".to_string()),
            ("b".to_string(), 1_000, "0000000000000003".to_string()), // a から 2
            ("c".to_string(), 2_000, "000000000000003f".to_string()), // b から 4
        ];
        let sizes = |threshold: u32| -> Vec<usize> {
            build_burst_groups(entries.clone(), threshold, &PairOverrides::new())
                .iter()
                .map(|group| group.photo_ids.len())
                .collect()
        };

        assert!(sizes(0).is_empty(), "閾値 0 では何もまとまらない");
        assert_eq!(sizes(2), vec![2], "a-b だけがまとまる");
        assert_eq!(sizes(64), vec![3], "閾値を上げ切れば1グループ");
    }

    /// 手で直したまとめは閾値より優先される。ここが効かないと、
    /// 解除したはずのまとまりが次のラウンドで復活する。
    #[test]
    fn hand_edited_pairs_win_over_the_threshold() {
        let entries = vec![
            ("a".to_string(), 0, "0000000000000000".to_string()),
            ("b".to_string(), 1_000, "0000000000000003".to_string()), // a から 2
            ("c".to_string(), 2_000, "000000000000003f".to_string()), // b から 4
        ];
        let shapes = |threshold: u32, overrides: &PairOverrides| -> Vec<Vec<String>> {
            build_burst_groups(entries.clone(), threshold, overrides)
                .into_iter()
                .map(|group| group.photo_ids)
                .collect()
        };
        let pair = |left: &str, right: &str, join: bool| {
            PairOverrides::from([((left.to_string(), right.to_string()), join)])
        };

        // 閾値 64 なら素では 1 グループ。a-b を切ると 2 枚だけが残る。
        assert_eq!(
            shapes(64, &pair("a", "b", false)),
            vec![vec!["b".to_string(), "c".to_string()]],
            "切った境目で分かれていない"
        );
        // 真ん中を両側から切ると b が独立し、1 枚のまとまりは消える。
        let split_both = PairOverrides::from([
            (("a".to_string(), "b".to_string()), false),
            (("b".to_string(), "c".to_string()), false),
        ]);
        assert!(shapes(64, &split_both).is_empty(), "b を外しきれていない");

        // 逆に、閾値では切れるペアも繋げる。
        assert_eq!(
            shapes(0, &pair("a", "b", true)),
            vec![vec!["a".to_string(), "b".to_string()]],
            "繋いだ境目が閾値に潰されている"
        );

        // 例外が無ければ従来どおり。
        assert_eq!(shapes(64, &PairOverrides::new()).len(), 1);
    }

    /// 例外の向きは撮影順の左→右。逆順のキーを拾ってしまうと、
    /// 切ったつもりが別の境目に効いてしまう。
    #[test]
    fn pair_overrides_are_keyed_left_to_right() {
        let entries = vec![
            ("a".to_string(), 0, "0000000000000000".to_string()),
            ("b".to_string(), 1_000, "0000000000000003".to_string()),
        ];
        let reversed = PairOverrides::from([(("b".to_string(), "a".to_string()), false)]);
        assert_eq!(
            build_burst_groups(entries, 64, &reversed).len(),
            1,
            "逆向きのキーが効いてしまっている"
        );
    }

    #[test]
    fn burst_threshold_columns_migrate_without_touching_existing_projects() {
        let directory = test_directory("burst-threshold");
        let database = directory.join("legacy.sqlite3");

        {
            // burst_threshold を持たない旧スキーマ。
            let legacy = Connection::open(&database).expect("open legacy");
            legacy
                .execute_batch(
                    "CREATE TABLE projects (
                       id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                       photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                       created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                     );",
                )
                .expect("create legacy projects");
            legacy
                .execute(
                    "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                     VALUES ('p1','旧プロジェクト','C:/photos',271,'ready',100,200)",
                    [],
                )
                .expect("insert legacy project");
        }

        let conn = open_database(&database).expect("migrate");
        let (name, count, threshold): (String, i64, Option<i64>) = conn
            .query_row(
                "SELECT name,photo_count,burst_threshold FROM projects WHERE id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read migrated project");
        assert_eq!(name, "旧プロジェクト", "既存の値を壊さない");
        assert_eq!(count, 271);
        assert_eq!(threshold, None, "未学習は NULL のまま");

        drop(conn);
        // 冪等であること。
        open_database(&database).expect("reopen");

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // 削除は写真原本に触れてはいけない。DB とサムネイルだけを消す。
    /// `photo://` の URI から指し先を取り出せる。
    ///
    /// **ここは環境ごとに形が変わる。** Tauri は `photo://localhost/<path>` に
    /// したり `http://photo.localhost/<path>` にしたりする。取り違えると
    /// 拡大表示が全部 404 になるが、デスクトップでは原本がローカルなので
    /// 気づきにくい。
    #[test]
    fn the_photo_scheme_recovers_the_original_reference() {
        // 届くのは encodeURIComponent された形。区切りの / は 1 つだけ。
        assert_eq!(
            photo_scheme_target("/C%3A%5CUsers%5Cmiyaji%5CPictures%5Ca.jpg", None),
            r"C:\Users\miyaji\Pictures\a.jpg"
        );
        // Unix の絶対パス。**先頭の / を落とすと相対になって読めない。**
        assert_eq!(
            photo_scheme_target("/%2Fhome%2Fuser%2Fa.jpg", None),
            "/home/user/a.jpg"
        );
        // Android の指し先。**ここが壊れていた。**
        assert_eq!(
            photo_scheme_target("/content%3A%2F%2Fmedia%2Fexternal%2Fimages%2Fmedia%2F18779", None),
            "content://media/external/images/media/18779"
        );
        assert_eq!(
            photo_scheme_target("/smb%3A%2F%2F192.168.11.8%2FShare%2F2021%2Fa.jpg", None),
            "smb://192.168.11.8/Share/2021/a.jpg"
        );
        // クエリで渡す形も受ける。パスより優先する。
        assert_eq!(
            photo_scheme_target("/ignored", Some("path=%2Fhome%2Fuser%2Fb.jpg")),
            "/home/user/b.jpg"
        );
        // 日本語のフォルダ名。
        assert_eq!(
            photo_scheme_target("/smb%3A%2F%2Fhost%2FShare%2F%E5%86%99%E7%9C%9F%2Fa.jpg", None),
            "smb://host/Share/写真/a.jpg"
        );
    }

    /// 原本が一時的に読めなくても、**前に作れていた解析結果を消さない。**
    ///
    /// NAS が落ちた・Wi-Fi が切れた・SMB のセッションが切れた、はどれも普通に
    /// 起きる。そのたびにサムネイルと dHash を NULL で塗り潰すと、作り終えた
    /// 結果が失われて連写のまとまりまで消える。実機の NAS で 187 枚中 172 枚が
    /// これで消えた（ファイルは残っているのに DB からは消えている状態）。
    #[test]
    fn a_failed_read_keeps_the_analysis_we_already_had() {
        let directory = test_directory("keep-analysis");
        let conn = open_database(&directory.join("keep.sqlite3")).expect("open database");
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p','p','smb://host/share',1,'ready',1,1)",
            [],
        )
        .expect("insert project");
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,
               fingerprint_mtime,fingerprint_size,is_missing,thumbnail_path,thumbnail_version,d_hash_version)
             VALUES ('photo','p','smb://host/share/a.jpg','a.jpg','a.jpg',1,'ffffffffffffffff',0,
               10,20,0,'/thumbs/photo.jpg',?1,?2)",
            params![THUMBNAIL_VERSION, D_HASH_VERSION],
        )
        .expect("insert photo");

        // 読めなかった1枚。fingerprint も取れていない。
        let mut failure = PhotoWork::new(0, "photo");
        failure.error = Some("ファイルを開けませんでした（移動・削除・権限）。".into());
        apply_analysis(&conn, &failure).expect("apply failure");

        let (hash, thumb, error): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT d_hash,thumbnail_path,analysis_error FROM photos WHERE id='photo'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read back");
        assert_eq!(hash.as_deref(), Some("ffffffffffffffff"), "dHash が消えている");
        assert_eq!(thumb.as_deref(), Some("/thumbs/photo.jpg"), "サムネイルが消えている");
        assert!(error.is_some(), "失敗したことは残す");

        // 読めた1枚は上書きする。**据え置きは失敗のときだけ。**
        let mut success = PhotoWork::new(1, "photo");
        success.d_hash = Some("0000000000000000".into());
        success.thumbnail_path = Some("/thumbs/new.jpg".into());
        success.fingerprint = Some((30, 40));
        apply_analysis(&conn, &success).expect("apply success");

        let (hash, thumb, error): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT d_hash,thumbnail_path,analysis_error FROM photos WHERE id='photo'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read back");
        assert_eq!(hash.as_deref(), Some("0000000000000000"), "新しい結果に入れ替わらない");
        assert_eq!(thumb.as_deref(), Some("/thumbs/new.jpg"));
        assert!(error.is_none(), "成功したら失敗の記録は消える");
    }

    #[test]
    fn deleting_a_project_removes_only_app_owned_data() {
        let directory = test_directory("delete-project");
        let original = directory.join("original.jpg");
        let thumbnail = directory.join("thumb.jpg");
        fs::write(&original, b"original photo bytes").expect("write original");
        fs::write(&thumbnail, b"thumb").expect("write thumbnail");
        let database = directory.join("delete.sqlite3");
        let conn = open_database(&database).expect("open database");

        for (project, photo) in [("keep", "photo-keep"), ("drop", "photo-drop")] {
            conn.execute(
                "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                 VALUES (?1,?1,?2,1,'ready',1,1)",
                params![project, directory.to_string_lossy().to_string()],
            )
            .expect("insert project");
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing,thumbnail_path)
                 VALUES (?1,?2,?3,'original.jpg','original.jpg',1,'0000000000000000',0,1,1,0,?4)",
                params![
                    photo,
                    project,
                    original.to_string_lossy().to_string(),
                    thumbnail.to_string_lossy().to_string()
                ],
            )
            .expect("insert photo");
            conn.execute(
                "INSERT INTO project_states (project_id,state_json,updated_at) VALUES (?1,'{}',1)",
                params![project],
            )
            .expect("insert state");
        }

        // delete_project の中身と同じ手順（AppHandle を要するコマンド本体は
        // ここから呼べないため、同じ SQL とファイル削除を並べて検証する）。
        let thumbnails: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT thumbnail_path FROM photos WHERE project_id=?1 AND thumbnail_path IS NOT NULL")
                .expect("prepare thumbnails");
            statement
                .query_map(params!["drop"], |row| row.get(0))
                .expect("query thumbnails")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect thumbnails")
        };
        for path in &thumbnails {
            let _ = fs::remove_file(Path::new(path));
        }
        conn.execute(
            "DELETE FROM project_states WHERE project_id=?1",
            params!["drop"],
        )
        .expect("delete state");
        conn.execute("DELETE FROM photos WHERE project_id=?1", params!["drop"])
            .expect("delete photos");
        conn.execute("DELETE FROM projects WHERE id=?1", params!["drop"])
            .expect("delete project");

        assert!(original.is_file(), "写真原本は残る（削除テスト）");
        assert!(!thumbnail.is_file(), "サムネイルは消える");

        let projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .expect("count projects");
        let photos: i64 = conn
            .query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))
            .expect("count photos");
        let states: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_states", [], |row| row.get(0))
            .expect("count states");
        assert_eq!(projects, 1, "他のプロジェクトは残る");
        assert_eq!(photos, 1);
        assert_eq!(states, 1);

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // -----------------------------------------------------------------------
    // レーティングと書き出し
    // -----------------------------------------------------------------------

    fn insert_rated_photo(conn: &Connection, project: &str, id: &str, name: &str, rating: i64) {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing,thumbnail_path)
             VALUES (?1,?2,?3,?4,?4,1,'0000000000000000',?5,1,1,0,'C:/thumb.jpg')",
            params![id, project, format!("C:/photos/{name}"), name, rating],
        )
        .expect("insert rated photo");
    }

    fn rated_fixture(database: &Path) -> Connection {
        let conn = open_database(database).expect("open database");
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',4,'ready',1,1)",
            [],
        )
        .expect("insert project");
        insert_rated_photo(&conn, "p1", "five", "e-five.jpg", 5);
        insert_rated_photo(&conn, "p1", "three", "d-three.jpg", 3);
        insert_rated_photo(&conn, "p1", "three-b", "c-three.jpg", 3);
        insert_rated_photo(&conn, "p1", "zero", "b-zero.jpg", 0);
        conn
    }

    /// `photo_page_query` が返した SQL と bind を**そのまま実行**する。
    /// 句だけを組み立てて検証していたときは、bind の数が合わないことに
    /// 気づけず「Wrong number of parameters passed to query」で落ちていた。
    fn run_photo_page(
        conn: &Connection,
        rating: Option<i64>,
        sort: Option<&str>,
    ) -> (i64, Vec<String>) {
        let (count_sql, page_sql, binds) = photo_page_query("p1", 0, 80, rating, sort);
        let count_binds: Vec<rusqlite::types::Value> = std::iter::once(binds[0].clone())
            .chain(binds.get(3).cloned())
            .collect();
        let total: i64 = conn
            .query_row(&count_sql, rusqlite::params_from_iter(count_binds), |row| {
                row.get(0)
            })
            .expect("count query");
        let mut statement = conn.prepare(&page_sql).expect("prepare page");
        let names = statement
            .query_map(rusqlite::params_from_iter(binds), |row| {
                row.get::<_, String>(0)
            })
            .expect("page query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect page");
        (total, names)
    }

    #[test]
    fn photo_page_query_binds_match_the_placeholders() {
        let directory = test_directory("page-query");
        let conn = rated_fixture(&directory.join("page.sqlite3"));

        // 星を指定しない場合。以前はここで bind が 1 個余って落ちていた。
        let (total, ids) = run_photo_page(&conn, None, None);
        assert_eq!(total, 4);
        assert_eq!(ids.len(), 4);

        // 星を指定した場合。件数も中身も絞られる。
        let (total, ids) = run_photo_page(&conn, Some(3), None);
        assert_eq!(total, 2, "★3 は 2 枚");
        assert_eq!(ids.len(), 2);

        let (total, _) = run_photo_page(&conn, Some(5), Some("rating"));
        assert_eq!(total, 1);
        let (total, _) = run_photo_page(&conn, Some(4), None);
        assert_eq!(total, 0, "該当なしでも落ちない");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn photo_page_query_sorts_by_rating() {
        let directory = test_directory("page-sort");
        let conn = rated_fixture(&directory.join("sort.sqlite3"));

        let (_, by_rating) = run_photo_page(&conn, None, Some("rating"));
        let (_, by_name) = run_photo_page(&conn, None, Some("name"));
        assert_eq!(by_rating, vec!["five", "three-b", "three", "zero"]);
        assert_eq!(by_name, vec!["zero", "three-b", "three", "five"]);

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn selection_targets_are_chosen_by_rating() {
        let directory = test_directory("rating-target");
        let conn = rated_fixture(&directory.join("t.sqlite3"));

        let ids = |rating: Option<i64>| -> Vec<String> {
            let clause = if rating.is_some() {
                " AND rating=?2"
            } else {
                ""
            };
            let mut statement = conn
                .prepare(&format!(
                    "SELECT id FROM photos WHERE project_id=?1 AND is_missing=0{clause} ORDER BY relative_path"
                ))
                .expect("prepare");
            statement
                .query_map(
                    rusqlite::params_from_iter(
                        std::iter::once(rusqlite::types::Value::from("p1".to_string()))
                            .chain(rating.map(rusqlite::types::Value::from)),
                    ),
                    |row| row.get(0),
                )
                .expect("query")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect")
        };

        // 同じ星の写真は、どの経路で辿り着いたかに関係なく1つの対象になる。
        assert_eq!(ids(Some(3)), vec!["three-b", "three"]);
        assert_eq!(ids(Some(5)), vec!["five"]);
        assert_eq!(ids(Some(0)), vec!["zero"]);
        assert_eq!(ids(None).len(), 4);

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    /// 選別の並びは撮影順。似た構図は撮影時刻が近いので、この並びのまま
    /// グループに切ると「同じ場面から1枚選ぶ」比較になる。
    /// 撮影日時の無い写真は末尾へ回し、その中はファイル順で安定させる。
    #[test]
    fn selection_seed_is_ordered_by_capture_time() {
        let directory = test_directory("seed-order");
        let conn = open_database(&directory.join("seed.sqlite3")).expect("open database");
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',5,'ready',1,1)",
            [],
        )
        .expect("insert project");

        // id 順・ファイル名順・挿入順のどれとも食い違う撮影順にする。
        // どれか1つでも一致していると、間違った ORDER BY を素通りさせてしまう。
        let insert = |id: &str, relative: &str, captured: Option<i64>| {
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,rating,is_missing)
                 VALUES (?1,'p1',?2,?3,?3,?4,0,0)",
                params![id, format!("C:/photos/{relative}"), relative, captured],
            )
            .expect("insert photo");
        };
        insert("id-a", "2-third.jpg", Some(300));
        insert("id-y", "b-no-time.jpg", None);
        insert("id-z", "3-first.jpg", Some(100));
        insert("id-b", "c-no-time.jpg", None);
        insert("id-m", "1-second.jpg", Some(200));

        let mut statement = conn
            .prepare(&format!(
                "SELECT id FROM photos WHERE project_id=?1 AND is_missing=0{SELECTION_SEED_ORDER}"
            ))
            .expect("prepare");
        let ids: Vec<String> = statement
            .query_map(params!["p1"], |row| row.get(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");

        assert_eq!(
            ids,
            vec!["id-z", "id-m", "id-a", "id-y", "id-b"],
            "撮影順。日時の無い2枚は末尾で、その中はファイル順"
        );

        drop(statement);
        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // ---- レートの移動 ----------------------------------------------------

    #[test]
    fn moving_a_rating_only_touches_the_source_star() {
        let directory = test_directory("move-rating");
        let path = directory.join("move.sqlite3");
        let conn = rated_fixture(&path);

        // ★3 の 2 枚をまるごと ★5 へ。
        let moved = move_rating_in(&conn, "p1", 3, 5, None, &[]).expect("move");
        assert_eq!(moved, 2);

        let count = |rating: i64| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM photos WHERE project_id='p1' AND rating=?1",
                params![rating],
                |row| row.get(0),
            )
            .expect("count")
        };
        assert_eq!(count(3), 0, "元の星は空になる");
        assert_eq!(count(5), 3, "元から★5 の1枚に2枚が合流する");
        assert_eq!(count(0), 1, "他の星は動かない");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn excluded_photos_stay_where_they_are() {
        let directory = test_directory("move-exclude");
        let conn = rated_fixture(&directory.join("ex.sqlite3"));

        let moved =
            move_rating_in(&conn, "p1", 3, 1, None, &["three-b".to_string()]).expect("move");
        assert_eq!(moved, 1);

        let rating = |id: &str| -> i64 {
            conn.query_row(
                "SELECT rating FROM photos WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .expect("read rating")
        };
        assert_eq!(rating("three"), 1);
        assert_eq!(rating("three-b"), 3, "除外した写真は元の星に残る");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn include_ids_move_only_those_photos() {
        let directory = test_directory("move-include");
        let conn = rated_fixture(&directory.join("in.sqlite3"));

        let include = Some(vec!["three-b".to_string()]);
        let moved = move_rating_in(&conn, "p1", 3, 2, include, &[]).expect("move");
        assert_eq!(moved, 1);

        let rating = |id: &str| -> i64 {
            conn.query_row(
                "SELECT rating FROM photos WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .expect("read rating")
        };
        assert_eq!(rating("three-b"), 2);
        assert_eq!(rating("three"), 3, "指定しなかった写真は動かない");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn moving_never_leaves_the_project_or_the_source_star() {
        let directory = test_directory("move-scope");
        let conn = rated_fixture(&directory.join("scope.sqlite3"));
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p2','p2','C:/other',1,'ready',1,1)",
            [],
        )
        .expect("insert other project");
        insert_rated_photo(&conn, "p2", "other-three", "other.jpg", 3);

        // include_ids に他プロジェクトの写真を混ぜても動かない。
        let include = Some(vec!["other-three".to_string(), "three".to_string()]);
        let moved = move_rating_in(&conn, "p1", 3, 4, include, &[]).expect("move");
        assert_eq!(moved, 1);

        let rating = |id: &str| -> i64 {
            conn.query_row(
                "SELECT rating FROM photos WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .expect("read rating")
        };
        assert_eq!(rating("other-three"), 3, "別プロジェクトには触れない");
        assert_eq!(rating("five"), 5, "元の星が違う写真も動かない");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn moving_to_the_same_star_or_out_of_range_is_rejected() {
        let directory = test_directory("move-guard");
        let conn = rated_fixture(&directory.join("guard.sqlite3"));

        assert_eq!(
            move_rating_in(&conn, "p1", 3, 3, None, &[]).expect("same star"),
            0,
            "同じ星への移動は何もしない"
        );
        assert!(move_rating_in(&conn, "p1", 3, MAX_RATING + 1, None, &[]).is_err());
        assert!(move_rating_in(&conn, "p1", -1, 3, None, &[]).is_err());

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    /// id を `IN (?, ?, …)` で渡すと SQLite の変数上限に当たる。
    /// 一時テーブル経由にしてあることを、上限を超える件数で確かめる。
    #[test]
    fn moving_handles_more_ids_than_sqlite_allows_as_parameters() {
        let directory = test_directory("move-many");
        let conn = open_database(&directory.join("many.sqlite3")).expect("open database");
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',1500,'ready',1,1)",
            [],
        )
        .expect("insert project");
        for index in 0..1500 {
            insert_rated_photo(
                &conn,
                "p1",
                &format!("id-{index}"),
                &format!("{index}.jpg"),
                2,
            );
        }

        // 1 枚だけ残して、残り 1,499 枚を明示指定で動かす。
        let include: Vec<String> = (0..1499).map(|index| format!("id-{index}")).collect();
        let moved = move_rating_in(&conn, "p1", 2, 4, Some(include), &[]).expect("move many");
        assert_eq!(moved, 1499);

        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE project_id='p1' AND rating=2",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(left, 1);

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn resetting_ratings_keeps_the_analysis() {
        let directory = test_directory("rating-reset");
        let conn = rated_fixture(&directory.join("r.sqlite3"));

        conn.execute("UPDATE photos SET rating=0 WHERE project_id='p1'", [])
            .expect("reset");

        let (ratings, hashes, thumbs): (i64, i64, i64) = conn
            .query_row(
                "SELECT SUM(rating),SUM(d_hash IS NOT NULL),SUM(thumbnail_path IS NOT NULL)
                 FROM photos WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read after reset");
        assert_eq!(ratings, 0, "星は消える");
        assert_eq!(hashes, 4, "d_hash は消さない");
        assert_eq!(thumbs, 4, "サムネイルは消さない");

        drop(conn);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn unique_destination_never_overwrites() {
        let directory = test_directory("unique-dest");
        fs::write(directory.join("a.jpg"), b"first").expect("write");
        let next = unique_destination(&directory, "a.jpg");
        assert_eq!(next.file_name().unwrap().to_str().unwrap(), "a (2).jpg");

        fs::write(&next, b"second").expect("write second");
        let third = unique_destination(&directory, "a.jpg");
        assert_eq!(third.file_name().unwrap().to_str().unwrap(), "a (3).jpg");
        // 既にあるファイルは書き換えられていない。
        assert_eq!(fs::read(directory.join("a.jpg")).unwrap(), b"first");

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    // ---- JPEG への星の書き込み ------------------------------------------

    /// 最小構成の JPEG。SOI + APP1(EXIF) + SOS + 画像データ + EOI。
    fn tiny_jpeg_with_exif() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        let exif_payload = b"Exif\0\0dummy-exif-payload";
        bytes.push(0xFF);
        bytes.push(0xE1);
        bytes.extend_from_slice(&((exif_payload.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(exif_payload);
        bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]); // SOS
        bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // 画像データのつもり
        bytes.extend_from_slice(&[0xFF, 0xD9]); // EOI
        bytes
    }

    fn xmp_ratings_in(bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes);
        text.match_indices("xmp:Rating=\"")
            .map(|(index, _)| {
                let rest = &text[index + 12..];
                rest.chars().take_while(|c| *c != '"').collect::<String>()
            })
            .collect()
    }

    #[test]
    fn writing_a_rating_keeps_the_image_data_and_exif() {
        let original = tiny_jpeg_with_exif();
        let updated = jpeg_with_rating(&original, 4).expect("write rating");

        assert_eq!(xmp_ratings_in(&updated), vec!["4"], "星が1つだけ入る");
        assert!(
            updated.windows(4).any(|w| w == b"Exif"),
            "EXIF セグメントを消さない"
        );
        // SOS 以降（画像本体）が一致すること。ここが変わったら画像が壊れる。
        let tail = |bytes: &[u8]| {
            let position = bytes
                .windows(2)
                .position(|w| w == [0xFF, 0xDA])
                .expect("SOS");
            bytes[position..].to_vec()
        };
        assert_eq!(
            tail(&updated),
            tail(&original),
            "画像本体は 1 バイトも変えない"
        );
        assert_eq!(&updated[updated.len() - 2..], &[0xFF, 0xD9], "EOI で終わる");
    }

    #[test]
    fn writing_a_rating_twice_replaces_instead_of_appending() {
        let original = tiny_jpeg_with_exif();
        let once = jpeg_with_rating(&original, 2).expect("first");
        let twice = jpeg_with_rating(&once, 5).expect("second");

        // 何度書いても XMP は1つ。追記され続けるとファイルが際限なく育つ。
        assert_eq!(xmp_ratings_in(&twice), vec!["5"]);
        assert_eq!(
            twice.iter().filter(|b| **b == 0xE1).count(),
            once.iter().filter(|b| **b == 0xE1).count(),
            "APP1 の数が増えない"
        );
    }

    #[test]
    fn rating_zero_is_written_as_zero() {
        // 「0 だったものはメタデータも 0」。xmp:Rating は 0 を持てる。
        let updated = jpeg_with_rating(&tiny_jpeg_with_exif(), 0).expect("write zero");
        assert_eq!(xmp_ratings_in(&updated), vec!["0"]);
    }

    /// 実写真に対する往復。原本ではなくコピーに対して行う。
    /// 環境変数 PHOTO_CURATOR_SAMPLE_JPEG に1枚のパスを渡すと実行される。
    #[test]
    fn writing_a_rating_to_a_real_photo_keeps_it_readable() {
        let Ok(source) = std::env::var("PHOTO_CURATOR_SAMPLE_JPEG") else {
            eprintln!("PHOTO_CURATOR_SAMPLE_JPEG が未設定のため skip");
            return;
        };
        let original = fs::read(&source).expect("read sample");
        let before = image::ImageReader::open(&source)
            .expect("open sample")
            .into_dimensions()
            .expect("read dimensions");

        let updated = jpeg_with_rating(&original, 4).expect("write rating");

        let directory = test_directory("real-jpeg");
        let target = directory.join("written.jpg");
        fs::write(&target, &updated).expect("write updated");

        let after = image::ImageReader::open(&target)
            .expect("open updated")
            .into_dimensions()
            .expect("read updated dimensions");
        assert_eq!(before, after, "画像の寸法が変わらない");
        assert_eq!(xmp_ratings_in(&updated), vec!["4"]);
        // EXIF が残っていれば撮影時刻も読めるはず。
        assert!(read_capture_time(&target).is_some(), "撮影時刻を読み直せる");
        eprintln!(
            "実写真: {}x{} / {} bytes → {} bytes",
            before.0,
            before.1,
            original.len(),
            updated.len()
        );

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn broken_input_is_rejected_before_touching_anything() {
        assert!(jpeg_with_rating(b"not a jpeg at all", 3).is_err());
        // SOI はあるが長さが壊れている。
        let broken = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFE, 0x00];
        assert!(jpeg_with_rating(&broken, 3).is_err());
    }
}
