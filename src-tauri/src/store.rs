//! アプリのデータフォルダへの保存（設計 07 章 段4-1）。
//!
//! 形は Android の `filesDir` と同じ発想: プロジェクトごとに JSON とファイルを
//! 素朴に置くだけ。SQLite は使わない。**旧版（SQLite）のデータは読まない。**
//!
//! 書き込みはすべて「隣に `.writing` を書いてから rename」で行う
//! （途中で落ちても壊れたファイルが残らない。Android の `writeAtomically` と同じ）。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use tauri::{AppHandle, Manager};

use crate::model::{AppSettings, Project, ProjectPhoto, ProjectSource, SidecarState};
use photo_curator_core::{PairOverride, Session};

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("データフォルダの場所が分かりません: {error}"))?;
    fs::create_dir_all(&dir).map_err(|error| format!("データフォルダを作れません: {error}"))?;
    Ok(dir)
}

pub fn project_dir(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("projects").join(project_id);
    fs::create_dir_all(&dir).map_err(|error| format!("プロジェクトフォルダを作れません: {error}"))?;
    Ok(dir)
}

pub fn thumbnail_dir(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let dir = project_dir(app, project_id)?.join("thumbnails");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

pub fn display_dir(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let dir = project_dir(app, project_id)?.join("display");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// 隣に `.writing` を書いてから rename する。**途中まで書いて落ちても、
/// 元のファイルか完全な新しいファイルのどちらかしか残らない。**
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.writing"),
        None => "writing".to_string(),
    });
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // 別ボリュームだと rename が原子的でなくなることがある。コピーで代える。
            let result = fs::copy(&temp, path).map(|_| ()).map_err(|error| error.to_string());
            let _ = fs::remove_file(&temp);
            result
        }
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("{}: 読み込めません（{error}）", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_atomically(path, &text)
}

// ---------------------------------------------------------------------------
// プロジェクト一覧・設定
// ---------------------------------------------------------------------------

fn projects_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("projects.json"))
}

pub fn load_projects(app: &AppHandle) -> Result<Vec<Project>, String> {
    Ok(read_json(&projects_path(app)?)?.unwrap_or_default())
}

pub fn save_projects(app: &AppHandle, projects: &[Project]) -> Result<(), String> {
    write_json(&projects_path(app)?, &projects)
}

pub fn upsert_project(app: &AppHandle, project: Project) -> Result<(), String> {
    let mut projects = load_projects(app)?;
    match projects.iter_mut().find(|p| p.id == project.id) {
        Some(slot) => *slot = project,
        None => projects.push(project),
    }
    save_projects(app, &projects)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, String> {
    Ok(read_json(&settings_path(app)?)?.unwrap_or_default())
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    write_json(&settings_path(app)?, settings)
}

fn recent_folders_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("recent_folders.json"))
}

pub fn load_recent_folders(app: &AppHandle) -> Result<Vec<ProjectSource>, String> {
    Ok(read_json(&recent_folders_path(app)?)?.unwrap_or_default())
}

/// 直近 10 件。同じキーは先頭へまとめる。
pub fn remember_recent_folder(app: &AppHandle, source: &ProjectSource) -> Result<(), String> {
    let mut recents = load_recent_folders(app)?;
    recents.retain(|s| s.key != source.key);
    recents.insert(0, source.clone());
    recents.truncate(10);
    write_json(&recent_folders_path(app)?, &recents)
}

// ---------------------------------------------------------------------------
// 端末の身元（サイドカーの updatedBy）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DeviceInfo {
    id: String,
    name: String,
}

fn device_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("device.json"))
}

fn device_name() -> String {
    if cfg!(target_os = "windows") {
        "Windows PC".to_string()
    } else if cfg!(target_os = "macos") {
        "Mac".to_string()
    } else {
        "PC".to_string()
    }
}

/// (id, 表示名)。id は初回に作って保存し、以後は同じ値を返す。
pub fn device_identity(app: &AppHandle) -> Result<(String, String), String> {
    let path = device_path(app)?;
    if let Some(info) = read_json::<DeviceInfo>(&path)? {
        return Ok((info.id, info.name));
    }
    let info = DeviceInfo { id: uuid::Uuid::new_v4().to_string(), name: device_name() };
    write_json(&path, &info)?;
    Ok((info.id, info.name))
}

// ---------------------------------------------------------------------------
// プロジェクトごとのデータ
// ---------------------------------------------------------------------------

pub fn load_photos(app: &AppHandle, project_id: &str) -> Result<Vec<ProjectPhoto>, String> {
    Ok(read_json(&project_dir(app, project_id)?.join("photos.json"))?.unwrap_or_default())
}

pub fn save_photos(app: &AppHandle, project_id: &str, photos: &[ProjectPhoto]) -> Result<(), String> {
    write_json(&project_dir(app, project_id)?.join("photos.json"), &photos)
}

pub fn load_session(app: &AppHandle, project_id: &str) -> Result<Option<Session>, String> {
    read_json(&project_dir(app, project_id)?.join("session.json"))
}

pub fn save_session(app: &AppHandle, project_id: &str, session: &Session) -> Result<(), String> {
    write_json(&project_dir(app, project_id)?.join("session.json"), session)
}

pub fn load_overrides(app: &AppHandle, project_id: &str) -> Result<Vec<PairOverride>, String> {
    Ok(read_json(&project_dir(app, project_id)?.join("overrides.json"))?.unwrap_or_default())
}

pub fn save_overrides(app: &AppHandle, project_id: &str, overrides: &[PairOverride]) -> Result<(), String> {
    write_json(&project_dir(app, project_id)?.join("overrides.json"), &overrides)
}

pub fn load_burst_distance(app: &AppHandle, project_id: &str) -> Result<Option<u32>, String> {
    Ok(read_json(&project_dir(app, project_id)?.join("burst_distance.json"))?.flatten())
}

pub fn save_burst_distance(app: &AppHandle, project_id: &str, distance: u32) -> Result<(), String> {
    write_json(&project_dir(app, project_id)?.join("burst_distance.json"), &Some(distance))
}

pub fn load_sidecar_state(app: &AppHandle, project_id: &str) -> Result<SidecarState, String> {
    Ok(read_json(&project_dir(app, project_id)?.join("sidecar_state.json"))?.unwrap_or_default())
}

pub fn save_sidecar_state(app: &AppHandle, project_id: &str, state: &SidecarState) -> Result<(), String> {
    write_json(&project_dir(app, project_id)?.join("sidecar_state.json"), state)
}

pub fn mark_changed(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let mut state = load_sidecar_state(app, project_id)?;
    state.local_changed = true;
    save_sidecar_state(app, project_id, &state)
}

/// プロジェクトが持つ容量の合計（サムネイル・表示用画像・保存データ）。
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

/// プロジェクトの持ち物を全部消す（削除・やり直しの一元化。Android の
/// `ProjectData` と同じ発想）。
pub fn remove_project_dir(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let dir = project_dir(app, project_id)?;
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 相対パス（`/` 区切り、OS に依存しない）をキャッシュのファイル名に変える。
/// 元の名前を残すと非対応文字や長さの問題が出るので、安定したハッシュにする。
pub fn cache_file_name(relative_path: &str) -> String {
    format!("{:016x}.jpg", fnv1a(relative_path.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// `root` からの相対パスを `/` 区切りの文字列にする（Web 版の `joinPath` と揃える）。
pub fn to_relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}
