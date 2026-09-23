//! Photo Curator（PC・Tauri）。**入出力のコマンドだけを持つ**（設計 07 章 段4-1）。
//!
//! 判断（星の付き方・連写のまとめ方）は `photo_curator_core` に任せる。ここは
//! ファイル・EXIF・XMP の読み書きと、core の型をそのまま JSON で運ぶだけ。
//! 旧版（SQLite・NAS の SMB 接続・Android の JNI 橋）は段4 の作り直しで落とした。
//! **旧版のデータは読まない。**

mod commands;
mod exif_util;
mod export;
mod format;
mod image_pipeline;
mod model;
mod scan;
mod sidecar;
mod store;
mod xmp;

use scan::PrepareRegistry;

pub fn run() {
    tauri::Builder::default()
        .manage(PrepareRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_project,
            commands::create_project,
            commands::rename_project,
            commands::estimate_delete_size,
            commands::delete_project,
            commands::restart_project,
            commands::load_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::list_entries,
            commands::recent_folders,
            commands::sample_folder,
            commands::prepare,
            commands::cancel_prepare,
            commands::list_photos,
            commands::thumbnail_path,
            commands::cover_path,
            commands::display_path,
            commands::original_path,
            commands::load_session,
            commands::save_session,
            commands::load_overrides,
            commands::save_overrides,
            commands::load_burst_distance,
            commands::save_burst_distance,
            commands::mark_changed,
            commands::mark_completed,
            commands::check_sidecar,
            commands::push_sidecar,
            commands::adopt_sidecar,
            commands::keep_mine_sidecar,
            commands::export_folders,
            commands::export_xmp,
            commands::export_csv,
            commands::storage_usage_bytes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Photo Curator");
}
