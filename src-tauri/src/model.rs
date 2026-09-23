//! PC（Tauri）の保存の形。**判断は持たない**（設計 07 章 段4-1）。
//!
//! フィールド名は `lib/backend.ts`・`types/project.ts` の TS 型とキャメルケースで
//! 揃える（`#[serde(rename_all = "camelCase")]`）。判断そのもの（Session・
//! PairOverride・Sidecar 等）は `photo_curator_core` の型をそのまま使う
//! （フィールド名の規則はそちらに合わせる。設計 04 章 note）。

use serde::{Deserialize, Serialize};

pub const SIDECAR_RELATIVE_PATH: &str = ".photo-curator/catalog.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Folder,
    Amazon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSource {
    pub kind: SourceKind,
    /// フォルダなら絶対パス。Amazon は段7まで PC では未対応。
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPhoto {
    pub relative_path: String,
    pub captured_at: Option<i64>,
    pub d_hash: Option<String>,
    pub d_hash_version: i32,
    pub size: u64,
    pub mtime_ms: i64,
    pub has_thumbnail: bool,
    pub has_display: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrepareTask {
    Scan,
    Meta,
    Hash,
    Display,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareProgress {
    pub task: PrepareTask,
    pub done: usize,
    pub total: usize,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source: ProjectSource,
    pub photo_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub scanned_count: usize,
    pub meta_hashed_count: usize,
    pub displayed_count: usize,
    pub prepare_warning: Option<String>,
    pub burst_distance: Option<u32>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub display_edge: u32,
    pub group_size: u32,
    pub group_bursts: bool,
    pub confirm_before_start: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            display_edge: 1024,
            group_size: 4,
            group_bursts: true,
            confirm_before_start: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectInput {
    pub name: String,
    pub source: ProjectSource,
    #[serde(default)]
    pub flatten: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub name: String,
    pub key: String,
    pub has_photos: bool,
    pub has_children: bool,
    pub sample: Option<String>,
}

/// 選んだフォルダの見本（設計 02 章「見本の絵」節）。作成画面の右側に
/// 「枚数・見本12枚」を出すために使う。原本は読まない（EXIF埋め込み
/// サムネイル→縮小デコードだけ。準備の meta 段と同じ手順）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSample {
    /// 中身で写真と判定できたファイルの数（動画・非対応形式は含まない）。
    pub count: usize,
    /// 先頭から `limit` 件ぶんの、JPEG に符号化した見本（各要素がバイト列）。
    pub samples: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RatedPhoto {
    pub relative_path: String,
    pub rating: i32,
    pub captured_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub processed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

impl ExportReport {
    pub fn fail(&mut self, path: &str, error: impl std::fmt::Display) {
        self.failed += 1;
        if self.errors.len() < 20 {
            self.errors.push(format!("{path}: {error}"));
        }
    }
}

/// 端末の控え。サイドカーの書き時の判定に使う（`sidecar_decide` の引数そのもの）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarState {
    pub seen_at: i64,
    pub seen_by: String,
    #[serde(default)]
    pub local_changed: bool,
}

impl Default for SidecarState {
    fn default() -> Self {
        Self { seen_at: 0, seen_by: String::new(), local_changed: false }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
