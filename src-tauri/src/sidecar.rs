//! サイドカー（`.photo-curator/catalog.json`）の読み書き（設計 03 章・07 章 段4）。
//!
//! **判断（4 通りのどれか）は `photo_curator_core::sidecar_decide` に任せる。**
//! ここは「いつ・どこへ・何を」書くかだけを持つ。書けるのは `.photo-curator/catalog.json`
//! だけ（CON-3）。原本には絶対に書かない。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use photo_curator_core::{Sidecar, SidecarPhoto, SidecarSessions, SidecarSync};
use tauri::AppHandle;

use crate::model::{now_ms, Project, SidecarState, SourceKind, SIDECAR_RELATIVE_PATH};
use crate::store;

fn sidecar_path(project: &Project) -> PathBuf {
    Path::new(&project.source.key).join(SIDECAR_RELATIVE_PATH)
}

pub fn supported(project: &Project) -> bool {
    project.source.kind == SourceKind::Folder
}

pub fn check(app: &AppHandle, project: &Project) -> Result<SidecarSync, String> {
    if !supported(project) {
        return Ok(SidecarSync::Settled);
    }
    let remote = fs::read_to_string(sidecar_path(project))
        .ok()
        .and_then(|text| photo_curator_core::sidecar_from_json(text));
    let state = store::load_sidecar_state(app, &project.id)?;
    Ok(photo_curator_core::sidecar_decide(state.seen_at, state.seen_by, state.local_changed, remote))
}

pub fn push_if_changed(app: &AppHandle, project: &Project) -> Result<(), String> {
    if !supported(project) {
        return Ok(());
    }
    let state = store::load_sidecar_state(app, &project.id)?;
    if !state.local_changed {
        return Ok(());
    }
    let session = store::load_session(app, &project.id)?;
    let overrides = store::load_overrides(app, &project.id)?;
    let burst_distance = store::load_burst_distance(app, &project.id)?;
    let (device_id, device_name) = store::device_identity(app)?;

    let mut photos = HashMap::new();
    if let Some(session) = &session {
        for (path, rating) in &session.ratings {
            photos.insert(path.clone(), SidecarPhoto { rating: *rating });
        }
    }
    let sidecar = Sidecar {
        version: 1,
        updated_at: now_ms(),
        updated_by: device_id,
        updated_by_name: device_name,
        photos,
        burst_overrides: overrides,
        sessions: SidecarSessions { tournament: session },
        burst_distance,
    };
    let path = sidecar_path(project);
    let json = photo_curator_core::sidecar_to_json(sidecar.clone());
    store::write_atomically(&path, json.as_bytes())?;
    store::save_sidecar_state(
        app,
        &project.id,
        &SidecarState { seen_at: sidecar.updated_at, seen_by: sidecar.updated_by, local_changed: false },
    )
}

pub fn adopt(app: &AppHandle, project_id: &str, sidecar: Sidecar) -> Result<(), String> {
    if let Some(session) = &sidecar.sessions.tournament {
        store::save_session(app, project_id, session)?;
    }
    store::save_overrides(app, project_id, &sidecar.burst_overrides)?;
    if let Some(distance) = sidecar.burst_distance {
        store::save_burst_distance(app, project_id, distance)?;
    }
    store::save_sidecar_state(
        app,
        project_id,
        &SidecarState { seen_at: sidecar.updated_at, seen_by: sidecar.updated_by, local_changed: false },
    )
}

/// 食い違いで「自分の方を残す」を選んだとき。相手の分は `catalog.<端末>.json` に退避し、
/// 自分の分を改めて書く。
pub fn keep_mine(app: &AppHandle, project: &Project, theirs: Sidecar) -> Result<(), String> {
    let suffix: String = theirs.updated_by.chars().take(12).collect();
    let aside = Path::new(&project.source.key)
        .join(".photo-curator")
        .join(format!("catalog.{suffix}.json"));
    store::write_atomically(&aside, photo_curator_core::sidecar_to_json(theirs).as_bytes())?;
    store::mark_changed(app, &project.id)?;
    push_if_changed(app, project)
}
