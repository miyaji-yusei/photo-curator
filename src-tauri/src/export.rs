//! PC だけの取り出し（設計 07 章 段4-4）: フォルダ分け・XMP・CSV。
//!
//! **フォルダ分けはコピーが既定**（原本は変わらない）。XMP は原本を書き換える
//! （CON-1。呼ぶ前に画面側で赤字の確認を挟む）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::format;
use crate::model::{ExportReport, Project, RatedPhoto};
use crate::store;
use crate::xmp;

fn original_path(project: &Project, relative_path: &str) -> PathBuf {
    let mut path = PathBuf::from(&project.source.key);
    for part in relative_path.split('/') {
        path.push(part);
    }
    path
}

/// 同名衝突を避ける。`foo.jpg` があれば `foo (2).jpg` … と数字を足す。
fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
    let candidate = directory.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 2..1000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = directory.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(file_name)
}

/// 星ごとのフォルダへコピーする。**移動は作らない**（07 章の決定。原本はそのまま残る）。
pub fn export_folders(project: &Project, photos: &[RatedPhoto]) -> ExportReport {
    let mut report = ExportReport::default();
    let root = Path::new(&project.source.key);
    for photo in photos {
        let source = original_path(project, &photo.relative_path);
        if !source.is_file() {
            report.skipped += 1;
            continue;
        }
        let target_dir = root.join(format!("star-{}", photo.rating));
        if let Err(error) = fs::create_dir_all(&target_dir) {
            report.fail(&photo.relative_path, error);
            continue;
        }
        let file_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("photo");
        let destination = unique_destination(&target_dir, file_name);
        match fs::copy(&source, &destination) {
            Ok(_) => report.processed += 1,
            Err(error) => report.fail(&photo.relative_path, error),
        }
    }
    report
}

/// 星を原本の XMP に書く。**原本を書き換える。** JPEG 以外は対象外。
pub fn export_xmp(project: &Project, photos: &[RatedPhoto]) -> ExportReport {
    let mut report = ExportReport::default();
    for photo in photos {
        let source = original_path(project, &photo.relative_path);
        if !source.is_file() {
            report.skipped += 1;
            continue;
        }
        let original = match fs::read(&source) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.fail(&photo.relative_path, error);
                continue;
            }
        };
        if !matches!(format::sniff(&original), format::FileKind::Jpeg) {
            report.skipped += 1;
            continue;
        }
        let updated = match xmp::jpeg_with_rating(&original, photo.rating as i64) {
            Ok(bytes) => bytes,
            Err(reason) => {
                report.fail(&photo.relative_path, reason);
                continue;
            }
        };
        let temporary = source.with_extension("photocurator-tmp");
        if let Err(error) = fs::write(&temporary, &updated) {
            let _ = fs::remove_file(&temporary);
            report.fail(&photo.relative_path, error);
            continue;
        }
        // 置き換える前に、書いたものが画像として開けるか確かめる。
        if let Err(error) = image::ImageReader::open(&temporary)
            .map_err(|e| e.to_string())
            .and_then(|reader| reader.into_dimensions().map_err(|e| e.to_string()))
        {
            let _ = fs::remove_file(&temporary);
            report.fail(&photo.relative_path, format!("検証に失敗したため原本は変更していません: {error}"));
            continue;
        }
        match fs::rename(&temporary, &source) {
            Ok(()) => report.processed += 1,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                report.fail(&photo.relative_path, error);
            }
        }
    }
    report
}

/// `relative_path,rating,captured_at`。Web（フォルダ）版と同じ形（設計 07 章）。
pub fn export_csv(photos: &[RatedPhoto]) -> String {
    let mut rows = vec!["relative_path,rating,captured_at".to_string()];
    for photo in photos {
        let path = serde_json::to_string(&photo.relative_path).unwrap_or_else(|_| "\"\"".into());
        let captured = photo.captured_at.map(|v| v.to_string()).unwrap_or_default();
        rows.push(format!("{path},{},{captured}", photo.rating));
    }
    rows.join("\n")
}

pub fn storage_usage_bytes(app: &tauri::AppHandle) -> u64 {
    store::app_data_dir(app).map(|dir| store::dir_size(&dir)).unwrap_or(0)
}
