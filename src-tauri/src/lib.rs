mod amazon;
mod format;
mod sidecar;
mod capture;
mod image_pipeline;
mod candidates;
mod scan;
mod xmp;
mod db;
mod parallel;
mod analysis;
mod display;
mod export;

use capture::*;
use image_pipeline::*;
use candidates::*;
use scan::*;
use xmp::*;
use db::*;
use parallel::*;
use analysis::*;
use display::*;
use export::*;

// U54 で移す前から crate の外に見えていた型は、移したあとも同じ名前（crate 直下）で見せる。
pub use candidates::{CandidateInput, CandidateSelection, PairEligibility};
pub use capture::{CaptureTime, LocalPhoto, PhotoSource, TimestampSource};
pub use image_pipeline::{AnalysisOutcome, CachedAnalysis, DecodeSource, ThumbnailState};

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
