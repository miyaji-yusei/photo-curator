//! Tauri コマンド。**判断を持たない**（設計 07 章 段4）。
//! `lib/backends/tauri.ts` が呼ぶ名前・形にそのまま合わせる。

use std::fs;
use std::path::PathBuf;

use photo_curator_core::{PairOverride, Session, Sidecar, SidecarSync};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::model::{
    now_ms, AppSettings, CreateProjectInput, ExportReport, FolderEntry, Project,
    ProjectPhoto, ProjectSource, RatedPhoto, SourceKind,
};
use crate::scan::{self, PrepareRegistry};
use crate::sidecar;
use crate::{export, store};

fn find_project(app: &AppHandle, id: &str) -> Result<Project, String> {
    store::load_projects(app)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "プロジェクトが見つかりません。".to_string())
}

// ---- プロジェクト一覧 ----

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    store::load_projects(&app)
}

#[tauri::command]
pub fn get_project(app: AppHandle, id: String) -> Result<Option<Project>, String> {
    Ok(store::load_projects(&app)?.into_iter().find(|p| p.id == id))
}

#[tauri::command]
pub fn create_project(app: AppHandle, input: CreateProjectInput) -> Result<Project, String> {
    let _ = input.flatten; // 07 章の決定: prepare は常に下位フォルダまで歩く（Web 版と同じ）
    if input.source.kind == SourceKind::Folder && !PathBuf::from(&input.source.key).is_dir() {
        return Err("フォルダが見つかりません。".into());
    }
    let now = now_ms();
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name: input.name,
        source: input.source.clone(),
        photo_count: 0,
        created_at: now,
        updated_at: now,
        scanned_count: 0,
        meta_hashed_count: 0,
        displayed_count: 0,
        prepare_warning: None,
        burst_distance: None,
        completed_at: None,
    };
    if input.source.kind == SourceKind::Folder {
        store::remember_recent_folder(&app, &input.source)?;
    }
    store::upsert_project(&app, project.clone())?;
    Ok(project)
}

#[tauri::command]
pub fn rename_project(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let mut project = find_project(&app, &id)?;
    project.name = name;
    project.updated_at = now_ms();
    store::upsert_project(&app, project)
}

#[tauri::command]
pub fn estimate_delete_size(app: AppHandle, id: String) -> Result<u64, String> {
    Ok(store::dir_size(&store::project_dir(&app, &id)?))
}

#[tauri::command]
pub fn delete_project(app: AppHandle, id: String) -> Result<(), String> {
    store::remove_project_dir(&app, &id)?;
    let projects: Vec<Project> = store::load_projects(&app)?.into_iter().filter(|p| p.id != id).collect();
    store::save_projects(&app, &projects)
}

/// 星・履歴・手直し・基準・時間を消す。顔ぶれ・指紋・絵は残す（`lib/backend.ts` の約束）。
#[tauri::command]
pub fn restart_project(app: AppHandle, id: String) -> Result<(), String> {
    let dir = store::project_dir(&app, &id)?;
    let _ = fs::remove_file(dir.join("session.json"));
    let _ = fs::remove_file(dir.join("overrides.json"));
    let _ = fs::remove_file(dir.join("burst_distance.json"));
    let mut project = find_project(&app, &id)?;
    project.completed_at = None;
    project.burst_distance = None;
    project.updated_at = now_ms();
    store::upsert_project(&app, project)?;
    store::mark_changed(&app, &id)
}

// ---- 設定 ----

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    store::load_settings(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    store::save_settings(&app, &settings)
}

// ---- 出所を選ぶ ----

#[tauri::command]
pub fn pick_folder(app: AppHandle) -> Result<Option<ProjectSource>, String> {
    let picked = app.dialog().file().blocking_pick_folder();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked
        .into_path()
        .map_err(|error| format!("フォルダの場所を読めません: {error}"))?;
    let label = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string());
    Ok(Some(ProjectSource { kind: SourceKind::Folder, key: path.display().to_string(), label }))
}

#[tauri::command]
pub fn list_entries(source: ProjectSource, sub_path: String) -> Result<Vec<FolderEntry>, String> {
    scan::list_entries(&source.key, &sub_path)
}

#[tauri::command]
pub fn recent_folders(app: AppHandle) -> Result<Vec<ProjectSource>, String> {
    store::load_recent_folders(&app)
}

// ---- 走査・準備 ----

#[tauri::command]
pub async fn prepare(app: AppHandle, registry: State<'_, PrepareRegistry>, project_id: String) -> Result<(), String> {
    let flag = registry.begin(&project_id)?;
    let mut project = find_project(&app, &project_id)?;
    let worker_app = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || scan::prepare(&worker_app, &mut project, &flag))
        .await
        .map_err(|error| error.to_string());
    registry.finish(&project_id);
    outcome?
}

#[tauri::command]
pub fn cancel_prepare(registry: State<'_, PrepareRegistry>, project_id: String) {
    registry.cancel(&project_id);
}

// ---- 写真 ----

#[tauri::command]
pub fn list_photos(app: AppHandle, project_id: String) -> Result<Vec<ProjectPhoto>, String> {
    store::load_photos(&app, &project_id)
}

#[tauri::command]
pub fn thumbnail_path(app: AppHandle, project_id: String, relative_path: String) -> Result<Option<String>, String> {
    let file = store::thumbnail_dir(&app, &project_id)?.join(store::cache_file_name(&relative_path));
    Ok(file.is_file().then(|| file.display().to_string()))
}

#[tauri::command]
pub fn display_path(app: AppHandle, project_id: String, relative_path: String) -> Result<Option<String>, String> {
    let file = store::display_dir(&app, &project_id)?.join(store::cache_file_name(&relative_path));
    Ok(file.is_file().then(|| file.display().to_string()))
}

#[tauri::command]
pub fn original_path(app: AppHandle, project_id: String, relative_path: String) -> Result<Option<String>, String> {
    let project = find_project(&app, &project_id)?;
    let mut path = PathBuf::from(&project.source.key);
    for part in relative_path.split('/') {
        path.push(part);
    }
    Ok(path.is_file().then(|| path.display().to_string()))
}

// ---- 選別の途中 ----

#[tauri::command]
pub fn load_session(app: AppHandle, project_id: String) -> Result<Option<Session>, String> {
    store::load_session(&app, &project_id)
}

#[tauri::command]
pub fn save_session(app: AppHandle, project_id: String, session: Session) -> Result<(), String> {
    store::save_session(&app, &project_id, &session)?;
    store::mark_changed(&app, &project_id)
}

#[tauri::command]
pub fn load_overrides(app: AppHandle, project_id: String) -> Result<Vec<PairOverride>, String> {
    store::load_overrides(&app, &project_id)
}

#[tauri::command]
pub fn save_overrides(app: AppHandle, project_id: String, overrides: Vec<PairOverride>) -> Result<(), String> {
    store::save_overrides(&app, &project_id, &overrides)?;
    store::mark_changed(&app, &project_id)
}

#[tauri::command]
pub fn load_burst_distance(app: AppHandle, project_id: String) -> Result<Option<u32>, String> {
    store::load_burst_distance(&app, &project_id)
}

#[tauri::command]
pub fn save_burst_distance(app: AppHandle, project_id: String, distance: u32) -> Result<(), String> {
    store::save_burst_distance(&app, &project_id, distance)?;
    let mut project = find_project(&app, &project_id)?;
    project.burst_distance = Some(distance);
    project.updated_at = now_ms();
    store::upsert_project(&app, project)?;
    store::mark_changed(&app, &project_id)
}

#[tauri::command]
pub fn mark_changed(app: AppHandle, project_id: String) -> Result<(), String> {
    store::mark_changed(&app, &project_id)
}

#[tauri::command]
pub fn mark_completed(app: AppHandle, project_id: String) -> Result<(), String> {
    let mut project = find_project(&app, &project_id)?;
    project.completed_at = Some(now_ms());
    project.updated_at = now_ms();
    store::upsert_project(&app, project)
}

// ---- サイドカー ----

#[tauri::command]
pub fn check_sidecar(app: AppHandle, project_id: String) -> Result<SidecarSync, String> {
    sidecar::check(&app, &find_project(&app, &project_id)?)
}

#[tauri::command]
pub fn push_sidecar(app: AppHandle, project_id: String) -> Result<(), String> {
    sidecar::push_if_changed(&app, &find_project(&app, &project_id)?)
}

#[tauri::command]
pub fn adopt_sidecar(app: AppHandle, project_id: String, sidecar: Sidecar) -> Result<(), String> {
    crate::sidecar::adopt(&app, &project_id, sidecar)
}

#[tauri::command]
pub fn keep_mine_sidecar(app: AppHandle, project_id: String, theirs: Sidecar) -> Result<(), String> {
    sidecar::keep_mine(&app, &find_project(&app, &project_id)?, theirs)
}

// ---- 取り出し ----

#[tauri::command]
pub fn export_folders(app: AppHandle, project_id: String, photos: Vec<RatedPhoto>) -> Result<ExportReport, String> {
    Ok(export::export_folders(&find_project(&app, &project_id)?, &photos))
}

#[tauri::command]
pub fn export_xmp(app: AppHandle, project_id: String, photos: Vec<RatedPhoto>) -> Result<ExportReport, String> {
    Ok(export::export_xmp(&find_project(&app, &project_id)?, &photos))
}

#[tauri::command]
pub fn export_csv(_app: AppHandle, _project_id: String, photos: Vec<RatedPhoto>) -> Result<String, String> {
    Ok(export::export_csv(&photos))
}

// ---- 保存量 ----

#[tauri::command]
pub fn storage_usage_bytes(app: AppHandle) -> Result<u64, String> {
    Ok(export::storage_usage_bytes(&app))
}
