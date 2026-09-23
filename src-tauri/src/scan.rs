//! 走査・準備（設計 07 章 段4-2）。
//!
//! scan → meta（撮影時刻・指紋・サムネイル）→ display の 3 段。
//! 50 枚ごとに保存し、中断しても「やった分から続く」（B-5）。
//! ディスク I/O 律速なので、worker は 2〜4 に絞る（旧実装の知見をそのまま採用）。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::exif_util::{self, EXIF_HEAD_PROBE};
use crate::format;
use crate::image_pipeline;
use crate::model::{PrepareProgress, PrepareTask, Project, ProjectPhoto};
use crate::store;

const MAX_WORKERS: usize = 4;
const MIN_WORKERS: usize = 2;
pub(crate) const CHECKPOINT_EVERY: usize = 50;
/// フォルダ一覧・作成画面で「見本」や hasPhotos を判定するときだけ読む先頭バイト数。
const SNIFF_PROBE: usize = 16;

pub(crate) fn worker_count() -> usize {
    if let Ok(raw) = std::env::var("PHOTO_CURATOR_WORKERS") {
        if let Ok(n) = raw.parse::<usize>() {
            return n.max(1);
        }
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (cores / 2).clamp(MIN_WORKERS, MAX_WORKERS)
}

/// 準備の実行中フラグ。`prepare` と `cancel_prepare` が同じプロジェクト ID で共有する。
#[derive(Default)]
pub struct PrepareRegistry {
    flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl PrepareRegistry {
    pub fn begin(&self, project_id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut flags = self.flags.lock().map_err(|_| "内部状態を取れません".to_string())?;
        if flags.contains_key(project_id) {
            return Err("このプロジェクトはすでに準備中です。".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        flags.insert(project_id.to_string(), flag.clone());
        Ok(flag)
    }

    pub fn finish(&self, project_id: &str) {
        if let Ok(mut flags) = self.flags.lock() {
            flags.remove(project_id);
        }
    }

    pub fn cancel(&self, project_id: &str) {
        if let Ok(flags) = self.flags.lock() {
            if let Some(flag) = flags.get(project_id) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// 先頭数バイトを読む。ファイルが短ければ全部返す。
fn read_head(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; limit];
    let mut total = 0usize;
    loop {
        let read = file.read(&mut buffer[total..]).ok()?;
        if read == 0 {
            break;
        }
        total += read;
        if total >= buffer.len() {
            break;
        }
    }
    buffer.truncate(total);
    Some(buffer)
}

pub(crate) fn run_parallel<T, R, F>(items: &[T], workers: usize, cancel: &AtomicBool, work: F, mut on_result: impl FnMut(usize, R))
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    if items.is_empty() {
        return;
    }
    let workers = workers.clamp(1, MAX_WORKERS).min(items.len());
    let cursor = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, R)>();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let work = &work;
            let cursor = &cursor;
            scope.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let index = cursor.fetch_add(1, Ordering::SeqCst);
                if index >= items.len() {
                    break;
                }
                let result = work(&items[index]);
                if tx.send((index, result)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (index, result) in rx.iter() {
            on_result(index, result);
        }
    });
}

// ---------------------------------------------------------------------------
// フォルダ一覧（作成画面・「中へ」）
// ---------------------------------------------------------------------------

pub fn resolve_path(root: &str, sub_path: &str) -> PathBuf {
    let mut path = PathBuf::from(root);
    if !sub_path.is_empty() {
        for part in sub_path.split('/').filter(|p| !p.is_empty()) {
            path.push(part);
        }
    }
    path
}

fn is_image_file(path: &Path) -> bool {
    let Some(head) = read_head(path, SNIFF_PROBE) else { return false };
    format::is_image(format::sniff(&head))
}

pub fn list_entries(root: &str, sub_path: &str) -> Result<Vec<crate::model::FolderEntry>, String> {
    let base = resolve_path(root, sub_path);
    let read = fs::read_dir(&base).map_err(|error| format!("フォルダを読めません: {error}"))?;
    let mut entries = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = item.file_name().to_str().map(str::to_owned) else { continue };
        if name.starts_with('.') {
            continue;
        }
        let mut has_photos = false;
        let mut has_children = false;
        if let Ok(children) = fs::read_dir(&path) {
            for child in children.flatten() {
                let child_path = child.path();
                if child_path.is_dir() {
                    if !child.file_name().to_string_lossy().starts_with('.') {
                        has_children = true;
                    }
                } else if !has_photos
                    && !child.file_name().to_string_lossy().starts_with('.')
                    && is_image_file(&child_path)
                {
                    has_photos = true;
                }
                if has_photos && has_children {
                    break;
                }
            }
        }
        if !has_photos && !has_children {
            continue; // 空フォルダは出さない
        }
        let key = if sub_path.is_empty() { name.clone() } else { format!("{sub_path}/{name}") };
        entries.push(crate::model::FolderEntry { name, key, has_photos, has_children, sample: None });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// 作成画面の右側「枚数・見本12枚」（設計 02 章「見本の絵」節・PC・Web の差分表）。
/// **下位フォルダも含めて数える**（`prepare` の `walk_all_files` と同じ範囲。
/// 実際に準備で拾う枚数と食い違わないようにする）。原本は読まない。
pub fn sample_folder(root: &str, sub_path: &str, limit: usize) -> Result<crate::model::FolderSample, String> {
    let base = resolve_path(root, sub_path);
    if !base.is_dir() {
        return Err(format!("フォルダが見つかりません: {}", base.display()));
    }
    let raw = walk_all_files(&base);
    let mut count = 0usize;
    let mut samples = Vec::new();
    for entry in &raw {
        if !is_image_file(&entry.absolute_path) {
            continue; // 動画・非対応形式は数えない（`prepare` の scan 段と同じ判定）
        }
        count += 1;
        if samples.len() >= limit {
            continue;
        }
        let Some(head) = read_head(&entry.absolute_path, EXIF_HEAD_PROBE) else { continue };
        let Some(image) = image_pipeline::decode_hash_source(&entry.absolute_path, &head) else { continue };
        let thumbnail = image_pipeline::scale_for_thumbnail(&image);
        if let Some(bytes) = image_pipeline::encode_thumbnail(&thumbnail) {
            samples.push(bytes);
        }
    }
    Ok(crate::model::FolderSample { count, samples })
}

// ---------------------------------------------------------------------------
// 走査（scan）
// ---------------------------------------------------------------------------

struct RawEntry {
    relative_path: String,
    absolute_path: PathBuf,
    size: u64,
    mtime_ms: i64,
}

fn file_mtime_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 直下だけでなく、下位のフォルダも常にたどる。**ドット始まりのフォルダは種類を問わず
/// 全部除外する**（`.photo-curator` だけを名指しで除いていたため、NAS の WebAccess
/// 機能が自動で作る `.webaxs\thumbnail`（縮小画像キャッシュ）を原本と一緒に数えて
/// しまい、「画質違いの同じ構図の写真が大量に出る」不具合になっていた。
/// `L:\名古屋ひとり旅` で実測: 235枚のうち150枚が `.webaxs\thumbnail` のキャッシュ、
/// 実写真は約85枚）。
/// Web（フォルダ）版の `prepare` と同じ挙動（作成時の「まとめて」判断はここでは見ない）。
fn walk_all_files(root: &Path) -> Vec<RawEntry> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        let Some(relative_path) = store::to_relative_path(root, entry.path()) else { continue };
        out.push(RawEntry {
            relative_path,
            absolute_path: entry.path().to_path_buf(),
            size: metadata.len(),
            mtime_ms: file_mtime_ms(&metadata),
        });
    }
    out
}

pub(crate) fn emit_progress(app: &AppHandle, project_id: &str, progress: PrepareProgress) {
    let _ = app.emit(&format!("prepare-progress:{project_id}"), progress);
}

/// scan → meta → display を通しで行う。
pub fn prepare(app: &AppHandle, project: &mut Project, cancel: &AtomicBool) -> Result<(), String> {
    if project.source.kind == crate::model::SourceKind::Amazon {
        return crate::amazon::prepare(app, project, cancel);
    }
    let project_id = project.id.clone();
    let root = PathBuf::from(&project.source.key);
    if !root.is_dir() {
        return Err(format!("フォルダが見つかりません: {}", root.display()));
    }

    // ---- scan: ファイルを集め、中身で写真だけに絞る ----
    let raw = walk_all_files(&root);
    let existing = store::load_photos(app, &project_id)?;
    let mut by_path: HashMap<String, ProjectPhoto> =
        existing.into_iter().map(|p| (p.relative_path.clone(), p)).collect();

    let mut photos: Vec<ProjectPhoto> = Vec::with_capacity(raw.len());
    let mut scanned = 0usize;
    for entry in &raw {
        if cancel.load(Ordering::Relaxed) {
            return finalize_partial(app, project, &photos, "中断しました");
        }
        let Some(head) = read_head(&entry.absolute_path, SNIFF_PROBE) else { continue };
        if !format::is_image(format::sniff(&head)) {
            continue; // 動画・非対応形式は数えない
        }
        let prior = by_path.remove(&entry.relative_path);
        let unchanged = prior
            .as_ref()
            .map(|p| p.size == entry.size && p.mtime_ms == entry.mtime_ms)
            .unwrap_or(false);
        photos.push(if unchanged {
            prior.unwrap()
        } else {
            ProjectPhoto {
                relative_path: entry.relative_path.clone(),
                captured_at: None,
                d_hash: None,
                d_hash_version: 0,
                size: entry.size,
                mtime_ms: entry.mtime_ms,
                has_thumbnail: false,
                has_display: false,
            }
        });
        scanned += 1;
        if scanned % CHECKPOINT_EVERY == 0 {
            emit_progress(
                app,
                &project_id,
                PrepareProgress { task: PrepareTask::Scan, done: scanned, total: raw.len(), warning: None },
            );
        }
    }
    photos.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    store::save_photos(app, &project_id, &photos)?;
    project.photo_count = photos.len();
    project.scanned_count = photos.len();
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    emit_progress(
        app,
        &project_id,
        PrepareProgress { task: PrepareTask::Scan, done: photos.len(), total: photos.len(), warning: None },
    );
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- meta: 撮影時刻・指紋・サムネイル ----
    let thumbnail_dir = store::thumbnail_dir(app, &project_id)?;
    let needs_meta: Vec<usize> = photos
        .iter()
        .enumerate()
        .filter(|(_, p)| p.captured_at.is_none() || p.d_hash.is_none())
        .map(|(i, _)| i)
        .collect();
    let meta_total = needs_meta.len();
    let mut meta_done = 0usize;
    let mut meta_failed = 0usize;
    let workers = worker_count();

    struct MetaResult {
        captured_at: Option<i64>,
        d_hash: Option<String>,
        has_thumbnail: bool,
        failed: bool,
    }

    let meta_targets: Vec<(usize, PathBuf, String, i64)> = needs_meta
        .iter()
        .map(|&i| (i, root.join(rel_to_os(&photos[i].relative_path)), photos[i].relative_path.clone(), photos[i].mtime_ms))
        .collect();

    let mut since_checkpoint = 0usize;
    run_parallel(
        &meta_targets,
        workers,
        cancel,
        |(_, path, relative_path, mtime_ms)| -> MetaResult {
            let Some(head) = read_head(path, EXIF_HEAD_PROBE) else {
                return MetaResult { captured_at: None, d_hash: None, has_thumbnail: false, failed: true };
            };
            let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
            let captured_at = exif_util::read_capture_time(&head, file_name, *mtime_ms).at;
            let Some(image) = image_pipeline::decode_hash_source(path, &head) else {
                return MetaResult { captured_at: Some(captured_at), d_hash: None, has_thumbnail: false, failed: true };
            };
            let d_hash = image_pipeline::d_hash_of(&image);
            let thumbnail = image_pipeline::encode_thumbnail(&image_pipeline::scale_for_thumbnail(&image));
            let has_thumbnail = if let Some(bytes) = &thumbnail {
                let file = thumbnail_dir.join(store::cache_file_name(relative_path));
                store::write_atomically(&file, bytes).is_ok()
            } else {
                false
            };
            MetaResult { captured_at: Some(captured_at), d_hash, has_thumbnail, failed: thumbnail.is_none() }
        },
        |result_index, result| {
            let photo_index = meta_targets[result_index].0;
            let photo = &mut photos[photo_index];
            photo.captured_at = result.captured_at;
            photo.d_hash = result.d_hash;
            photo.d_hash_version = 2;
            photo.has_thumbnail = result.has_thumbnail;
            if result.failed {
                meta_failed += 1;
            }
            meta_done += 1;
            since_checkpoint += 1;
            if since_checkpoint >= CHECKPOINT_EVERY {
                since_checkpoint = 0;
                let _ = store::save_photos(app, &project_id, &photos);
            }
            emit_progress(
                app,
                &project_id,
                PrepareProgress {
                    task: PrepareTask::Meta,
                    done: meta_done,
                    total: meta_total,
                    warning: if meta_failed > 0 { Some(format!("{meta_failed} 枚を読めませんでした")) } else { None },
                },
            );
        },
    );
    store::save_photos(app, &project_id, &photos)?;
    let meta_hashed_count = photos.iter().filter(|p| p.captured_at.is_some() || p.d_hash.is_some()).count();
    project.meta_hashed_count = meta_hashed_count;
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- display: 選別画面に出す表示用画像 ----
    let settings = store::load_settings(app)?;
    let edge = image_pipeline::normalize_display_edge(settings.display_edge);
    let display_dir = store::display_dir(app, &project_id)?;
    let needs_display: Vec<(usize, PathBuf, String)> = photos
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.has_display)
        .map(|(i, p)| (i, root.join(rel_to_os(&p.relative_path)), p.relative_path.clone()))
        .collect();
    let display_total = needs_display.len();
    let mut display_done = 0usize;
    let mut display_failed = 0usize;
    let mut since_checkpoint = 0usize;

    run_parallel(
        &needs_display,
        workers,
        cancel,
        |(_, path, relative_path)| -> bool {
            let Some(head) = read_head(path, EXIF_HEAD_PROBE) else { return false };
            let Some(image) = image_pipeline::decode_full(path, &head) else { return false };
            let Some(bytes) = image_pipeline::encode_display(&image, edge) else { return false };
            let file = display_dir.join(store::cache_file_name(relative_path));
            store::write_atomically(&file, &bytes).is_ok()
        },
        |result_index, ok| {
            let photo_index = needs_display[result_index].0;
            photos[photo_index].has_display = ok;
            if !ok {
                display_failed += 1;
            }
            display_done += 1;
            since_checkpoint += 1;
            if since_checkpoint >= CHECKPOINT_EVERY {
                since_checkpoint = 0;
                let _ = store::save_photos(app, &project_id, &photos);
            }
            emit_progress(
                app,
                &project_id,
                PrepareProgress {
                    task: PrepareTask::Display,
                    done: display_done,
                    total: display_total,
                    warning: if display_failed > 0 { Some(format!("{display_failed} 枚を読めませんでした")) } else { None },
                },
            );
        },
    );
    store::save_photos(app, &project_id, &photos)?;
    project.displayed_count = photos.iter().filter(|p| p.has_display).count();
    let total_failed = meta_failed + display_failed;
    project.prepare_warning = if total_failed > 0 { Some(format!("{total_failed} 枚を読めませんでした")) } else { None };
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    Ok(())
}

fn finalize_partial(app: &AppHandle, project: &mut Project, photos: &[ProjectPhoto], warning: &str) -> Result<(), String> {
    store::save_photos(app, &project.id, photos)?;
    project.photo_count = photos.len();
    project.scanned_count = photos.len();
    project.prepare_warning = Some(warning.to_string());
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    Ok(())
}

/// 相対パス（`/` 区切り）を OS のパス区切りに戻す。
fn rel_to_os(relative_path: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for part in relative_path.split('/') {
        path.push(part);
    }
    path
}
