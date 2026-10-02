mod amazon;
mod format;
mod sidecar;
mod capture;

use capture::*;

use exif::{In, Reader, Tag, Value};
use image::DynamicImage;
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
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
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
/// Amazon の共有リンクの原本の置き場（プロジェクトごとの下に node id で置く）。
const AMAZON_CACHE_DIR: &str = "amazon-cache";
/// 作成画面の見本の置き場。
const SAMPLES_DIR: &str = "samples";
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
const D_HASH_VERSION: i64 = photo_curator_core::D_HASH_VERSION as i64;
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
/// 表示用画像 1 枚の打ち切り。原本の全画素デコードが入るので解析より長くする。
const DISPLAY_TIMEOUT_MS: u64 = 60_000;
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

    fn workers(self, folder: &Path) -> usize {
        match self {
            Self::Foreground => analysis_worker_count_for(folder),
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

/// フォルダに合わせた worker 数。ネットワークのフォルダは並列 2 に抑える
/// （同時に何本も投げると NAS 側が詰まって、かえって遅くなる）。
/// 環境変数で明示された値があればそれを優先する。
fn analysis_worker_count_for(folder: &Path) -> usize {
    if std::env::var(WORKER_COUNT_ENV).is_ok() {
        return analysis_worker_count();
    }
    if is_network_path(folder) {
        return NETWORK_ANALYSIS_WORKERS;
    }
    analysis_worker_count()
}

/// ネットワークのフォルダを解析するときの worker 数。
const NETWORK_ANALYSIS_WORKERS: usize = 2;

/// フォルダがネットワーク上か。UNC（`\\` 始まり）か、ドライブの種類が
/// `DRIVE_REMOTE`（割り当て済みのネットワークドライブ）なら true。
#[cfg(windows)]
fn is_network_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    // windows-sys では別の feature（WindowsProgramming）にあるので、値（4）を直に持つ。
    const DRIVE_REMOTE: u32 = 4;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        // 接頭辞として読めなくても、`\\server` のように `\\` で始まるなら UNC とみなす。
        let text = path.to_string_lossy();
        return text.starts_with("\\\\") && !text.starts_with("\\\\?\\") && !text.starts_with("\\\\.\\");
    };
    let letter = match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return true,
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return false,
    };
    let root: Vec<u16> = std::ffi::OsString::from(format!("{}:\\", letter as char))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `root` は NUL 終端の UTF-16 で、呼び出しの間だけ生きている。
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
}

/// Windows 以外では常にローカル扱い。
#[cfg(not(windows))]
fn is_network_path(_path: &Path) -> bool {
    false
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
    /// 写真の出所。`folder`（PC のフォルダ）か `amazon`（Amazon Photos の共有リンク）。
    source_kind: String,
    /// 同名の JPEG と RAW を 1 枚の写真として扱い、組の RAW を対象から外す（U46）。既定は true。
    pair_raw_jpeg: bool,
    /// `pair_raw_jpeg` を切り替えた時刻（ms）。0 は「作ったまま一度も切り替えていない」。
    /// サイドカーで端末どうしの設定が違うとき、新しく切り替えた方を採るのに使う（U48）。
    pair_raw_jpeg_at: i64,
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhotoPage {
    photos: Vec<Photo>,
    total: i64,
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
    // ハッシュ値の計算式を core に替えたので、古いハッシュ値が混ざらないよう別のファイルにする。
    // 旧 `photo-curator.sqlite3` は読まない・消さない。
    Ok(dir.join("photo-curator-v2.sqlite3"))
}

/// スキーマ整備（`open_database`）を済ませた DB ファイルのパス。プロセスの中で 1 本の
/// ファイルにつき 1 回だけ整備し、以後の `connection()` は軽く開くだけにする（U27 R1）。
static MIGRATED_DATABASES: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

fn connection(app: &AppHandle) -> Result<Connection, String> {
    open_connection(&db_path(app)?)
}

/// 初回（プロセスで最初に開くとき）だけ `open_database` で整備し、2 回目以降は
/// 開いて接続の設定をするだけ。整備が失敗したら記録しないので、次回また整備する。
fn open_connection(path: &Path) -> Result<Connection, String> {
    // 整備中に別スレッドが同じ整備を重ねないよう、ロックを持ったまま整備する。
    let mut migrated = MIGRATED_DATABASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if migrated.iter().any(|known| known == path) {
        drop(migrated);
        let conn = Connection::open(path)
            .map_err(|error| database_error("ローカルデータベースを開けませんでした", error))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;
        // journal_mode=WAL はファイルに残る。synchronous だけは接続ごとの設定。
        conn.execute_batch("PRAGMA synchronous=NORMAL;")
            .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;
        return Ok(conn);
    }
    let conn = open_database(path)?;
    migrated.push(path.to_path_buf());
    Ok(conn)
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

/// アプリのデータフォルダの下の置き場（なければ作る）。
fn data_subdir(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(name);
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
                // Amazon の行は fingerprint を持たない（実在しない相対パスの stat が毎回
                // 失敗するだけ）ので、フォルダ以外のプロジェクトの行は除く。
                "SELECT id,path FROM photos
                 WHERE (fingerprint_mtime IS NULL OR fingerprint_size IS NULL)
                   AND project_id NOT IN (SELECT id FROM projects WHERE source_kind<>'folder')
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
    // 写真の出所（T9）。既存のプロジェクトはすべてフォルダ。Amazon は `source_key` に
    // `"{host}|{shareId}"` を持ち、`folder_path` にはリンクの URL を置く。
    add_column_if_missing(&conn, "projects", "source_kind", "TEXT NOT NULL DEFAULT 'folder'")?;
    add_column_if_missing(&conn, "projects", "source_key", "TEXT")?;
    // 同名の JPEG と RAW を 1 枚の写真として扱う（U46）。既存のプロジェクトも既定の 1（オン）。
    add_column_if_missing(&conn, "projects", "pair_raw_jpeg", "INTEGER NOT NULL DEFAULT 1")?;
    // その設定を切り替えた時刻（U48）。既存のプロジェクトは 0（一度も切り替えていない）。
    add_column_if_missing(&conn, "projects", "pair_raw_jpeg_at", "INTEGER NOT NULL DEFAULT 0")?;
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
        // 手で直した連写の例外。core の `PairOverride`（鍵は relativePath）をそのまま持つ。
        // 旧版の `burst_pair_overrides`（photo id の組）は使わない。既存の DB には残るが読まない。
        "CREATE TABLE IF NOT EXISTS pair_overrides (
           project_id TEXT NOT NULL,
           left_path TEXT NOT NULL,
           right_path TEXT NOT NULL,
           decision TEXT NOT NULL,
           PRIMARY KEY (project_id, left_path, right_path)
         );",
    )
    .map_err(|error| error.to_string())?;

    // サイドカー（T8）。端末が覚える 3 つの値。
    sidecar::ensure_tables(&conn)?;
    // Amazon の共有リンクの tempLink の控え（T9）。
    amazon::ensure_tables(&conn)?;

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
    /// 中身が画像ではなかった。行を `is_missing=1` にして数から外す。
    not_image: bool,
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
            not_image: false,
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
/// - timeout で見捨てた worker は戻ってこないかもしれないので、1 本見捨てるたびに
///   代わりを 1 本足す（最大で `workers` 本まで。全体で `workers * 2` 本）。代わりも含めて
///   全員が固まって上限に達したら、まだ取られていない写真を全部失敗として確定して戻る
///   （永久に待たない）。正常なとき（timeout が起きないとき）は何も変わらない。
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
    // 上限は呼び出し側が決める（ローカル・NAS は `analysis_worker_count*` が
    // MAX_ANALYSIS_WORKERS に丸め済み。Amazon は amazon::WORKERS を使う）。
    let workers = workers.max(1).min(total);

    // worker 側はこのフラグだけを見る。`is_cancelled` は Tauri の State を
    // 借りていて 'static にできないため、writer が毎ティック転記する。
    let stop = Arc::new(AtomicBool::new(false));
    let cursor = Arc::new(AtomicUsize::new(0));
    // 見捨てた worker の代わりのぶんも含めて、枠を先に用意する。
    let max_slots = workers * 2;
    let in_flight: Arc<Vec<Mutex<Option<(usize, Instant)>>>> =
        Arc::new((0..max_slots).map(|_| Mutex::new(None)).collect());
    let work = Arc::new(work);
    let (sender, receiver) = mpsc::channel::<PhotoWork>();

    let spawn_worker = |slot: usize| {
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
    };
    for slot in 0..workers {
        spawn_worker(slot);
    }
    let mut spawned = workers;
    // 見捨てて、代わりを足した枠。
    let mut abandoned_slots = vec![false; max_slots];

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
        for (slot, cell) in in_flight.iter().enumerate() {
            let Some((index, started)) = cell.lock().ok().and_then(|cell| *cell) else {
                continue;
            };
            if started.elapsed() < timeout {
                continue;
            }
            if reported[index] {
                // 前の tick で timeout にした 1 枚。代わりをまだ足していなければ足す。
                if !abandoned_slots[slot] && spawned < max_slots {
                    abandoned_slots[slot] = true;
                    spawn_worker(spawned);
                    spawned += 1;
                }
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
        // 代わりも含めて、動かせる worker が 1 本も残っていない（全員が timeout 済みの
        // 1 枚から戻らず、足せる枠も無い）。まだ誰にも取られていない写真は、もう
        // 処理されないので、失敗として確定して戻る。
        let all_stuck = spawned == max_slots
            && in_flight.iter().all(|cell| {
                cell.lock()
                    .ok()
                    .and_then(|cell| *cell)
                    .is_some_and(|(index, _)| reported[index])
            });
        if all_stuck && outcome.completed < total {
            stop.store(true, Ordering::Relaxed);
            for index in 0..total {
                if reported[index] {
                    continue;
                }
                reported[index] = true;
                outcome.completed += 1;
                outcome.timed_out += 1;
                let mut result = PhotoWork::new(index, items[index].photo_id());
                result.error = Some("読み込みが応答しないため、解析できませんでした。".into());
                on_result(result)?;
            }
            return Ok(outcome);
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

/// ハッシュ値。**必ず core を通す**（Android・Web と同じ計算式にするため）。
/// 大きい画像のまま平均を取ると遅いので、先に軽く縮めてから渡す。
/// 作れないとき（極端に小さい画像など）は None で、0 を入れない。
fn d_hash_of(image: &DynamicImage) -> Option<String> {
    let scaled = if image.width().max(image.height()) > 128 {
        image.thumbnail(128, 128)
    } else {
        image.clone()
    };
    let gray = scaled.to_luma8();
    let (width, height) = (gray.width(), gray.height());
    photo_curator_core::d_hash_from_gray(gray.into_raw(), width, height)
}

/// dHash は**必ず保存するサムネイルのバイト列から**計算する。
/// 生成直後とキャッシュヒット時で必ず同じ値になることを保証するため、
/// JPEG 符号化の往復を挟んだあとの画素を使う。
fn hash_thumbnail_bytes(bytes: &[u8]) -> Option<String> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .ok()
        .and_then(|image| d_hash_of(&image))
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
    source: &Path,
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

    let Some((image, decode_source)) = decode_hash_source(source) else {
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

#[cfg_attr(not(feature = "bench"), allow(dead_code))]
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
/// 以前は閾値学習の出題元も同じ規則を使っていた（いまは core が判断する）。
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
    fingerprint_of(&fs::metadata(path).ok()?)
}

/// 「変わったか」の目印の定義: (更新時刻ミリ秒, 大きさ)。走査の列挙で得た情報と
/// `fingerprint(path)` が同じ値になるよう、作り方をここ 1 か所にする。
fn fingerprint_of(metadata: &fs::Metadata) -> Option<(i64, i64)> {
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((mtime, metadata.len() as i64))
}

/// 画像・RAW の名前の拡張子。拡張子は「候補に入れるか」だけに使い、
/// 画像かどうかは中身で決める（`content_is_not_image`）。
const IMAGE_EXTENSIONS: [&str; 13] = [
    "jpg", "jpeg", "png", "webp", "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];
/// 拡張子が動画のものは、中身に関わらず常に除く。
const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "mov", "m4v", "avi", "mts", "m2ts", "3gp"];
/// 中身が JPEG・PNG・WebP でなくても「画像だが今は読めない」に数える拡張子。
const OTHER_IMAGE_EXTENSIONS: [&str; 9] = [
    "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

/// RAW の拡張子（`IMAGE_EXTENSIONS` のうち HEIC・HEIF 以外。rw2・pef・srw は今は走査の
/// 候補に入らないが、入れる日のために並べておく）。
const RAW_EXTENSIONS: [&str; 10] = [
    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
];

/// RAW＋JPEG 同時撮影の「組」の RAW を除く（U46）。同じフォルダに、拡張子を除いた名前が
/// 大文字小文字を無視して一致する JPEG（.jpg・.jpeg）がある RAW は、写真に数えない。
/// Android は jpg/png/webp だけを走査するので、これで 2 台の顔ぶれが揃う。
/// 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組は除かない。
/// `enabled` はプロジェクトの設定（`pair_raw_jpeg`）。false なら何も除かない。
fn skip_paired_raw<T>(items: Vec<T>, enabled: bool, path_of: impl Fn(&T) -> &Path) -> Vec<T> {
    if !enabled {
        return items;
    }
    let key_of = |path: &Path| -> Option<(PathBuf, String)> {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        Some((path.parent().map(Path::to_path_buf).unwrap_or_default(), stem))
    };
    let jpeg_keys: HashSet<(PathBuf, String)> = items
        .iter()
        .filter_map(|item| {
            let path = path_of(item);
            let ext = extension_lower(path)?;
            if ext == "jpg" || ext == "jpeg" {
                key_of(path)
            } else {
                None
            }
        })
        .collect();
    items
        .into_iter()
        .filter(|item| {
            let path = path_of(item);
            let is_raw = extension_lower(path)
                .map(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
                .unwrap_or(false);
            !(is_raw && key_of(path).is_some_and(|key| jpeg_keys.contains(&key)))
        })
        .collect()
}

/// 拡張子が画像・RAW の名前か、拡張子が無いもの。動画の拡張子は常に除く。
fn is_supported(path: &Path) -> bool {
    match extension_lower(path) {
        Some(ext) => {
            !VIDEO_EXTENSIONS.contains(&ext.as_str()) && IMAGE_EXTENSIONS.contains(&ext.as_str())
        }
        None => true,
    }
}

/// 中身が画像ではない（動画・テキスト・壊れたファイルなど）。先頭バイトで決める。
/// JPEG・PNG・WebP 以外の画像（HEIC・RAW）は先頭バイトでは見分けられないので、
/// 拡張子がその種類なら画像として通し、「読めなかった」に数える（解析で失敗する）。
/// HEIC・CR3 は `ftyp` で始まり、`sniff` は動画と判定するため、この扱いが要る。
fn content_is_not_image(head: &[u8], path: &Path) -> bool {
    let kind = format::sniff(head);
    if format::is_image(kind) {
        return false;
    }
    let other_image = extension_lower(path)
        .map(|ext| OTHER_IMAGE_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false);
    !other_image
}

/// 隠しフォルダ・隠しファイル（名前が `.` で始まる）。NAS の `.webaxs` のような
/// サムネイルのキャッシュを原本として数えないために、走査の入口で除く。
fn is_hidden_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.')
}

/// 列挙で見つかった 1 枚。ハッシュ値ではなく「変わったか」の目印（更新時刻・大きさ）を、
/// 列挙で得た情報から作って持つ。
struct ListedFile {
    path: PathBuf,
    /// `fingerprint(path)` と同じ定義の (更新時刻ミリ秒, 大きさ)。読めなければ None。
    fingerprint: Option<(i64, i64)>,
}

/// フォルダの列挙の結果。`unreadable` は、列挙の途中で読めなかった場所の数
/// （権限・切断など）。1 件でもあれば、列挙は「全部は見えていない」。
struct FolderListing {
    files: Vec<ListedFile>,
    unreadable: usize,
}

/// 走査の入口。フォルダ自体が開けなければ（NAS の切断・共有の取り外し・移動）、
/// 空のフォルダとして通さずエラーにする。
///
/// 更新時刻・大きさは、ディレクトリの列挙で得た情報（Windows では列挙の結果に
/// 入っている）から取る。ファイルごとに `stat` をやり直すと、ネットワークの
/// フォルダでは 1 枚 1 往復になる。
fn list_photo_files(folder: &str, pair_raw: bool) -> Result<FolderListing, String> {
    fs::read_dir(folder)
        .map_err(|error| format!("フォルダに接続できませんでした（{folder}）: {error}"))?;
    let mut files = Vec::new();
    let mut unreadable = 0;
    for entry in WalkDir::new(folder)
        .into_iter()
        .filter_entry(|entry| !is_hidden_entry(entry))
    {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() && is_supported(entry.path()) {
                    let fingerprint = entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| fingerprint_of(&metadata));
                    files.push(ListedFile {
                        path: entry.into_path(),
                        fingerprint,
                    });
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    // RAW＋JPEG 同時撮影の組の RAW は写真に数えない（U46）。
    let files = skip_paired_raw(files, pair_raw, |file| file.path.as_path());
    Ok(FolderListing { files, unreadable })
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
    })
}

const PHOTO_COLUMNS: &str =
    "id,project_id,path,relative_path,name,captured_at,d_hash,rating,thumbnail_path,display_path";
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

/// `scan_folder` の終わり方。
enum ScanEnd {
    Completed {
        total: usize,
        /// 走査後に画面へ出る写真の数。
        #[cfg_attr(not(test), allow(dead_code))]
        count: i64,
        /// 列挙で読めなかった場所の数。0 でないとき、欠損の印は付け直していない。
        unreadable: usize,
    },
    Cancelled {
        processed: usize,
        total: usize,
    },
}

/// 走査が 1 つのトランザクションに入れる写真の数。書き込みロックを持つ時間を
/// 短くして、他の書き込み（星の保存・設定）が `database is locked` にならないようにする。
const SCAN_BATCH: usize = 200;

/// フォルダの走査の本体（DB とフォルダだけを触る。アプリの窓には触れない）。
///
/// 写真の登録は `batch_size` 枚ごとの短いトランザクションに分ける（ロックを長く
/// 持たない）。「見つからなかった写真」への欠損の印（`is_missing=1`）は、**最後まで
/// 成功したときだけ**付ける。取り消し・失敗なら印は変わらず、走査前のまま残る
/// （登録済みの分は残る。次の走査で同じ行に上書きされる）。また列挙で読めなかった
/// 場所が 1 件でもあれば、見えていない写真を「無くなった」とは決められないので、
/// 印を付け直さない。
fn scan_folder(
    conn: &Connection,
    project_id: &str,
    folder: &str,
    batch_size: usize,
    is_cancelled: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(&str, usize, usize, &str),
) -> Result<ScanEnd, String> {
    let pair_raw = project_pair_raw(conn, project_id);
    let listing = list_photo_files(folder, pair_raw)?;
    let entries = listing.files;
    let total = entries.len();
    on_progress("indexing", 0, total, "Scanning photo files…");

    conn.execute(
        "UPDATE projects SET status='scanning', updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;

    let mut seen: HashSet<String> = HashSet::with_capacity(total);
    let mut processed = 0;
    let mut next_report = 250;
    for chunk in entries.chunks(batch_size.max(1)) {
        // ループの中はファイルに触れない（DB への書き込みだけ）ので、取り消しは
        // 塊の頭で見れば十分。ここではトランザクションを持っていない。
        if is_cancelled() {
            // 登録済みの塊は残っているので、件数を数え直してから状態を戻す。
            let _ = recount_photos(conn, project_id);
            conn.execute(
                "UPDATE projects SET status='ready', updated_at=?1 WHERE id=?2",
                params![now(), project_id],
            )
            .map_err(|error| error.to_string())?;
            return Ok(ScanEnd::Cancelled { processed, total });
        }
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for file in chunk {
            let path = &file.path;
            let relative = path
                .strip_prefix(folder)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("photo")
                .to_owned();
            let absolute = path.to_string_lossy().to_string();
            // 読めない場合に (0,0) を入れると、読めないファイル同士が「同じ
            // fingerprint」に見えてしまう。NULL のまま持たせる。
            let (mtime, size) = match file.fingerprint {
                Some((mtime, size)) => (Some(mtime), Some(size)),
                None => (None, None),
            };
            upsert_photo(
                &transaction,
                project_id,
                &absolute,
                &relative,
                &name,
                mtime,
                size,
            )?;
            seen.insert(absolute);
        }
        transaction.commit().map_err(|error| error.to_string())?;
        processed += chunk.len();
        if processed >= next_report || processed == total {
            on_progress("indexing", processed, total, "Recording photo locations…");
            next_report = processed + 250;
        }
    }

    if listing.unreadable == 0 {
        // 最後まで成功し、列挙も欠けていなかったときだけ、見つからなかった写真に印を付ける。
        let stale: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT id,path FROM photos WHERE project_id=?1 AND is_missing=0")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            let mut stale = Vec::new();
            for row in rows {
                let (id, path) = row.map_err(|error| error.to_string())?;
                if !seen.contains(&path) {
                    stale.push(id);
                }
            }
            stale
        };
        if !stale.is_empty() {
            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            for id in &stale {
                transaction
                    .execute("UPDATE photos SET is_missing=1 WHERE id=?1", params![id])
                    .map_err(|error| error.to_string())?;
            }
            transaction.commit().map_err(|error| error.to_string())?;
        }
    }
    let count = recount_photos(conn, project_id)?;
    conn.execute(
        "UPDATE projects SET status='ready',updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(ScanEnd::Completed {
        total,
        count,
        unreadable: listing.unreadable,
    })
}

fn run_scan(app: AppHandle, registry: &TaskRegistry, project_id: String) -> Result<(), String> {
    // 出所で分ける。Amazon の共有リンクは、一覧を読むことが走査になる。
    if let Some(source) = amazon_source_of(&connection(&app)?, &project_id)? {
        return run_amazon_scan(app, registry, project_id, source);
    }
    let task_key = format!("scan:{project_id}");
    let folder = project_folder(&app, &project_id)?;
    let conn = connection(&app)?;
    let ended = scan_folder(
        &conn,
        &project_id,
        &folder,
        SCAN_BATCH,
        &|| registry.is_cancelled(&task_key),
        &mut |phase, processed, total, message| {
            progress(&app, &project_id, "scan", phase, processed, total, message)
        },
    );
    let (total, unreadable) = match ended {
        Ok(ScanEnd::Completed {
            total, unreadable, ..
        }) => (total, unreadable),
        Ok(ScanEnd::Cancelled { processed, total }) => {
            progress(
                &app,
                &project_id,
                "scan",
                "cancelled",
                processed,
                total,
                "Scanning was cancelled.",
            );
            return Ok(());
        }
        Err(message) => {
            // 登録済みの塊があるので件数を数え直し、'scanning' のまま残さない（写真があれば ready、なければ new）。
            let _ = recount_photos(&conn, &project_id);
            settle_project_status(&conn, &project_id);
            return Err(message);
        }
    };
    let complete_message = if unreadable > 0 {
        format!(
            "写真の読み込みが完了しました。ただし読めなかったフォルダが {unreadable} か所あったため、見つからなかった写真の整理はしていません。"
        )
    } else {
        "写真の読み込みが完了しました。".to_string()
    };
    progress(
        &app,
        &project_id,
        "scan",
        "complete",
        total,
        total,
        complete_message,
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

/// Amazon の写真の行を登録・更新する。`path` と `relative_path` は node id。
/// 撮影時刻は一覧の `contentDate`（その土地の時計）をそのまま入れる。
/// 大きさ（`fingerprint_size`）が変わった行だけ、解析の結果を捨てる。
fn upsert_amazon_photo(
    conn: &Connection,
    project_id: &str,
    node: &amazon::AmazonNode,
) -> Result<(), String> {
    let size = node
        .content_properties
        .as_ref()
        .and_then(|properties| properties.size)
        .map(|size| size as i64);
    let stem = Path::new(&node.name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let (captured_at, source) = match amazon::captured_at_of(node) {
        Some(at) => (Some(at), TimestampSource::ExifOriginal),
        None => match filename_capture_time_of(stem) {
            Some(at) => (Some(at), TimestampSource::FilenameInferred),
            None => (None, TimestampSource::Unknown),
        },
    };
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,timestamp_source,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
         VALUES (?1,?2,?3,?3,?4,?5,?6,NULL,0,NULL,?7,0)
         ON CONFLICT(project_id,path) DO UPDATE SET
           relative_path=excluded.relative_path, name=excluded.name, is_missing=0,
           captured_at=excluded.captured_at, timestamp_source=excluded.timestamp_source,
           d_hash=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash ELSE NULL END,
           d_hash_version=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash_version ELSE NULL END,
           thumbnail_path=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_path ELSE NULL END,
           thumbnail_version=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_version ELSE NULL END,
           display_path=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_path ELSE NULL END,
           display_edge=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_edge ELSE NULL END,
           analysis_error=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error ELSE NULL END,
           analysis_error_at=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_at ELSE NULL END,
           fingerprint_size=excluded.fingerprint_size",
        params![
            Uuid::new_v4().to_string(),
            project_id,
            node.id,
            node.name,
            captured_at,
            source.as_str(),
            size
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// `run_scan` の Amazon 版。共有 → アルバム → FILE の一覧を読み、写真の行と tempLink を作る。
/// 共有リンクが消えていたら、状態を `missing` にして理由を返す（自動ではやり直さない）。
fn run_amazon_scan(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
    source: amazon::AmazonSource,
) -> Result<(), String> {
    let task_key = format!("scan:{project_id}");
    progress(&app, &project_id, "scan", "indexing", 0, 0, "Amazon Photos の一覧を読んでいます…");
    let conn = connection(&app)?;
    conn.execute(
        "UPDATE projects SET status='scanning', updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;

    let listed = amazon::fetch_share_root(&source)
        .and_then(|root| amazon::list_photos(&source, &root.node_id));
    let nodes = match listed {
        Ok(nodes) => nodes,
        Err(message) => {
            if message == amazon::GONE_MESSAGE {
                mark_amazon_gone(&conn, &project_id);
            } else {
                settle_project_status(&conn, &project_id);
            }
            return Err(message);
        }
    };
    let total = nodes.len();
    if registry.is_cancelled(&task_key) {
        settle_project_status(&conn, &project_id);
        progress(&app, &project_id, "scan", "cancelled", 0, total, "Scanning was cancelled.");
        return Ok(());
    }

    // 欠損の印は登録と同じトランザクションに入れる。途中で失敗しても走査前のまま残る。
    // 失敗で 'scanning' のまま残さない。
    let written = (|| -> Result<i64, String> {
        let transaction = conn.unchecked_transaction().map_err(|error| error.to_string())?;
        transaction
            .execute("UPDATE photos SET is_missing=1 WHERE project_id=?1", params![project_id])
            .map_err(|error| error.to_string())?;
        for (index, node) in nodes.iter().enumerate() {
            upsert_amazon_photo(&transaction, &project_id, node)?;
            if (index + 1) % 250 == 0 || index + 1 == total {
                progress(&app, &project_id, "scan", "indexing", index + 1, total, "Recording photo locations…");
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        // tempLink は node id で控える。拡大のたびに一覧をたどり直さないため。
        amazon::save_links(&conn, &project_id, &amazon::links_of(&nodes))?;
        recount_photos(&conn, &project_id)
    })();
    let count = match written {
        Ok(count) => count,
        Err(message) => {
            settle_project_status(&conn, &project_id);
            return Err(message);
        }
    };
    conn.execute(
        "UPDATE projects SET status='ready',updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    progress(&app, &project_id, "scan", "complete", count as usize, count as usize, "写真の読み込みが完了しました。");
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let _ = spawn_analysis(handle.clone(), &registry, project_id, AnalysisMode::Background);
    });
    Ok(())
}

/// 撮影時刻の段の worker 1本ぶん。先頭 64KB を 1 回だけ読み、画像かどうかと
/// 撮影時刻を決める。**DB には触れない。**
fn metadata_one(index: usize, job: &MetadataJob) -> PhotoWork {
    let mut result = PhotoWork::new(index, &job.id);
    let path = Path::new(&job.path);
    let source = LocalPhoto(path);
    let head = source.head(EXIF_HEAD_PROBE);
    if let Some(head) = head.as_deref() {
        if content_is_not_image(head, path) {
            result.not_image = true;
            return result;
        }
    }
    match capture_time_from_head(&source, head.as_deref()) {
        Some(capture) => {
            result.captured_at = Some(capture.at);
            result.timestamp_source = Some(capture.source);
        }
        None => {
            result.error = Some("撮影時刻を読み取れませんでした。".into());
        }
    }
    result
}

/// 撮影時刻の段の結果を書く。画像ではなかった行は数から外す（`is_missing=1`）。
/// 撮影時刻は空のままにするので、再走査で行が戻っても次の解析でまた判定される。
fn apply_metadata(tx: &Connection, item: &PhotoWork) -> Result<(), String> {
    if item.not_image {
        tx.execute(
            "UPDATE photos SET is_missing=1,captured_at=NULL,timestamp_source=NULL,
               analysis_error=NULL,analysis_error_at=NULL
             WHERE id=?1",
            params![item.photo_id],
        )
        .map_err(|error| error.to_string())?;
        return Ok(());
    }
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
}

/// プロジェクトの写真の数を、欠損を除いて数え直す。
fn recount_photos(conn: &Connection, project_id: &str) -> Result<i64, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE projects SET photo_count=?1 WHERE id=?2",
        params![count, project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(count)
}

/// worker 1本ぶんの仕事。1枚を読み、サムネイルと dHash を作る。
/// **DB には触れない。**書き込みは writer が `flush_results` でまとめて行う。
fn hash_one(thumbnails: &Path, index: usize, record: &HashRecord) -> PhotoWork {
    let mut result = PhotoWork::new(index, &record.id);
    let photo_path = Path::new(&record.path);
    let current = fingerprint(photo_path);
    let analysed = analyse_photo(thumbnails, &record.id, photo_path, current, &record.cached);
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

/// `hash_one` の Amazon 版。`viewBox=160` の画像を取り、旧版と同じサムネイルとハッシュ値にする。
/// サムネイルが残っていて版も合えば、網を使わない。**DB には触れない。**
fn hash_one_amazon(
    book: &amazon::LinkBook,
    thumbnails: &Path,
    index: usize,
    record: &HashRecord,
) -> PhotoWork {
    let mut result = PhotoWork::new(index, &record.id);
    let file = thumbnail_file(thumbnails, &record.id);
    let stored = file.to_string_lossy().to_string();
    let cached = &record.cached;
    if cached.thumbnail_path.as_deref() == Some(stored.as_str())
        && cached.thumbnail_version == Some(THUMBNAIL_VERSION)
        && file.is_file()
    {
        if cached.d_hash.is_some() && cached.d_hash_version == Some(D_HASH_VERSION) {
            result.d_hash = cached.d_hash.clone();
            result.thumbnail_path = Some(stored);
            result.hash_reused = true;
            return result;
        }
        if let Some(hash) = fs::read(&file).ok().as_deref().and_then(hash_thumbnail_bytes) {
            result.d_hash = Some(hash);
            result.thumbnail_path = Some(stored);
            return result;
        }
    }
    let bytes = match book.fetch(&record.path, Some(AMAZON_THUMBNAIL_EDGE)) {
        Ok(bytes) => bytes,
        Err(message) => {
            result.error = Some(message);
            return result;
        }
    };
    let Some(image) = image::load_from_memory(&bytes).ok() else {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
        return result;
    };
    let Some(thumbnail) = encode_thumbnail(&scale_for_thumbnail(&image)) else {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
        return result;
    };
    result.d_hash = hash_thumbnail_bytes(&thumbnail);
    if write_atomically(&file, &thumbnail).is_ok() {
        result.thumbnail_path = Some(stored);
        result.thumbnail_source = Some("amazon");
    }
    if result.d_hash.is_none() {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
    }
    result
}

/// Amazon に縮小させるときの長辺（サムネイルとハッシュ値のもと）。
const AMAZON_THUMBNAIL_EDGE: u32 = 160;

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
    // ネットワークのフォルダは worker 数を抑えるので、フォルダの場所を先に知る。
    let folder = project_folder(&app, &project_id)?;
    // Amazon の共有リンクは、撮影時刻を一覧の contentDate から走査で入れてあり、
    // サムネイル・ハッシュ値は viewBox=160 の画像から作る。並列は WORKERS（8）。
    let amazon_book = amazon_book_of(&app, &project_id)?;
    let workers = if amazon_book.is_some() {
        amazon::WORKERS
    } else {
        mode.workers(Path::new(&folder))
    };
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
        .filter(|(_, _, captured_at, source)| {
            amazon_book.is_none() && (captured_at.is_none() || source.is_none())
        })
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
        let apply = apply_metadata;
        let outcome = run_in_parallel(
            Arc::new(metadata_jobs),
            workers,
            timeout,
            &should_stop,
            // worker はファイルを読むだけ。DB には触れない。
            |index, job: &MetadataJob| metadata_one(index, job),
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
        // 中身が画像ではなかった写真は数から外れているので、件数を数え直す。
        recount_photos(&conn, &project_id)?;
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
        // Amazon は撮影時刻の無い写真にもサムネイルが要る（一覧の絵になる）ので絞らない。
        let dated_only = if amazon_book.is_some() {
            ""
        } else {
            "AND captured_at IS NOT NULL"
        };
        let mut statement = conn
            .prepare(&format!(
                "SELECT id,path,captured_at,timestamp_source,d_hash,d_hash_version,
                        thumbnail_path,thumbnail_mtime,thumbnail_size,thumbnail_version
                 FROM photos
                 WHERE project_id=?1 AND is_missing=0 {dated_only}
                 ORDER BY captured_at IS NULL, captured_at, path"
            ))
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                let source: Option<String> = row.get(3)?;
                Ok(HashRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    captured_at: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
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
    let needed_records: Vec<_> = if amazon_book.is_some() {
        // Amazon は小さい画像を取るだけなので、近い写真に絞らず全部を作る。
        candidates.clone()
    } else {
        candidates
            .iter()
            .filter(|record| selection.ids.contains(&record.id))
            .cloned()
            .collect()
    };
    let needed_total = needed_records.len();
    let warning = (selection.narrowed && amazon_book.is_none()).then(|| {
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
    let is_amazon = amazon_book.is_some();
    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        // 据え置きで済んだ1枚は、書き込む理由が無い。
        if item.hash_reused {
            return Ok(());
        }
        if is_amazon {
            // 原本の mtime は無い。走査が入れた大きさ（fingerprint_size）は触らない。
            tx.execute(
                "UPDATE photos SET d_hash=?1,d_hash_version=?2,thumbnail_path=?3,
                   thumbnail_source=COALESCE(?4,thumbnail_source),
                   thumbnail_version=?5,
                   analysis_error=?6,analysis_error_at=?7
                 WHERE id=?8",
                params![
                    item.d_hash,
                    item.d_hash.as_ref().map(|_| D_HASH_VERSION),
                    item.thumbnail_path,
                    item.thumbnail_source,
                    item.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
                    item.error,
                    item.error.as_ref().map(|_| now()),
                    item.photo_id
                ],
            )
            .map_err(|error| error.to_string())?;
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
        tx.execute(
            "UPDATE photos SET d_hash=?1,d_hash_version=?2,thumbnail_path=?3,
               thumbnail_mtime=?4,thumbnail_size=?5,
               thumbnail_source=COALESCE(?6,thumbnail_source),
               thumbnail_version=?7,
               fingerprint_mtime=?8,fingerprint_size=?9,
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
                item.photo_id
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    };
    let thumbnails_for_workers = thumbnails.clone();
    let book_for_workers = amazon_book.clone();
    let outcome = run_in_parallel(
        Arc::new(needed_records),
        workers,
        timeout,
        &should_stop,
        // worker はファイルを読んでサムネイルを書くだけ。DB には触れない。
        move |index, record: &HashRecord| match &book_for_workers {
            Some(book) => hash_one_amazon(book, &thumbnails_for_workers, index, record),
            None => hash_one(&thumbnails_for_workers, index, record),
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
    // リンクが消えていたら、ここで止めて理由を伝える。開いたときに自動では続けない。
    if amazon_book.as_ref().is_some_and(|book| book.is_gone()) {
        mark_amazon_gone(&conn, &project_id);
        return Err(amazon::GONE_MESSAGE.to_string());
    }
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

#[tauri::command(async)]
fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let conn = connection(&app)?;
    let mut statement = conn
        .prepare("SELECT id,name,folder_path,photo_count,status,created_at,updated_at,burst_threshold,burst_threshold_learned_at,source_kind,pair_raw_jpeg,pair_raw_jpeg_at FROM projects ORDER BY updated_at DESC")
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
                source_kind: row.get(9)?,
                pair_raw_jpeg: row.get::<_, i64>(10)? != 0,
                pair_raw_jpeg_at: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn create_project(app: AppHandle, name: String, folder_path: String) -> Result<Project, String> {
    if !Path::new(&folder_path).is_dir() {
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
        source_kind: SOURCE_FOLDER.into(),
        pair_raw_jpeg: true,
        pair_raw_jpeg_at: 0,
    };
    connection(&app)?
        .execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at,source_kind) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![project.id, project.name, project.folder_path, project.photo_count, project.status, project.created_at, project.updated_at, project.source_kind],
        )
        .map_err(|error| error.to_string())?;
    Ok(project)
}

// ---------------------------------------------------------------------------
// Amazon Photos の共有リンク（T9）。ログインしない。公開の JSON を読むだけ。
// 分岐は Rust の入口（走査・解析・表示用・書き出し・サイドカー）で `source_kind` を見て行い、
// 画面側には持たせない。
// ---------------------------------------------------------------------------

const SOURCE_FOLDER: &str = "folder";
const SOURCE_AMAZON: &str = "amazon";
const AMAZON_UNSUPPORTED: &str = "Amazon の写真には使えません。";

/// 写真の出所の種類と鍵。
fn project_source(conn: &Connection, project_id: &str) -> Result<(String, Option<String>), String> {
    conn.query_row(
        "SELECT source_kind,source_key FROM projects WHERE id=?1",
        params![project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|_| "Project was not found.".to_string())
}

/// Amazon のプロジェクトなら共有リンクの出所を返す。フォルダなら None。
fn amazon_source_of(conn: &Connection, project_id: &str) -> Result<Option<amazon::AmazonSource>, String> {
    let (kind, key) = project_source(conn, project_id)?;
    if kind != SOURCE_AMAZON {
        return Ok(None);
    }
    key.as_deref()
        .and_then(amazon::parse_key)
        .map(Some)
        .ok_or_else(|| "Amazon Photos のリンクの情報を読めません。".to_string())
}

/// Amazon のプロジェクトの tempLink の控えと取り直し。フォルダなら None。
fn amazon_book_of(app: &AppHandle, project_id: &str) -> Result<Option<Arc<amazon::LinkBook>>, String> {
    let conn = connection(app)?;
    let Some(source) = amazon_source_of(&conn, project_id)? else {
        return Ok(None);
    };
    let book = amazon::LinkBook::load(db_path(app)?, &conn, project_id, source)?;
    Ok(Some(Arc::new(book)))
}

/// リンクが消えていたと分かったとき。以後、開いたときに自動では読みにいかない。
fn mark_amazon_gone(conn: &Connection, project_id: &str) {
    let _ = conn.execute(
        "UPDATE projects SET status='missing',updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    );
}

/// 走査を途中でやめたとき・失敗したときの状態。写真があれば ready、なければ new。
fn settle_project_status(conn: &Connection, project_id: &str) {
    let _ = conn.execute(
        "UPDATE projects SET status=CASE WHEN photo_count>0 THEN 'ready' ELSE 'new' END,updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    );
}

/// 作成画面の見本。バイト列は JSON に載せず、ファイルに書いてパスを返す。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AmazonPreviewInfo {
    key: String,
    name: String,
    count: usize,
    /// 見本の JPEG のパス（アプリのデータフォルダの `samples/`）。
    samples: Vec<String>,
}

const AMAZON_SAMPLE_LIMIT: usize = 12;

fn clear_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

fn amazon_preview_blocking(app: AppHandle, share_url: String) -> Result<AmazonPreviewInfo, String> {
    let preview = amazon::preview(&share_url, AMAZON_SAMPLE_LIMIT)?;
    let dir = data_subdir(&app, SAMPLES_DIR)?;
    // 前の見本は要らない。名前を毎回変えるので、画面が古い画像を覚えていても取り違えない。
    clear_dir(&dir);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let batch = Uuid::new_v4();
    let mut samples = Vec::new();
    for (index, bytes) in preview.samples.iter().enumerate() {
        let file = dir.join(format!("{batch}-{index}.jpg"));
        if fs::write(&file, bytes).is_ok() {
            samples.push(file.to_string_lossy().to_string());
        }
    }
    Ok(AmazonPreviewInfo { key: preview.key, name: preview.name, count: preview.count, samples })
}

#[tauri::command]
async fn amazon_preview(app: AppHandle, share_url: String) -> Result<AmazonPreviewInfo, String> {
    tauri::async_runtime::spawn_blocking(move || amazon_preview_blocking(app, share_url))
        .await
        .map_err(|error| error.to_string())?
}

fn create_amazon_project_blocking(app: AppHandle, name: String, share_url: String) -> Result<Project, String> {
    let source = amazon::parse_share_url(&share_url)
        .ok_or_else(|| "Amazon Photos の共有リンクの形ではありません。".to_string())?;
    let name = name.trim();
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name: if name.is_empty() { "Amazon Photos".to_string() } else { name.to_string() },
        folder_path: share_url.trim().to_string(),
        photo_count: 0,
        status: "new".into(),
        created_at: now(),
        updated_at: now(),
        burst_threshold: None,
        burst_threshold_learned_at: None,
        source_kind: SOURCE_AMAZON.into(),
        pair_raw_jpeg: true,
        pair_raw_jpeg_at: 0,
    };
    connection(&app)?
        .execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at,source_kind,source_key) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![project.id, project.name, project.folder_path, project.photo_count, project.status, project.created_at, project.updated_at, project.source_kind, amazon::key_for(&source)],
        )
        .map_err(|error| error.to_string())?;
    // 見本はもう要らない。
    if let Ok(dir) = data_subdir(&app, SAMPLES_DIR) {
        clear_dir(&dir);
    }
    Ok(project)
}

#[tauri::command]
async fn create_amazon_project(app: AppHandle, name: String, share_url: String) -> Result<Project, String> {
    tauri::async_runtime::spawn_blocking(move || create_amazon_project_blocking(app, name, share_url))
        .await
        .map_err(|error| error.to_string())?
}

/// 原本を `amazon-cache/<project_id>/<node_id>.jpg` に置いてパスを返す。あれば使い回す。
/// 取れなかったら一覧を読み直して tempLink を取り直し、1 回だけやり直す（`LinkBook::fetch`）。
fn amazon_original_blocking(app: AppHandle, project_id: String, photo_id: String) -> Result<String, String> {
    let conn = connection(&app)?;
    let node_id: String = conn
        .query_row(
            "SELECT path FROM photos WHERE id=?1 AND project_id=?2",
            params![photo_id, project_id],
            |row| row.get(0),
        )
        .map_err(|_| "この写真は見つかりませんでした。".to_string())?;
    let Some(book) = amazon_book_of(&app, &project_id)? else {
        return Err("Amazon の写真ではありません。".into());
    };
    let dir = data_subdir(&app, AMAZON_CACHE_DIR)?.join(&project_id);
    let file = dir.join(format!("{}.jpg", amazon::safe_file_stem(&node_id)));
    if file.is_file() {
        return Ok(file.to_string_lossy().to_string());
    }
    let bytes = match book.fetch(&node_id, None) {
        Ok(bytes) => bytes,
        Err(message) => {
            if book.is_gone() {
                mark_amazon_gone(&conn, &project_id);
            }
            return Err(message);
        }
    };
    write_atomically(&file, &bytes)?;
    Ok(file.to_string_lossy().to_string())
}

#[tauri::command]
async fn amazon_original(app: AppHandle, project_id: String, photo_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || amazon_original_blocking(app, project_id, photo_id))
        .await
        .map_err(|error| error.to_string())?
}

/// 一時ファイルに書いてから置き換える。途中で切れた半端なファイルを残さない。
fn write_atomically(file: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = file.with_extension("part");
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, file).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}

/// まだ解析が必要な写真の枚数。0 なら事前生成を起動する意味がない。
///
/// これが無かったため、プロジェクトを開くたびに無条件で事前生成を起動しており、
/// 実際には何もすることが無くても進捗イベントだけが飛んで、UI に解析中の帯が
/// 一瞬出ていた。
#[tauri::command(async)]
fn get_analysis_backlog(app: AppHandle, project_id: String) -> Result<i64, String> {
    let conn = connection(&app)?;
    // Amazon の撮影時刻は走査で入る（contentDate が無い写真は空のまま）ので、空でも「未解析」に数えない。
    let is_amazon = amazon_source_of(&conn, &project_id)?.is_some();
    analysis_backlog(&conn, &project_id, is_amazon)
}

/// 「解析の対象なのに未処理」の枚数。
///
/// ハッシュ値・サムネイルを作る対象は、解析（`select_burst_candidates`）が選んだ連写の候補だけ
/// （Amazon は全部）。対象でない写真はハッシュ値もサムネイルも空のままなので、数えない。
/// 数えると、候補でない写真の分だけ backlog が 0 にならず、ホームがずっと「準備中」になる。
/// 撮影時刻が未読の写真は、対象が決まる前なので常に数える。
fn analysis_backlog(conn: &Connection, project_id: &str, is_amazon: bool) -> Result<i64, String> {
    struct Row {
        id: String,
        captured_at: Option<i64>,
        source: Option<String>,
        stale: bool,
    }
    let mut statement = conn
        .prepare(
            "SELECT id,captured_at,timestamp_source,
                    (d_hash IS NULL OR d_hash_version IS NULL OR d_hash_version <> ?2
                     OR thumbnail_path IS NULL OR thumbnail_version IS NULL OR thumbnail_version <> ?3)
             FROM photos
             WHERE project_id=?1 AND is_missing=0
             ORDER BY captured_at IS NULL, captured_at, path",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, D_HASH_VERSION, THUMBNAIL_VERSION], |row| {
            Ok(Row {
                id: row.get(0)?,
                captured_at: row.get(1)?,
                source: row.get(2)?,
                stale: row.get::<_, i64>(3)? != 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let selection = select_burst_candidates(
        &rows
            .iter()
            .filter_map(|row| {
                Some(CandidateInput {
                    id: row.id.clone(),
                    captured_at: row.captured_at?,
                    source: TimestampSource::parse(row.source.as_deref()),
                })
            })
            .collect::<Vec<_>>(),
    );
    let pending = rows
        .iter()
        .filter(|row| {
            let time_missing = !is_amazon && (row.captured_at.is_none() || row.source.is_none());
            let targeted = is_amazon || selection.ids.contains(&row.id);
            time_missing || (targeted && row.stale)
        })
        .count();
    Ok(pending as i64)
}

/// プロジェクトを削除する。**写真原本には一切触れない。**
/// 消すのは DB の行と、このアプリが `app_data_dir` 配下に作ったサムネイルだけ。
#[tauri::command]
async fn delete_project(app: AppHandle, project_id: String) -> Result<(), String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || delete_project_blocking(app, project_id))
        .await
        .map_err(|e| e.to_string())?
}

/// そのプロジェクトの表示用画像（`display/<photo_id>.jpg`）を消す。消すのは DB の
/// `display_path` が **表示用画像の置き場の直下** を指すファイルだけ。置き場の外を指す
/// 値（壊れた行・原本を指す値）には触れない。消せた数を返す（U27 R7）。
fn remove_project_display_files(
    conn: &Connection,
    project_id: &str,
    display_dir: &Path,
) -> Result<usize, String> {
    let mut statement = conn
        .prepare("SELECT display_path FROM photos WHERE project_id=?1 AND display_path IS NOT NULL")
        .map_err(|error| error.to_string())?;
    let paths = statement
        .query_map(params![project_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut removed = 0usize;
    for path in paths {
        let path = Path::new(&path);
        let inside = path.parent() == Some(display_dir)
            && path.file_name().is_some()
            && !path.components().any(|part| matches!(part, std::path::Component::ParentDir));
        if inside && fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn delete_project_blocking(app: AppHandle, project_id: String) -> Result<(), String> {
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

    // 表示用画像も、DB を消す前に（対象が分からなくなる前に）消す。
    let removed_display = match display_dir(&app) {
        Ok(dir) => remove_project_display_files(&conn, &project_id, &dir).unwrap_or(0),
        Err(_) => 0,
    };

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
            "DELETE FROM pair_overrides WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM sidecar_state WHERE project_id=?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
    amazon::delete_links(&transaction, &project_id)?;
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
    // Amazon の原本の置き場と、作成画面の見本。フォルダのプロジェクトでは無いので何も起きない。
    if let Ok(dir) = data_subdir(&app, AMAZON_CACHE_DIR) {
        clear_dir(&dir.join(&project_id));
    }
    if let Ok(dir) = data_subdir(&app, SAMPLES_DIR) {
        clear_dir(&dir);
    }
    eprintln!(
        "削除: プロジェクト {project_id} / サムネイル {removed} 件 / 表示用画像 {removed_display} 件"
    );
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
async fn get_project_photo_page(app: AppHandle, project_id: String, offset: i64, limit: i64, rating: Option<i64>, sort: Option<String>) -> Result<PhotoPage, String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || get_project_photo_page_blocking(app, project_id, offset, limit, rating, sort))
        .await
        .map_err(|e| e.to_string())?
}

fn get_project_photo_page_blocking(
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
async fn get_selection_summary(app: AppHandle, project_id: String) -> Result<SelectionSummary, String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || get_selection_summary_blocking(app, project_id))
        .await
        .map_err(|e| e.to_string())?
}

fn get_selection_summary_blocking(app: AppHandle, project_id: String) -> Result<SelectionSummary, String> {
    let conn = connection(&app)?;
    selection_summary(&conn, &project_id)
}

/// 星ごとの数。一覧（`get_project_photo_page`）と同じく、欠損（移動済みなど）は数えない。
fn selection_summary(conn: &Connection, project_id: &str) -> Result<SelectionSummary, String> {
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
    /// 組の RAW の `.xmp` に書けた数（U47。`processed` には含めない。失敗は `failed`）。
    paired_raw_processed: usize,
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

/// 対象の写真を取り出す。**対象は TS が決めた写真の id**（連写の仲間まで広げたあと）で、
/// 星では選ばない。id が空なら何も返さない。並びは相対パス順。
fn photos_for_export(
    conn: &Connection,
    project_id: &str,
    photo_ids: &[String],
) -> Result<Vec<(String, String, i64)>, String> {
    if photo_ids.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: std::collections::HashSet<&str> = photo_ids.iter().map(String::as_str).collect();
    let mut statement = conn
        .prepare("SELECT id,path,rating FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY relative_path")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (id, path, rating) = row.map_err(|error| error.to_string())?;
        if wanted.contains(id.as_str()) {
            out.push((id, path, rating));
        }
    }
    Ok(out)
}

/// 移動できた写真の行だけを欠損にする（場所が古くなったので、次の scan で拾い直させる）。
/// 移動しなかった写真や、失敗した写真は、そのまま。
fn mark_photos_missing(conn: &Connection, project_id: &str, ids: &[String]) -> Result<(), String> {
    for id in ids {
        conn.execute(
            "UPDATE photos SET is_missing=1 WHERE project_id=?1 AND id=?2",
            params![project_id, id],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
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

/// 選んだ写真を星ごとのフォルダへ書き出す。`star-5` `star-4` … を出力先に作る。
/// 対象は `photo_ids`（TS が連写の仲間まで広げて決める）。
///
/// `move_files` が false ならコピー。true なら移動で、**原本フォルダから写真が
/// 消える**。移動は「コピーしてから元を消す」順で行い、コピーに失敗したら
/// 元は残す。同じボリュームなら rename を試し、失敗したらコピーへ落とす。
#[tauri::command]
async fn export_photos(app: AppHandle, project_id: String, destination: String, photo_ids: Vec<String>, move_files: bool) -> Result<ExportReport, String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || export_photos_blocking(app, project_id, destination, photo_ids, move_files))
        .await
        .map_err(|e| e.to_string())?
}

fn export_photos_blocking(
    app: AppHandle,
    project_id: String,
    destination: String,
    photo_ids: Vec<String>,
    move_files: bool,
) -> Result<ExportReport, String> {
    let root = PathBuf::from(&destination);
    if !root.is_dir() {
        return Err("出力先フォルダが見つかりません。".into());
    }
    // Amazon の写真は、原本を取ってきて書き出し先に置く（移動はできない）。
    if let Some(book) = amazon_book_of(&app, &project_id)? {
        if move_files {
            return Err(AMAZON_UNSUPPORTED.into());
        }
        return export_amazon_copy(&app, &project_id, book, &root, &photo_ids);
    }
    // 出力先が写真フォルダの中だと、書き出した先をまた読んでしまう。
    let folder = project_folder(&app, &project_id)?;
    if root.starts_with(Path::new(&folder)) {
        return Err("出力先には、写真フォルダの外を指定してください。".into());
    }

    let conn = connection(&app)?;
    let targets = photos_for_export(&conn, &project_id, &photo_ids)?;
    let mut report = ExportReport::default();

    let mut moved_ids: Vec<String> = Vec::new();
    for (id, path, rating) in &targets {
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
                moved_ids.push(id.clone());
                continue;
            }
            match fs::copy(source, &target) {
                Ok(_) => match fs::remove_file(source) {
                    Ok(()) => {
                        report.processed += 1;
                        moved_ids.push(id.clone());
                    }
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
    if move_files {
        mark_photos_missing(&conn, &project_id, &moved_ids)?;
    }
    Ok(report)
}

/// Amazon の写真を星ごとのフォルダへコピーする。原本を Amazon から取り、書き出し先に直接置く
/// （端末の `amazon-cache` は使わない）。失敗した 1 枚で止めない。
fn export_amazon_copy(
    app: &AppHandle,
    project_id: &str,
    book: Arc<amazon::LinkBook>,
    root: &Path,
    photo_ids: &[String],
) -> Result<ExportReport, String> {
    let conn = connection(app)?;
    let targets: Vec<(String, String, String, i64)> = {
        let mut statement = conn
            .prepare("SELECT id,path,name,rating FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY relative_path")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    let wanted: std::collections::HashSet<&str> = photo_ids.iter().map(String::as_str).collect();
    let mut report = ExportReport::default();
    for (id, node_id, name, rating) in targets {
        if !wanted.contains(id.as_str()) {
            continue;
        }
        let directory = root.join(format!("star-{rating}"));
        if let Err(error) = fs::create_dir_all(&directory) {
            report.fail(&name, error);
            continue;
        }
        // 名前に区切りが入っていても、書き出し先の外には出さない。
        let file_name = Path::new(&name)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("photo.jpg");
        match book.fetch(&node_id, None) {
            Ok(bytes) => {
                let target = unique_destination(&directory, file_name);
                match fs::write(&target, &bytes) {
                    Ok(()) => report.processed += 1,
                    Err(error) => {
                        let _ = fs::remove_file(&target);
                        report.fail(&name, error);
                    }
                }
            }
            Err(message) => {
                report.fail(&name, message);
                // リンクが消えていたら、残りを 1 枚ずつ試さない。
                if book.is_gone() {
                    mark_amazon_gone(&conn, project_id);
                    break;
                }
            }
        }
    }
    Ok(report)
}

/// JPEG の XMP パケットを組み立てる。`xmp:Rating` は 0〜5 をそのまま持てる。
fn xmp_body(rating: i64) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

fn xmp_packet(rating: i64) -> Vec<u8> {
    let body = xmp_body(rating);
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
/// 対象は `photo_ids`（TS が連写の仲間まで広げて決める）。
#[tauri::command]
async fn write_ratings_to_photos(app: AppHandle, project_id: String, photo_ids: Vec<String>) -> Result<ExportReport, String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || write_ratings_to_photos_blocking(app, project_id, photo_ids))
        .await
        .map_err(|e| e.to_string())?
}

/// 書いたファイルが画像として開けるか確かめる。
/// **形式は拡張子ではなく中身から判定する。** 一時ファイルの拡張子（`.photocurator-tmp`）からは
/// 形式が分からず、確かめが毎回失敗して書き込みを取り消していた（新版の 6e56524 と同じ不具合）。
fn verify_image_file(path: &Path) -> Result<(), String> {
    image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---- 組の RAW の .xmp（U47）----------------------------------------------

/// XML のタグ 1 つの位置（`text` 内のバイト位置。`end` は `>` の次）。
struct XmlTag {
    start: usize,
    end: usize,
    name: String,
    is_end: bool,
    self_closing: bool,
}

/// XML のタグを前から拾う。コメント・処理命令・CDATA・DOCTYPE は読み飛ばす。
/// タグの入れ子が合っていなければ（`.xmp` が壊れている）Err。**XML として読めるかの検証を兼ねる。**
fn scan_xml_tags(text: &str) -> Result<Vec<XmlTag>, String> {
    let bytes = text.as_bytes();
    let mut tags = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0usize;
    let skip_to = |from: usize, needle: &str| -> Result<usize, String> {
        text[from..]
            .find(needle)
            .map(|offset| from + offset + needle.len())
            .ok_or_else(|| "XML が途中で終わっています".to_string())
    };
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let rest = &text[index..];
        if rest.starts_with("<!--") {
            index = skip_to(index + 4, "-->")?;
        } else if rest.starts_with("<?") {
            index = skip_to(index + 2, "?>")?;
        } else if rest.starts_with("<![CDATA[") {
            index = skip_to(index + 9, "]]>")?;
        } else if rest.starts_with("<!") {
            index = skip_to(index + 2, ">")?;
        } else {
            // 開始タグ・終了タグ。属性の値の中の `>` は無視する。
            let mut cursor = index + 1;
            let mut quote: Option<u8> = None;
            let mut close = None;
            while cursor < bytes.len() {
                let byte = bytes[cursor];
                match quote {
                    Some(q) => {
                        if byte == q {
                            quote = None;
                        }
                    }
                    None => {
                        if byte == b'"' || byte == b'\'' {
                            quote = Some(byte);
                        } else if byte == b'>' {
                            close = Some(cursor);
                            break;
                        }
                    }
                }
                cursor += 1;
            }
            let close = close.ok_or_else(|| "タグが閉じていません".to_string())?;
            let inner = &text[index + 1..close];
            let is_end = inner.starts_with('/');
            let self_closing = !is_end && inner.ends_with('/');
            let name_source = inner.trim_start_matches('/');
            let name: String = name_source
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
                .collect();
            if name.is_empty() {
                return Err("タグの名前がありません".into());
            }
            if is_end {
                match stack.pop() {
                    Some(open) if open == name => {}
                    _ => return Err(format!("タグの対応が合っていません: {name}")),
                }
            } else if !self_closing {
                stack.push(name.clone());
            }
            tags.push(XmlTag { start: index, end: close + 1, name, is_end, self_closing });
            index = close + 1;
        }
    }
    if !stack.is_empty() {
        return Err("閉じていないタグがあります".into());
    }
    if tags.is_empty() {
        return Err("XML のタグがありません".into());
    }
    Ok(tags)
}

/// 開始タグの属性（名前・値の範囲。範囲は `text` 内のバイト位置で、引用符の内側）。
fn tag_attributes(text: &str, tag: &XmlTag) -> Vec<(String, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let limit = tag.end - 1;
    let mut cursor = tag.start + 1;
    // タグの名前を飛ばす。
    while cursor < limit && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    loop {
        while cursor < limit && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/') {
            cursor += 1;
        }
        if cursor >= limit {
            break;
        }
        let name_start = cursor;
        while cursor < limit && bytes[cursor] != b'=' && !bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let name = text[name_start..cursor].to_string();
        while cursor < limit && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= limit || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < limit && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= limit || (bytes[cursor] != b'"' && bytes[cursor] != b'\'') {
            break;
        }
        let quote = bytes[cursor];
        let value_start = cursor + 1;
        let Some(length) = bytes[value_start..limit].iter().position(|b| *b == quote) else {
            break;
        };
        out.push((name, value_start..value_start + length));
        cursor = value_start + length + 1;
    }
    out
}

/// `.xmp` の中の `xmp:Rating`（属性・要素）の値を、見つけた順に返す。読めなければ Err。
fn xmp_ratings_of_document(text: &str) -> Result<Vec<String>, String> {
    let tags = scan_xml_tags(text)?;
    let mut found = Vec::new();
    for (position, tag) in tags.iter().enumerate() {
        if tag.is_end {
            continue;
        }
        for (name, range) in tag_attributes(text, tag) {
            if name == "xmp:Rating" {
                found.push(text[range].trim().to_string());
            }
        }
        if tag.name == "xmp:Rating" && !tag.self_closing {
            if let Some(next) = tags.get(position + 1) {
                if next.is_end && next.name == "xmp:Rating" {
                    found.push(text[tag.end..next.start].trim().to_string());
                }
            }
        }
    }
    Ok(found)
}

/// 既にある `.xmp` の星（`xmp:Rating`）だけを書き換える。ほかの内容は 1 バイトも変えない。
/// `xmp:Rating` が無ければ、最初の `rdf:Description` に属性として足す。
/// 読めない・`rdf:Description` が無いときは Err（呼び出し側は何も書かない）。
fn xmp_with_rating(existing: &str, rating: i64) -> Result<String, String> {
    let tags = scan_xml_tags(existing)?;
    let value = rating.to_string();
    // (差し替える範囲, 入れる文字列)
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (position, tag) in tags.iter().enumerate() {
        if tag.is_end {
            continue;
        }
        for (name, range) in tag_attributes(existing, tag) {
            if name == "xmp:Rating" {
                edits.push((range, value.clone()));
            }
        }
        if tag.name == "xmp:Rating" && !tag.self_closing {
            if let Some(next) = tags.get(position + 1) {
                if next.is_end && next.name == "xmp:Rating" {
                    edits.push((tag.end..next.start, value.clone()));
                }
            }
        }
    }
    if edits.is_empty() {
        let description = tags
            .iter()
            .find(|tag| !tag.is_end && tag.name == "rdf:Description")
            .ok_or_else(|| "rdf:Description が見つかりません".to_string())?;
        let has_namespace = tag_attributes(existing, description)
            .iter()
            .any(|(name, _)| name == "xmlns:xmp");
        let insert_at = description.end - if description.self_closing { 2 } else { 1 };
        let mut addition = String::new();
        if !has_namespace {
            addition.push_str(" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"");
        }
        addition.push_str(&format!(" xmp:Rating=\"{value}\""));
        edits.push((insert_at..insert_at, addition));
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = existing.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    Ok(out)
}

/// 新しく作る `.xmp` の中身（最小の XMP パケット）。
fn new_sidecar_xmp(rating: i64) -> String {
    xmp_body(rating)
}

/// JPEG（`.jpg`・`.jpeg`）の組の RAW を、同じフォルダの一覧から選ぶ純関数。
/// 拡張子を除いた名前が大文字小文字を無視して一致し、拡張子が RAW の一覧にあるもの。
/// フォルダが違うものは組ではない。並びはパス順。
fn paired_raw_files(jpeg: &Path, siblings: &[PathBuf]) -> Vec<PathBuf> {
    let Some(stem) = jpeg.file_stem().map(|v| v.to_string_lossy().to_lowercase()) else {
        return Vec::new();
    };
    let folder = jpeg.parent();
    let mut found: Vec<PathBuf> = siblings
        .iter()
        .filter(|candidate| {
            candidate.parent() == folder
                && candidate
                    .file_stem()
                    .is_some_and(|v| v.to_string_lossy().to_lowercase() == stem)
                && extension_lower(candidate).is_some_and(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
        })
        .cloned()
        .collect();
    found.sort();
    found
}

/// JPEG と同じフォルダのファイルを列挙して、組の RAW を返す（読めなければ空）。
fn find_paired_raws(jpeg: &Path) -> Vec<PathBuf> {
    let Some(folder) = jpeg.parent() else {
        return Vec::new();
    };
    let siblings: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => return Vec::new(),
    };
    paired_raw_files(jpeg, &siblings)
}

/// RAW の隣の `.xmp` に星を書く。**RAW 本体は読みも書きもしない。**
/// 既にあれば（名前の大文字小文字は問わない）`xmp:Rating` だけを更新し、無ければ新しく作る。
/// 同じフォルダの一時ファイルへ書き、XML として読めて星が期待どおりか確かめてから置き換える。
/// 読み取り専用の `.xmp`・UTF-8 でない `.xmp`・構造が読めない `.xmp` には触らず Err。
fn write_sidecar_xmp_for_raw(raw: &Path, rating: i64) -> Result<PathBuf, String> {
    let folder = raw.parent().ok_or("RAW のフォルダが分かりません")?;
    let stem = raw.file_stem().ok_or("RAW の名前が分かりません")?;
    let wanted = format!("{}.xmp", stem.to_string_lossy().to_lowercase());
    let existing: Option<PathBuf> = fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase() == wanted)
        });

    let (target, content) = match &existing {
        Some(path) => {
            let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
            if metadata.permissions().readonly() {
                return Err("既存の .xmp が読み取り専用のため書きませんでした".into());
            }
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let text = String::from_utf8(bytes)
                .map_err(|_| "既存の .xmp が UTF-8 ではないため書きませんでした".to_string())?;
            let updated = xmp_with_rating(&text, rating)
                .map_err(|reason| format!("既存の .xmp を読めないため書きませんでした: {reason}"))?;
            (path.clone(), updated)
        }
        None => {
            let mut name = stem.to_os_string();
            name.push(".xmp");
            (folder.join(name), new_sidecar_xmp(rating))
        }
    };

    let mut temporary_name = target.file_name().unwrap_or_default().to_os_string();
    temporary_name.push(".photocurator-tmp");
    let temporary = folder.join(temporary_name);
    if let Err(error) = fs::write(&temporary, content.as_bytes()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    let expected = rating.to_string();
    let verified = fs::read_to_string(&temporary)
        .map_err(|error| error.to_string())
        .and_then(|text| xmp_ratings_of_document(&text))
        .and_then(|ratings| {
            if !ratings.is_empty() && ratings.iter().all(|value| *value == expected) {
                Ok(())
            } else {
                Err("星が期待どおりに書けていません".to_string())
            }
        });
    if let Err(reason) = verified {
        let _ = fs::remove_file(&temporary);
        return Err(format!("検証に失敗したため .xmp は変更していません: {reason}"));
    }
    match fs::rename(&temporary, &target) {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error.to_string())
        }
    }
}

/// 対象の写真（id・パス・星）に星を書く。JPEG は中の XMP に、
/// `pair_raw` がオンなら組の RAW の隣の `.xmp` にも。JPEG の書き込みが失敗したら RAW 側は書かない。
fn write_ratings_to_targets(targets: &[(String, String, i64)], pair_raw: bool) -> ExportReport {
    let mut report = ExportReport::default();

    for (_id, path, rating) in targets {
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
        if let Err(error) = verify_image_file(&temporary) {
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
                continue;
            }
        }

        // 組の RAW の隣の .xmp（U47）。オフのときは RAW が自分の行で扱われるので何もしない。
        // 同じ名前の RAW が複数あっても `.xmp` は 1 つなので、1 回だけ書く。
        if pair_raw {
            let mut written: HashSet<PathBuf> = HashSet::new();
            for raw in find_paired_raws(source) {
                let key = raw.with_extension("").to_string_lossy().to_lowercase();
                if !written.insert(PathBuf::from(key)) {
                    continue;
                }
                match write_sidecar_xmp_for_raw(&raw, *rating) {
                    Ok(_) => report.paired_raw_processed += 1,
                    Err(reason) => report.fail(&raw.to_string_lossy(), reason),
                }
            }
        }
    }
    report
}

fn write_ratings_to_photos_blocking(
    app: AppHandle,
    project_id: String,
    photo_ids: Vec<String>,
) -> Result<ExportReport, String> {
    let conn = connection(&app)?;
    // Amazon の写真の原本は書き換えられない。
    if amazon_source_of(&conn, &project_id)?.is_some() {
        return Err(AMAZON_UNSUPPORTED.into());
    }
    let targets = photos_for_export(&conn, &project_id, &photo_ids)?;
    let pair_raw = project_pair_raw(&conn, &project_id);
    Ok(write_ratings_to_targets(&targets, pair_raw))
}

/// 結果の CSV を、保存ダイアログで選ばれた場所へ書く。**書けるのは `.csv` だけ**
/// （画面が渡すのは保存ダイアログの結果と、TS が作った CSV の文）。
#[tauri::command]
async fn write_text_file(path: String, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = PathBuf::from(&path);
        let is_csv = target
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("csv"));
        if !is_csv {
            return Err("CSV のファイルにだけ書けます。".to_string());
        }
        fs::write(&target, text.as_bytes()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(async)]
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

/// core に渡す写真の行。**その星に関係なく全件**（欠損を除く）を 1 回で返す。
/// 並べ替えはフロント（`utils/coreInputs.ts`）が撮影順にする。
#[tauri::command]
async fn get_core_inputs(app: AppHandle, project_id: String) -> Result<Vec<Photo>, String> {
    // 重い処理（ディスクと SQLite）はメインスレッドから外す。
    tauri::async_runtime::spawn_blocking(move || get_core_inputs_blocking(app, project_id))
        .await
        .map_err(|e| e.to_string())?
}

fn get_core_inputs_blocking(app: AppHandle, project_id: String) -> Result<Vec<Photo>, String> {
    let conn = connection(&app)?;
    let mut statement = conn
        .prepare(&format!(
            "SELECT {PHOTO_COLUMNS} FROM photos WHERE project_id=?1 AND is_missing=0"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], photo_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// 欠損の印の写真のうち、星が 1 以上のもの（U52 D13）。サイドカーに載せて、一時的に見えないだけの写真の
/// 星を NAS から消さないために使う。
#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
struct MissingRating {
    relative_path: String,
    rating: i64,
}

fn missing_ratings(conn: &Connection, project_id: &str) -> Result<Vec<MissingRating>, String> {
    let mut statement = conn
        .prepare(
            "SELECT relative_path, rating FROM photos WHERE project_id=?1 AND is_missing=1 AND rating>=1
             ORDER BY relative_path",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(MissingRating { relative_path: row.get(0)?, rating: row.get(1)? })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_missing_ratings(app: AppHandle, project_id: String) -> Result<Vec<MissingRating>, String> {
    tauri::async_runtime::spawn_blocking(move || missing_ratings(&connection(&app)?, &project_id))
        .await
        .map_err(|e| e.to_string())?
}

/// 手で直した連写の例外（core の `PairOverride`）。鍵は relativePath。
#[derive(Clone, Serialize, serde::Deserialize)]
struct PairOverrideRow {
    left: String,
    right: String,
    decision: String,
}

#[tauri::command(async)]
fn get_pair_overrides(app: AppHandle, project_id: String) -> Result<Vec<PairOverrideRow>, String> {
    let conn = connection(&app)?;
    let mut statement = conn
        .prepare(
            "SELECT left_path,right_path,decision FROM pair_overrides WHERE project_id=?1
             ORDER BY left_path,right_path",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(PairOverrideRow {
                left: row.get(0)?,
                right: row.get(1)?,
                decision: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// そのプロジェクトの例外を、渡したものに丸ごと入れ替える。
#[tauri::command]
fn save_pair_overrides(
    app: AppHandle,
    project_id: String,
    overrides: Vec<PairOverrideRow>,
) -> Result<(), String> {
    let mut conn = connection(&app)?;
    replace_pair_overrides(&mut conn, &project_id, &overrides)
}

fn replace_pair_overrides(
    conn: &mut Connection,
    project_id: &str,
    overrides: &[PairOverrideRow],
) -> Result<(), String> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    tx.execute(
        "DELETE FROM pair_overrides WHERE project_id=?1",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;
    for item in overrides {
        // 同じ組が重ねて来たら、**先に出たものが勝つ**（core の `group_bursts` と同じ。U41。
        // 以前は後のものが勝ち、core と食い違っていた）。
        tx.execute(
            "INSERT INTO pair_overrides (project_id,left_path,right_path,decision)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project_id,left_path,right_path) DO NOTHING",
            params![project_id, item.left, item.right, item.decision],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// 表示用画像の設定と生成
// ---------------------------------------------------------------------------

const SETTING_DISPLAY_EDGE: &str = "display_edge";

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
    /// プロジェクトを渡したとき、そのプロジェクトで実際に使う長辺（上書き → 全体の既定）。
    /// 読むだけで、上書きは消さない。
    project_edge: Option<u32>,
}

#[tauri::command(async)]
fn get_display_settings(app: AppHandle, project_id: Option<String>) -> Result<DisplaySettings, String> {
    let project_edge = match project_id {
        Some(id) => Some(resolve_display_edge(&app, &id)?),
        None => None,
    };
    let conn = connection(&app)?;
    let edge = read_setting(&conn, SETTING_DISPLAY_EDGE)
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(DISPLAY_EDGE_DEFAULT, normalize_display_edge);
    Ok(DisplaySettings {
        edge,
        choices: DISPLAY_EDGES.to_vec(),
        default_edge: DISPLAY_EDGE_DEFAULT,
        large_edge: DISPLAY_EDGE_LARGE,
        project_edge,
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

/// プロジェクトの「同名の JPEG と RAW を 1 枚の写真として扱う」設定（U46）。
/// 既定はオン。読めない・行が無いときもオン（既定どおり）。
fn project_pair_raw(conn: &Connection, project_id: &str) -> bool {
    conn.query_row(
        "SELECT pair_raw_jpeg FROM projects WHERE id=?1",
        params![project_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
    .unwrap_or(true)
}

/// 設定と、切り替えた時刻を保存する（U48）。`at` が None なら今の時刻（画面で切り替えたとき）、
/// Some ならその時刻（ほかの端末の設定をサイドカーから取り込んだとき）。
fn store_project_pair_raw(
    conn: &Connection,
    project_id: &str,
    enabled: bool,
    at: Option<i64>,
) -> Result<bool, String> {
    let stamp = now();
    conn.execute(
        "UPDATE projects SET pair_raw_jpeg=?1, pair_raw_jpeg_at=?2, updated_at=?3 WHERE id=?4",
        params![enabled as i64, at.unwrap_or(stamp), stamp, project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(project_pair_raw(conn, project_id))
}

/// 設定を保存する。反映は次の走査（「写真を再読み込み」）から。
#[tauri::command]
fn save_project_pair_raw(
    app: AppHandle,
    project_id: String,
    enabled: bool,
    at: Option<i64>,
) -> Result<bool, String> {
    let conn = connection(&app)?;
    store_project_pair_raw(&conn, &project_id, enabled, at)
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
#[tauri::command(async)]
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

/// 表示用画像を作る 1 枚。
#[derive(Clone, Debug)]
struct DisplayJob {
    id: String,
    path: String,
    stored_path: Option<String>,
    stored_edge: Option<i64>,
}

impl PhotoJob for DisplayJob {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

/// 表示用画像 1 枚を作ってファイルへ書く（worker 側。DB には触れない）。
/// 作れなかったら `error` に理由を入れる（writer が印を NULL に戻す）。
fn display_one(dir: &Path, edge: u32, index: usize, job: &DisplayJob) -> PhotoWork {
    let mut work = PhotoWork::new(index, &job.id);
    let existing = match (job.stored_path.as_deref(), job.stored_edge) {
        (Some(p), Some(e)) if e > 0 => Some((Path::new(p), e as u32)),
        _ => None,
    };
    let built = build_display(&LocalPhoto(Path::new(&job.path)), edge, existing);
    let file = display_file(dir, &job.id);
    let saved = built.and_then(|bytes| fs::write(&file, &bytes).ok());
    if saved.is_none() {
        work.error = Some("表示用の画像を作れませんでした。".into());
    }
    work
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

    // Amazon は縮小を Amazon にさせる（`viewBox=<長辺>`）。原本は読まない。
    if let Some(book) = amazon_book_of(&app, &project_id)? {
        let targets = pending
            .into_iter()
            .map(|(photo_id, node_id, _, _)| (photo_id, node_id))
            .collect();
        return run_amazon_display(&app, registry, &project_id, book, edge, &dir, targets);
    }

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

    // 原本を読んで表示用に縮める工程は `run_in_parallel` に載せる（U27 R2）。worker は
    // 画像を作ってファイルへ書くだけで DB に触れない。単一の writer が 100 件ずつ UPDATE する。
    // 並列数は解析と同じ（ローカルは 2〜4、ネットワークのフォルダは 2）。
    let folder = project_folder(&app, &project_id)?;
    let workers = analysis_worker_count_for(Path::new(&folder));
    let jobs: Vec<DisplayJob> = pending
        .into_iter()
        .map(|(id, path, stored_path, stored_edge)| DisplayJob {
            id,
            path,
            stored_path,
            stored_edge,
        })
        .collect();
    let conn = connection(&app)?;
    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        if item.error.is_none() {
            tx.execute(
                "UPDATE photos SET display_path=?1, display_edge=?2 WHERE id=?3",
                params![
                    display_file(&dir, &item.photo_id).to_string_lossy().to_string(),
                    edge as i64,
                    item.photo_id
                ],
            )
        } else {
            // 作れなかった写真は次回また拾えるよう、印を残さない。
            tx.execute(
                "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                params![item.photo_id],
            )
        }
        .map_err(|error| error.to_string())?;
        Ok(())
    };
    let mut pending_results: Vec<PhotoWork> = Vec::new();
    let mut committed = 0usize;
    let dir_for_workers = dir.clone();
    let outcome = run_in_parallel(
        Arc::new(jobs),
        workers,
        Duration::from_millis(DISPLAY_TIMEOUT_MS),
        &|| registry.is_cancelled(&task_key),
        move |index, job: &DisplayJob| display_one(&dir_for_workers, edge, index, job),
        &mut |item| {
            pending_results.push(item);
            if pending_results.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending_results, &apply)?;
            }
            let done = committed + pending_results.len();
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
            Ok(())
        },
    )?;
    committed += flush_results(&conn, &mut pending_results, &apply)?;
    if outcome.cancelled {
        progress_note(
            &app,
            &project_id,
            "display",
            "cancelled",
            committed,
            total,
            format!("中断しました。{committed} 件まで作成済みです。"),
            ProgressNote {
                warning: None,
                failed: 0,
            },
        );
        return Ok(());
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

/// `run_display_generation` の Amazon 版。`viewBox=<表示用の長辺>` のバイトを、そのまま表示用のファイルに書く。
/// 並列 4。まとまり（16 枚）ごとに DB へ書き、中断とリンク切れを見る。
fn run_amazon_display(
    app: &AppHandle,
    registry: &TaskRegistry,
    project_id: &str,
    book: Arc<amazon::LinkBook>,
    edge: u32,
    dir: &Path,
    pending: Vec<(String, String)>,
) -> Result<(), String> {
    let task_key = format!("display:{project_id}");
    let total = pending.len();
    let message = "選別用の画像を作っています…";
    progress(app, project_id, "display", "hashing", 0, total, message);
    let mut done = 0usize;
    let mut failed = 0usize;
    for batch in pending.chunks(amazon::WORKERS * 4) {
        if registry.is_cancelled(&task_key) {
            progress_note(
                app,
                project_id,
                "display",
                "cancelled",
                done,
                total,
                format!("中断しました。{done} 件まで作成済みです。"),
                ProgressNote { warning: None, failed },
            );
            return Ok(());
        }
        if book.is_gone() {
            break;
        }
        let next = AtomicUsize::new(0);
        let saved: Mutex<Vec<(usize, bool)>> = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..amazon::WORKERS.min(batch.len()) {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some((photo_id, node_id)) = batch.get(index) else { break };
                    let ok = book
                        .fetch(node_id, Some(edge))
                        .ok()
                        // 画像として読める形のときだけ置く（エラーの文書を表示用にしない）。
                        .filter(|bytes| image::guess_format(bytes).is_ok())
                        .is_some_and(|bytes| write_atomically(&display_file(dir, photo_id), &bytes).is_ok());
                    if let Ok(mut list) = saved.lock() {
                        list.push((index, ok));
                    }
                });
            }
        });
        let results = saved.into_inner().map_err(|_| "表示用画像の結果を読めません。".to_string())?;
        let conn = connection(app)?;
        for (index, ok) in results {
            let photo_id = &batch[index].0;
            if ok {
                conn.execute(
                    "UPDATE photos SET display_path=?1, display_edge=?2 WHERE id=?3",
                    params![display_file(dir, photo_id).to_string_lossy().to_string(), edge as i64, photo_id],
                )
                .map_err(|error| error.to_string())?;
            } else {
                // 作れなかった写真は次回また拾えるよう、印を残さない。
                failed += 1;
                conn.execute(
                    "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                    params![photo_id],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        done += batch.len();
        progress_note(
            app,
            project_id,
            "display",
            "hashing",
            done,
            total,
            message,
            ProgressNote { warning: None, failed },
        );
    }
    if book.is_gone() {
        mark_amazon_gone(&connection(app)?, project_id);
        return Err(amazon::GONE_MESSAGE.to_string());
    }
    progress_note(
        app,
        project_id,
        "display",
        "complete",
        total,
        total,
        "選別用の画像がそろいました。",
        ProgressNote { warning: None, failed },
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

    pub use super::capture::TimestampSource;
    pub use super::{
        AnalysisOutcome, CachedAnalysis, CandidateInput, CandidateSelection, DecodeSource,
        ThumbnailState,
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
        super::analyse_photo(thumbnail_dir, photo_id, source, current, cached)
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

}


// ---------------------------------------------------------------------------
// サイドカー（`.photo-curator/catalog.json`）。判断は画面側の core が行う。
// ---------------------------------------------------------------------------

fn sidecar_folder(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(project_folder(app, project_id)?))
}

#[tauri::command]
async fn sidecar_supported(app: AppHandle, project_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Amazon の共有リンクには、書く場所が無い。
        if amazon_source_of(&connection(&app)?, &project_id)?.is_some() {
            return Ok("none".to_string());
        }
        Ok(sidecar::support(&sidecar_folder(&app, &project_id)?).to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn read_sidecar(app: AppHandle, project_id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if amazon_source_of(&connection(&app)?, &project_id)?.is_some() {
            return Ok(None);
        }
        sidecar::read(&sidecar_folder(&app, &project_id)?)
    })
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn write_sidecar(
    app: AppHandle,
    project_id: String,
    json: String,
    file_name: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if amazon_source_of(&connection(&app)?, &project_id)?.is_some() {
            return Err("Amazon の共有リンクにはサイドカーを書けません。".to_string());
        }
        let name = file_name.unwrap_or_else(|| sidecar::SIDECAR_FILE.to_string());
        sidecar::write(&sidecar_folder(&app, &project_id)?, &name, &json)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// `catalog.json` を楽観ロックで書く（U34。設計書 §4.4）。`expected` は画面が判断に使った中身
/// （無かったなら None）。`aside_tag` があれば、確かめたあとで置き換える版を退避する（U52 D4）。
/// 返すのは "written" / "changed" / "locked"。
#[tauri::command]
async fn write_sidecar_checked(
    app: AppHandle,
    project_id: String,
    json: String,
    expected: Option<String>,
    aside_tag: Option<String>,
) -> Result<sidecar::CheckedWrite, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = connection(&app)?;
        if amazon_source_of(&conn, &project_id)?.is_some() {
            return Err("Amazon の共有リンクにはサイドカーを書けません。".to_string());
        }
        let holder = sidecar::device_identity(&conn)?;
        sidecar::write_checked_aside(
            &sidecar_folder(&app, &project_id)?,
            &json,
            expected.as_deref(),
            &format!("{} ({})", holder.name, holder.id),
            aside_tag.as_deref(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

/// NAS に退避する（U52 D4。`catalog.<印>.<UTC 時刻>.json` を無いときだけ作り、同じ印は最新 5 つ）。返すのは名前。
#[tauri::command]
async fn aside_sidecar(app: AppHandle, project_id: String, json: String, tag: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if amazon_source_of(&connection(&app)?, &project_id)?.is_some() {
            return Err("Amazon の共有リンクにはサイドカーを書けません。".to_string());
        }
        sidecar::write_aside(&sidecar_folder(&app, &project_id)?, &tag, &json)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 端末の中に退避する（U52 D4。アプリのデータフォルダの `aside/`、プロジェクトごとに最新 5 つ）。
#[tauri::command]
async fn aside_local(app: AppHandle, project_id: String, json: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        sidecar::write_local_aside(&data_subdir(&app, "aside")?, &project_id, &json)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(async)]
fn load_sidecar_state(app: AppHandle, project_id: String) -> Result<sidecar::SidecarState, String> {
    sidecar::load_state(&connection(&app)?, &project_id)
}

#[tauri::command]
fn save_sidecar_state(
    app: AppHandle,
    project_id: String,
    state: sidecar::SidecarState,
) -> Result<(), String> {
    sidecar::save_state(&connection(&app)?, &project_id, &state)
}

#[tauri::command(async)]
fn device_identity(app: AppHandle) -> Result<sidecar::DeviceIdentity, String> {
    sidecar::device_identity(&connection(&app)?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(TaskRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            delete_project,
            amazon_preview,
            create_amazon_project,
            amazon_original,
            get_analysis_backlog,
            get_project_photo_page,
            get_photos_by_ids,
            save_selection_results,
            reset_selection_results,
            move_rating,
            get_selection_summary,
            export_photos,
            write_ratings_to_photos,
            write_text_file,
            get_display_settings,
            save_display_edge,
            save_project_display_edge,
            save_project_pair_raw,
            get_display_backlog,
            start_display_generation,
            reset_display_images,
            get_core_inputs,
            get_pair_overrides,
            save_pair_overrides,
            save_burst_threshold,
            clear_burst_threshold,
            start_project_scan,
            start_burst_analysis,
            start_background_analysis,
            cancel_project_task,
            save_project_state,
            load_project_state,
            sidecar_supported,
            read_sidecar,
            write_sidecar,
            write_sidecar_checked,
            get_missing_ratings,
            aside_sidecar,
            aside_local,
            load_sidecar_state,
            save_sidecar_state,
            device_identity
        ])
        .run(tauri::generate_context!())
        .expect("error while running Photo Curator");
}

#[cfg(test)]
mod tests;
