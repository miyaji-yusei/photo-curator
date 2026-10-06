//! 書き出し（星ごとのフォルダへのコピー・移動、Amazon の書き出し、写真本体と組の RAW への星の書き込み）。
//! U54 で lib.rs から移した（中身は変えていない）。コマンドは lib.rs に残している。

use super::*;

// ---------------------------------------------------------------------------
// 書き出し（原本に触れる唯一の領域）
// ---------------------------------------------------------------------------

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportReport {
    pub(crate) processed: usize,
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
    /// 組の RAW の `.xmp` に書けた数（U47。`processed` には含めない。失敗は `failed`）。
    pub(crate) paired_raw_processed: usize,
    /// 失敗と、その理由。全部は返さず先頭だけ。
    pub(crate) errors: Vec<String>,
}

impl ExportReport {
    pub(crate) fn fail(&mut self, path: &str, reason: impl std::fmt::Display) {
        self.failed += 1;
        if self.errors.len() < 20 {
            self.errors.push(format!("{path}: {reason}"));
        }
    }
}

/// 対象の写真を取り出す。**対象は TS が決めた写真の id**（連写の仲間まで広げたあと）で、
/// 星では選ばない。id が空なら何も返さない。並びは相対パス順。
pub(crate) fn photos_for_export(
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
pub(crate) fn mark_photos_missing(conn: &Connection, project_id: &str, ids: &[String]) -> Result<(), String> {
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
pub(crate) fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
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
pub(crate) fn export_photos_blocking(
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
pub(crate) fn export_amazon_copy(
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
/// 対象の写真（id・パス・星）に星を書く。JPEG は中の XMP に、
/// `pair_raw` がオンなら組の RAW の隣の `.xmp` にも。JPEG の書き込みが失敗したら RAW 側は書かない。
pub(crate) fn write_ratings_to_targets(targets: &[(String, String, i64)], pair_raw: bool) -> ExportReport {
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

pub(crate) fn write_ratings_to_photos_blocking(
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
