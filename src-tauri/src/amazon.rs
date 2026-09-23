//! Amazon Photos 共有リンク（設計 08 章・07 章 段7）。
//!
//! **ログイン・Cookie は使わない。読み取り専用。** 共有ページ自身が読んでいる
//! 非公式 API を、PC は Rust から直接呼ぶ（ブラウザの CORS 制限を受けない。
//! 2026-09-24 の実測で、JSON の API はブラウザからも読めたが、画像本体の
//! `fetch()` は CORS で失敗することを確認した。Web 版がこの出所に対応しない
//! 理由もそこにある）。

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::exif_util::civil_timestamp_ms;

/// FILE 以外は 2 階層まで潜る（08章「落とし穴」3）。
const MAX_DEPTH: u32 = 2;

#[derive(Debug, Clone)]
pub struct AmazonSource {
    pub host: String,
    pub share_id: String,
}

/// リンクから host・shareId を取り出す。
/// `https://www.amazon.co.jp/photos/share/{id}` `.../clouddrive/share/{id}` の両方を受ける。
pub fn parse_share_url(url: &str) -> Option<AmazonSource> {
    let trimmed = url.trim();
    let after_scheme = trimmed.split("://").nth(1).unwrap_or(trimmed);
    let mut parts = after_scheme.splitn(2, '/');
    let host = parts.next()?.to_string();
    if !host.starts_with("www.amazon.") {
        return None;
    }
    let rest = parts.next().unwrap_or("");
    for prefix in ["photos/share/", "clouddrive/share/"] {
        if let Some(after) = rest.strip_prefix(prefix) {
            let share_id: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !share_id.is_empty() {
                return Some(AmazonSource { host, share_id });
            }
        }
    }
    None
}

/// `ProjectSource.key` の形（08章5.1: `"{host}|{shareId}"`）。
pub fn key_for(source: &AmazonSource) -> String {
    format!("{}|{}", source.host, source.share_id)
}

pub fn parse_key(key: &str) -> Option<AmazonSource> {
    let (host, share_id) = key.split_once('|')?;
    Some(AmazonSource { host: host.to_string(), share_id: share_id.to_string() })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
}

fn describe_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(404, _) => "このリンクは削除されたか、無効です。".to_string(),
        ureq::Error::Status(code, _) => format!("Amazon から読めませんでした（{code}）。"),
        ureq::Error::Transport(_) => "ネットワークにつながっていません。".to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct ShareInfo {
    #[serde(rename = "nodeInfo")]
    node_info: NodeInfo,
}
#[derive(Debug, Deserialize)]
struct NodeInfo {
    id: String,
    name: String,
}

pub struct ShareRoot {
    pub node_id: String,
    pub name: String,
}

/// ① 共有の中身。
pub fn fetch_share_root(source: &AmazonSource) -> Result<ShareRoot, String> {
    let url = format!(
        "https://{}/drive/v1/shares/{}?shareId={}&resourceVersion=V2&ContentType=JSON",
        source.host, source.share_id, source.share_id
    );
    let resp = agent().get(&url).call().map_err(describe_error)?;
    let info: ShareInfo = resp.into_json().map_err(|e| e.to_string())?;
    Ok(ShareRoot { node_id: info.node_info.id, name: info.node_info.name })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "contentProperties")]
    pub content_properties: Option<ContentProperties>,
    #[serde(rename = "tempLink")]
    pub temp_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentProperties {
    #[serde(rename = "contentType")]
    pub content_type: Option<String>,
    #[serde(rename = "contentDate")]
    pub content_date: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChildrenPage {
    count: usize,
    data: Vec<AmazonNode>,
}

/// ② 子の一覧。200 件ずつ、offset で送る（nextToken は来ない。08章2.2）。
fn fetch_children_page(source: &AmazonSource, node_id: &str, offset: usize) -> Result<ChildrenPage, String> {
    let url = format!(
        "https://{}/drive/v1/nodes/{}/children?asset=ALL&limit=200&offset={}&searchOnFamily=false&tempLink=true&shareId={}&sort=%5B%27contentProperties.contentDate+ASC%27%5D&resourceVersion=V2&ContentType=JSON",
        source.host, node_id, offset, source.share_id
    );
    let resp = agent().get(&url).call().map_err(describe_error)?;
    resp.into_json().map_err(|e| e.to_string())
}

fn fetch_all_children(source: &AmazonSource, node_id: &str) -> Result<Vec<AmazonNode>, String> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let page = fetch_children_page(source, node_id, offset)?;
        let got = page.data.len();
        out.extend(page.data);
        offset += got;
        if got == 0 || offset >= page.count {
            break;
        }
    }
    Ok(out)
}

fn is_image_node(node: &AmazonNode) -> bool {
    node.content_properties
        .as_ref()
        .and_then(|p| p.content_type.as_deref())
        .map(|t| t.starts_with("image/"))
        .unwrap_or(false)
}

/// FILE 以外は 2 階層まで潜り、`image/*` の FILE だけ集める
/// （**拡張子で判定しない**。08章「落とし穴」1・2）。
pub fn list_photos(source: &AmazonSource, root_node_id: &str) -> Result<Vec<AmazonNode>, String> {
    let mut photos = Vec::new();
    collect(source, root_node_id, 0, &mut photos)?;
    Ok(photos)
}

fn collect(source: &AmazonSource, node_id: &str, depth: u32, out: &mut Vec<AmazonNode>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let children = fetch_all_children(source, node_id)?;
    for child in children {
        if child.kind == "FILE" {
            if is_image_node(&child) {
                out.push(child);
            }
        } else {
            collect(source, &child.id, depth + 1, out)?;
        }
    }
    Ok(())
}

/// ③ 画像バイト列。`view_box` を指定するとその長辺まで縮小して来る（無指定は原本）。
pub fn fetch_image_bytes(temp_link: &str, view_box: Option<u32>) -> Result<Vec<u8>, String> {
    let url = match view_box {
        Some(edge) => format!("{temp_link}?viewBox={edge},{edge}"),
        None => temp_link.to_string(),
    };
    let resp = agent().get(&url).call().map_err(describe_error)?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(64 * 1024 * 1024) // 原本でも 64MB を超えることはない想定。念のため上限を置く。
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// 撮影時刻（ミリ秒）。**末尾の `Z` を信じない。** Amazon は EXIF のその土地の
/// 時計にそのまま `Z` を付けて返すので、offset を無視してそのまま読む
/// （08章「落とし穴」6。09-12 に Android で見つかった 9 時間ずれの教訓と同じ）。
pub fn parse_content_date(content_date: &str) -> Option<i64> {
    let cleaned = content_date.trim_end_matches('Z');
    let (date_part, time_part) = cleaned.split_once('T')?;
    let date: Vec<i64> = date_part.splitn(3, '-').filter_map(|s| s.parse().ok()).collect();
    let time_main = time_part.split('.').next().unwrap_or(time_part);
    let time: Vec<i64> = time_main.splitn(3, ':').filter_map(|s| s.parse().ok()).collect();
    if date.len() != 3 || time.len() != 3 {
        return None;
    }
    civil_timestamp_ms(date[0], date[1], date[2], time[0], time[1], time[2])
}

// ---------------------------------------------------------------------------
// 作成画面の見本（08章8.1）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonPreview {
    /// `ProjectSource.key`（`"{host}|{shareId}"`）。作成するときそのまま使う。
    pub key: String,
    pub name: String,
    pub count: usize,
    /// 先頭12枚ぶんの見本（`viewBox=160` を JPEG のまま）。原本は読まない。
    pub samples: Vec<Vec<u8>>,
}

/// 共有リンクを読み、名前・枚数・見本12枚を返す（作成画面。08章「プロジェクト名の決め方」）。
pub fn preview(share_url: &str, sample_limit: usize) -> Result<AmazonPreview, String> {
    let source = parse_share_url(share_url).ok_or_else(|| "Amazon Photos の共有リンクの形ではありません。".to_string())?;
    let root = fetch_share_root(&source)?;
    let nodes = list_photos(&source, &root.node_id)?;

    // 共有の直下がアルバム1つだけなら、そのアルバムの名前を使う（08章「プロジェクト名の決め方」）。
    let top_children = fetch_all_children(&source, &root.node_id)?;
    let albums: Vec<&AmazonNode> = top_children.iter().filter(|n| n.kind != "FILE").collect();
    let name = if albums.len() == 1 { albums[0].name.clone() } else { root.name.clone() };

    let mut samples = Vec::new();
    for node in nodes.iter().take(sample_limit) {
        let Some(temp_link) = &node.temp_link else { continue };
        if let Ok(bytes) = fetch_image_bytes(temp_link, Some(160)) {
            samples.push(bytes);
        }
    }
    Ok(AmazonPreview { key: key_for(&source), name, count: nodes.len(), samples })
}

// ---------------------------------------------------------------------------
// 準備（scan → meta → display。既存の3段にそのまま乗せる。08章7）
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

use crate::image_pipeline;
use crate::model::{PrepareProgress, PrepareTask, Project, ProjectPhoto};
use crate::scan::{emit_progress, run_parallel, worker_count, CHECKPOINT_EVERY};
use crate::store;

pub fn prepare(app: &AppHandle, project: &mut Project, cancel: &AtomicBool) -> Result<(), String> {
    let project_id = project.id.clone();
    let source = parse_key(&project.source.key).ok_or("出所の形が正しくありません。")?;
    let root = fetch_share_root(&source)?;
    let nodes = list_photos(&source, &root.node_id)?;
    let node_by_id: HashMap<String, AmazonNode> = nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();

    // ---- scan: 一覧そのものが走査（08章7「写真の走査」） ----
    let existing = store::load_photos(app, &project_id)?;
    let mut by_path: HashMap<String, ProjectPhoto> =
        existing.into_iter().map(|p| (p.relative_path.clone(), p)).collect();
    let mut photos: Vec<ProjectPhoto> = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let relative_path = node.id.clone(); // 08章5.2: relativePath は nodeId（星・連写の鍵）
        let size = node.content_properties.as_ref().and_then(|p| p.size).unwrap_or(0);
        let prior = by_path.remove(&relative_path);
        let unchanged = prior.as_ref().map(|p| p.size == size).unwrap_or(false);
        photos.push(if unchanged {
            prior.unwrap()
        } else {
            ProjectPhoto {
                relative_path,
                captured_at: None,
                d_hash: None,
                d_hash_version: 0,
                size,
                mtime_ms: 0,
                has_thumbnail: false,
                has_display: false,
            }
        });
    }
    photos.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    store::save_photos(app, &project_id, &photos)?;
    project.photo_count = photos.len();
    project.scanned_count = photos.len();
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    emit_progress(app, &project_id, PrepareProgress { task: PrepareTask::Scan, done: photos.len(), total: photos.len(), warning: None });
    // tempLink を控えておく。**拡大のたびに一覧をたどり直さない**ための鍵（08章2.5「鍵は node id」）。
    let _ = save_temp_links(app, &project_id, &nodes);
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- meta: 撮影時刻・指紋・サムネイル（viewBox=160 から。08章7「それで指紋を作る」） ----
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
    let meta_targets: Vec<(usize, String)> = needs_meta.iter().map(|&i| (i, photos[i].relative_path.clone())).collect();
    let mut since_checkpoint = 0usize;
    run_parallel(
        &meta_targets,
        workers,
        cancel,
        |(_, node_id)| -> MetaResult {
            let Some(node) = node_by_id.get(node_id) else {
                return MetaResult { captured_at: None, d_hash: None, has_thumbnail: false, failed: true };
            };
            let captured_at = node
                .content_properties
                .as_ref()
                .and_then(|p| p.content_date.as_deref())
                .and_then(parse_content_date);
            let Some(temp_link) = &node.temp_link else {
                return MetaResult { captured_at, d_hash: None, has_thumbnail: false, failed: true };
            };
            let Ok(bytes) = fetch_image_bytes(temp_link, Some(160)) else {
                return MetaResult { captured_at, d_hash: None, has_thumbnail: false, failed: true };
            };
            let Some(image) = image_pipeline::decode_bytes(&bytes) else {
                return MetaResult { captured_at, d_hash: None, has_thumbnail: false, failed: true };
            };
            let d_hash = image_pipeline::d_hash_of(&image);
            let thumbnail = image_pipeline::encode_thumbnail(&image_pipeline::scale_for_thumbnail(&image));
            let has_thumbnail = if let Some(bytes) = &thumbnail {
                let file = thumbnail_dir.join(store::cache_file_name(node_id));
                store::write_atomically(&file, bytes).is_ok()
            } else {
                false
            };
            MetaResult { captured_at, d_hash, has_thumbnail, failed: thumbnail.is_none() }
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
    project.meta_hashed_count = photos.iter().filter(|p| p.captured_at.is_some() || p.d_hash.is_some()).count();
    project.updated_at = crate::model::now_ms();
    store::upsert_project(app, project.clone())?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- display: 選別画面に出す表示用画像（08章7「最初に上限を測り」は簡略化し、
    // 既定の display_edge をそのまま使う。原本を超える大きさは Amazon 側が
    // 自動で原本の大きさに丸めて返す＝壊れない） ----
    let settings = store::load_settings(app)?;
    let edge = image_pipeline::normalize_display_edge(settings.display_edge);
    let display_dir = store::display_dir(app, &project_id)?;
    let needs_display: Vec<(usize, String)> = photos
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.has_display)
        .map(|(i, p)| (i, p.relative_path.clone()))
        .collect();
    let display_total = needs_display.len();
    let mut display_done = 0usize;
    let mut display_failed = 0usize;
    let mut since_checkpoint = 0usize;

    run_parallel(
        &needs_display,
        workers,
        cancel,
        |(_, node_id)| -> bool {
            let Some(node) = node_by_id.get(node_id) else { return false };
            let Some(temp_link) = &node.temp_link else { return false };
            let Ok(bytes) = fetch_image_bytes(temp_link, Some(edge)) else { return false };
            let Some(image) = image_pipeline::decode_bytes(&bytes) else { return false };
            let Some(out) = image_pipeline::encode_display(&image, edge) else { return false };
            let file = display_dir.join(store::cache_file_name(node_id));
            store::write_atomically(&file, &out).is_ok()
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

fn temp_links_path(app: &AppHandle, project_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(store::project_dir(app, project_id)?.join("amazon_temp_links.json"))
}

/// tempLink を控える。**鍵は node id**（08章2.5・2026-09-24「落とし穴」4 と同じ理由）。
fn save_temp_links(app: &AppHandle, project_id: &str, nodes: &[AmazonNode]) -> Result<(), String> {
    let map: HashMap<&str, &str> =
        nodes.iter().filter_map(|n| n.temp_link.as_deref().map(|t| (n.id.as_str(), t))).collect();
    let json = serde_json::to_vec_pretty(&map).map_err(|e| e.to_string())?;
    store::write_atomically(&temp_links_path(app, project_id)?, &json)
}

fn load_temp_links(app: &AppHandle, project_id: &str) -> HashMap<String, String> {
    let Ok(path) = temp_links_path(app, project_id) else { return HashMap::new() };
    let Ok(text) = std::fs::read_to_string(&path) else { return HashMap::new() };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 拡大・「ギャラリーに保存」相当。**そのたびに原本を取ってきて出す。端末には置かない**
/// （08章1・6章「絵の3段」）。控えた tempLink をまず使い、使えなければ一覧を読み直して
/// 1回だけ取り直す（08章2.4）。
pub fn fetch_original(app: &AppHandle, project: &Project, relative_path: &str) -> Result<Vec<u8>, String> {
    let cached = load_temp_links(app, &project.id);
    if let Some(temp_link) = cached.get(relative_path) {
        if let Ok(bytes) = fetch_image_bytes(temp_link, None) {
            return Ok(bytes);
        }
    }
    let source = parse_key(&project.source.key).ok_or("出所の形が正しくありません。")?;
    let root = fetch_share_root(&source)?;
    let nodes = list_photos(&source, &root.node_id)?;
    let _ = save_temp_links(app, &project.id, &nodes);
    let node = nodes
        .iter()
        .find(|n| n.id == relative_path)
        .ok_or_else(|| "この写真は見つかりませんでした。".to_string())?;
    let temp_link = node.temp_link.as_ref().ok_or_else(|| "この写真は見つかりませんでした。".to_string())?;
    fetch_image_bytes(temp_link, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn リンクの形からhostとshareIdを取り出す() {
        let source = parse_share_url("https://www.amazon.co.jp/photos/share/abcDEF-12_3").unwrap();
        assert_eq!(source.host, "www.amazon.co.jp");
        assert_eq!(source.share_id, "abcDEF-12_3");
    }

    #[test]
    fn clouddrive形式のリンクも読める() {
        let source = parse_share_url("https://www.amazon.com/clouddrive/share/xyz789").unwrap();
        assert_eq!(source.host, "www.amazon.com");
        assert_eq!(source.share_id, "xyz789");
    }

    #[test]
    fn amazonでないリンクは拒否する() {
        assert!(parse_share_url("https://example.com/photos/share/abc").is_none());
    }

    #[test]
    fn keyの行き来ができる() {
        let source = AmazonSource { host: "www.amazon.co.jp".into(), share_id: "abc".into() };
        let key = key_for(&source);
        assert_eq!(key, "www.amazon.co.jp|abc");
        let back = parse_key(&key).unwrap();
        assert_eq!(back.host, source.host);
        assert_eq!(back.share_id, source.share_id);
    }

    #[test]
    fn 撮影時刻はzを無視してそのままの数字で読む() {
        let ms = parse_content_date("2021-07-23T13:13:29.000Z").unwrap();
        // civil_timestamp_ms(2021,7,23,13,13,29) と同じ値になるはず（offset を掛けない）。
        assert_eq!(ms, civil_timestamp_ms(2021, 7, 23, 13, 13, 29).unwrap());
    }
}
