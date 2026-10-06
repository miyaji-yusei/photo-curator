//! 走査（目印 fingerprint・拡張子・中身の判定・RAW＋JPEG の組の RAW を除く・フォルダの列挙と、
//! 走査の本体 scan_folder・run_scan・Amazon の run_amazon_scan と写真の行の登録）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

pub(crate) fn fingerprint(path: &Path) -> Option<(i64, i64)> {
    fingerprint_of(&fs::metadata(path).ok()?)
}

/// 「変わったか」の目印の定義: (更新時刻ミリ秒, 大きさ)。走査の列挙で得た情報と
/// `fingerprint(path)` が同じ値になるよう、作り方をここ 1 か所にする。
pub(crate) fn fingerprint_of(metadata: &fs::Metadata) -> Option<(i64, i64)> {
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((mtime, metadata.len() as i64))
}

/// 画像・RAW の名前の拡張子。拡張子は「候補に入れるか」だけに使い、
/// 画像かどうかは中身で決める（`content_is_not_image`）。
pub(crate) const IMAGE_EXTENSIONS: [&str; 13] = [
    "jpg", "jpeg", "png", "webp", "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];
/// 拡張子が動画のものは、中身に関わらず常に除く。
pub(crate) const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "mov", "m4v", "avi", "mts", "m2ts", "3gp"];
/// 中身が JPEG・PNG・WebP でなくても「画像だが今は読めない」に数える拡張子。
pub(crate) const OTHER_IMAGE_EXTENSIONS: [&str; 9] = [
    "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];

pub(crate) fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

/// RAW の拡張子（`IMAGE_EXTENSIONS` のうち HEIC・HEIF 以外。rw2・pef・srw は今は走査の
/// 候補に入らないが、入れる日のために並べておく）。
pub(crate) const RAW_EXTENSIONS: [&str; 10] = [
    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
];

/// RAW＋JPEG 同時撮影の「組」の RAW を除く（U46）。同じフォルダに、拡張子を除いた名前が
/// 大文字小文字を無視して一致する JPEG（.jpg・.jpeg）がある RAW は、写真に数えない。
/// Android は jpg/png/webp だけを走査するので、これで 2 台の顔ぶれが揃う。
/// 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組は除かない。
/// `enabled` はプロジェクトの設定（`pair_raw_jpeg`）。false なら何も除かない。
pub(crate) fn skip_paired_raw<T>(items: Vec<T>, enabled: bool, path_of: impl Fn(&T) -> &Path) -> Vec<T> {
    if !enabled {
        return items;
    }
    let key_of = |path: &Path| -> Option<(PathBuf, String)> {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        Some((path.parent().map(Path::to_path_buf).unwrap_or_default(), stem))
    };
    let jpeg_keys: HashSet<(PathBuf, String)> = items
        .iter()
        .filter_map(|item| {
            let path = path_of(item);
            let ext = extension_lower(path)?;
            if ext == "jpg" || ext == "jpeg" {
                key_of(path)
            } else {
                None
            }
        })
        .collect();
    items
        .into_iter()
        .filter(|item| {
            let path = path_of(item);
            let is_raw = extension_lower(path)
                .map(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
                .unwrap_or(false);
            !(is_raw && key_of(path).is_some_and(|key| jpeg_keys.contains(&key)))
        })
        .collect()
}

/// 拡張子が画像・RAW の名前か、拡張子が無いもの。動画の拡張子は常に除く。
pub(crate) fn is_supported(path: &Path) -> bool {
    match extension_lower(path) {
        Some(ext) => {
            !VIDEO_EXTENSIONS.contains(&ext.as_str()) && IMAGE_EXTENSIONS.contains(&ext.as_str())
        }
        None => true,
    }
}

/// 中身が画像ではない（動画・テキスト・壊れたファイルなど）。先頭バイトで決める。
/// JPEG・PNG・WebP 以外の画像（HEIC・RAW）は先頭バイトでは見分けられないので、
/// 拡張子がその種類なら画像として通し、「読めなかった」に数える（解析で失敗する）。
/// HEIC・CR3 は `ftyp` で始まり、`sniff` は動画と判定するため、この扱いが要る。
pub(crate) fn content_is_not_image(head: &[u8], path: &Path) -> bool {
    let kind = format::sniff(head);
    if format::is_image(kind) {
        return false;
    }
    let other_image = extension_lower(path)
        .map(|ext| OTHER_IMAGE_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false);
    !other_image
}

/// 隠しフォルダ・隠しファイル（名前が `.` で始まる）。NAS の `.webaxs` のような
/// サムネイルのキャッシュを原本として数えないために、走査の入口で除く。
pub(crate) fn is_hidden_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.')
}

/// 列挙で見つかった 1 枚。ハッシュ値ではなく「変わったか」の目印（更新時刻・大きさ）を、
/// 列挙で得た情報から作って持つ。
pub(crate) struct ListedFile {
    pub(crate) path: PathBuf,
    /// `fingerprint(path)` と同じ定義の (更新時刻ミリ秒, 大きさ)。読めなければ None。
    pub(crate) fingerprint: Option<(i64, i64)>,
}

/// フォルダの列挙の結果。`unreadable` は、列挙の途中で読めなかった場所の数
/// （権限・切断など）。1 件でもあれば、列挙は「全部は見えていない」。
pub(crate) struct FolderListing {
    pub(crate) files: Vec<ListedFile>,
    pub(crate) unreadable: usize,
}

/// 走査の入口。フォルダ自体が開けなければ（NAS の切断・共有の取り外し・移動）、
/// 空のフォルダとして通さずエラーにする。
///
/// 更新時刻・大きさは、ディレクトリの列挙で得た情報（Windows では列挙の結果に
/// 入っている）から取る。ファイルごとに `stat` をやり直すと、ネットワークの
/// フォルダでは 1 枚 1 往復になる。
pub(crate) fn list_photo_files(folder: &str, pair_raw: bool) -> Result<FolderListing, String> {
    fs::read_dir(folder)
        .map_err(|error| format!("フォルダに接続できませんでした（{folder}）: {error}"))?;
    let mut files = Vec::new();
    let mut unreadable = 0;
    for entry in WalkDir::new(folder)
        .into_iter()
        .filter_entry(|entry| !is_hidden_entry(entry))
    {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() && is_supported(entry.path()) {
                    let fingerprint = entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| fingerprint_of(&metadata));
                    files.push(ListedFile {
                        path: entry.into_path(),
                        fingerprint,
                    });
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    // RAW＋JPEG 同時撮影の組の RAW は写真に数えない（U46）。
    let files = skip_paired_raw(files, pair_raw, |file| file.path.as_path());
    Ok(FolderListing { files, unreadable })
}

// scan が見つけた1枚を登録・更新する。
// 比較に `=` ではなく `IS` を使うのが要点。SQL では `NULL = NULL` も
// `NULL = 値` も真にならないため、`=` だと fingerprint が NULL の行で
// captured_at と d_hash が再 scan のたびに NULL へ巻き戻されていた。
pub(crate) fn upsert_photo(
    conn: &Connection,
    project_id: &str,
    absolute: &str,
    relative: &str,
    name: &str,
    mtime: Option<i64>,
    size: Option<i64>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
         VALUES (?1,?2,?3,?4,?5,NULL,NULL,0,?6,?7,0)
         ON CONFLICT(project_id,path) DO UPDATE SET
           relative_path=excluded.relative_path, name=excluded.name, is_missing=0,
           captured_at=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.captured_at ELSE NULL END,
           timestamp_source=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.timestamp_source ELSE NULL END,
           d_hash=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash ELSE NULL END,
           d_hash_version=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash_version ELSE NULL END,
           thumbnail_path=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_path ELSE NULL END,
           thumbnail_mtime=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_mtime ELSE NULL END,
           thumbnail_size=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_size ELSE NULL END,
           thumbnail_source=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_source ELSE NULL END,
           thumbnail_version=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_version ELSE NULL END,
           display_path=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_path ELSE NULL END,
           display_edge=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_edge ELSE NULL END,
           analysis_error=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error ELSE NULL END,
           analysis_error_at=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_at ELSE NULL END,
           analysis_error_kind=CASE WHEN photos.fingerprint_mtime IS excluded.fingerprint_mtime AND photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_kind ELSE NULL END,
           fingerprint_mtime=excluded.fingerprint_mtime, fingerprint_size=excluded.fingerprint_size",
        params![Uuid::new_v4().to_string(), project_id, absolute, relative, name, mtime, size],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn project_folder(app: &AppHandle, project_id: &str) -> Result<String, String> {
    connection(app)?
        .query_row(
            "SELECT folder_path FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|_| "Project was not found.".to_string())
}

/// `scan_folder` の終わり方。
pub(crate) enum ScanEnd {
    Completed {
        total: usize,
        /// 走査後に画面へ出る写真の数。
        #[cfg_attr(not(test), allow(dead_code))]
        count: i64,
        /// 列挙で読めなかった場所の数。0 でないとき、欠損の印は付け直していない。
        unreadable: usize,
    },
    Cancelled {
        processed: usize,
        total: usize,
    },
}

/// 走査が 1 つのトランザクションに入れる写真の数。書き込みロックを持つ時間を
/// 短くして、他の書き込み（星の保存・設定）が `database is locked` にならないようにする。
pub(crate) const SCAN_BATCH: usize = 200;

/// フォルダの走査の本体（DB とフォルダだけを触る。アプリの窓には触れない）。
///
/// 写真の登録は `batch_size` 枚ごとの短いトランザクションに分ける（ロックを長く
/// 持たない）。「見つからなかった写真」への欠損の印（`is_missing=1`）は、**最後まで
/// 成功したときだけ**付ける。取り消し・失敗なら印は変わらず、走査前のまま残る
/// （登録済みの分は残る。次の走査で同じ行に上書きされる）。また列挙で読めなかった
/// 場所が 1 件でもあれば、見えていない写真を「無くなった」とは決められないので、
/// 印を付け直さない。
pub(crate) fn scan_folder(
    conn: &Connection,
    project_id: &str,
    folder: &str,
    batch_size: usize,
    is_cancelled: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(&str, usize, usize, &str),
) -> Result<ScanEnd, String> {
    let pair_raw = project_pair_raw(conn, project_id);
    let listing = list_photo_files(folder, pair_raw)?;
    let entries = listing.files;
    let total = entries.len();
    on_progress("indexing", 0, total, "Scanning photo files…");

    conn.execute(
        "UPDATE projects SET status='scanning', updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;

    let mut seen: HashSet<String> = HashSet::with_capacity(total);
    let mut processed = 0;
    let mut next_report = 250;
    for chunk in entries.chunks(batch_size.max(1)) {
        // ループの中はファイルに触れない（DB への書き込みだけ）ので、取り消しは
        // 塊の頭で見れば十分。ここではトランザクションを持っていない。
        if is_cancelled() {
            // 登録済みの塊は残っているので、件数を数え直してから状態を戻す。
            let _ = recount_photos(conn, project_id);
            conn.execute(
                "UPDATE projects SET status='ready', updated_at=?1 WHERE id=?2",
                params![now(), project_id],
            )
            .map_err(|error| error.to_string())?;
            return Ok(ScanEnd::Cancelled { processed, total });
        }
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for file in chunk {
            let path = &file.path;
            let relative = path
                .strip_prefix(folder)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("photo")
                .to_owned();
            let absolute = path.to_string_lossy().to_string();
            // 読めない場合に (0,0) を入れると、読めないファイル同士が「同じ
            // fingerprint」に見えてしまう。NULL のまま持たせる。
            let (mtime, size) = match file.fingerprint {
                Some((mtime, size)) => (Some(mtime), Some(size)),
                None => (None, None),
            };
            upsert_photo(
                &transaction,
                project_id,
                &absolute,
                &relative,
                &name,
                mtime,
                size,
            )?;
            seen.insert(absolute);
        }
        transaction.commit().map_err(|error| error.to_string())?;
        processed += chunk.len();
        if processed >= next_report || processed == total {
            on_progress("indexing", processed, total, "Recording photo locations…");
            next_report = processed + 250;
        }
    }

    if listing.unreadable == 0 {
        // 最後まで成功し、列挙も欠けていなかったときだけ、見つからなかった写真に印を付ける。
        let stale: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT id,path FROM photos WHERE project_id=?1 AND is_missing=0")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            let mut stale = Vec::new();
            for row in rows {
                let (id, path) = row.map_err(|error| error.to_string())?;
                if !seen.contains(&path) {
                    stale.push(id);
                }
            }
            stale
        };
        if !stale.is_empty() {
            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            for id in &stale {
                transaction
                    .execute("UPDATE photos SET is_missing=1 WHERE id=?1", params![id])
                    .map_err(|error| error.to_string())?;
            }
            transaction.commit().map_err(|error| error.to_string())?;
        }
    }
    let count = recount_photos(conn, project_id)?;
    conn.execute(
        "UPDATE projects SET status='ready',updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(ScanEnd::Completed {
        total,
        count,
        unreadable: listing.unreadable,
    })
}

pub(crate) fn run_scan(app: AppHandle, registry: &TaskRegistry, project_id: String) -> Result<(), String> {
    // 出所で分ける。Amazon の共有リンクは、一覧を読むことが走査になる。
    if let Some(source) = amazon_source_of(&connection(&app)?, &project_id)? {
        return run_amazon_scan(app, registry, project_id, source);
    }
    let task_key = format!("scan:{project_id}");
    let folder = project_folder(&app, &project_id)?;
    let conn = connection(&app)?;
    let ended = scan_folder(
        &conn,
        &project_id,
        &folder,
        SCAN_BATCH,
        &|| registry.is_cancelled(&task_key),
        &mut |phase, processed, total, message| {
            progress(&app, &project_id, "scan", phase, processed, total, message)
        },
    );
    let (total, unreadable) = match ended {
        Ok(ScanEnd::Completed {
            total, unreadable, ..
        }) => (total, unreadable),
        Ok(ScanEnd::Cancelled { processed, total }) => {
            progress(
                &app,
                &project_id,
                "scan",
                "cancelled",
                processed,
                total,
                "Scanning was cancelled.",
            );
            return Ok(());
        }
        Err(message) => {
            // 登録済みの塊があるので件数を数え直し、'scanning' のまま残さない（写真があれば ready、なければ new）。
            let _ = recount_photos(&conn, &project_id);
            settle_project_status(&conn, &project_id);
            return Err(message);
        }
    };
    let complete_message = if unreadable > 0 {
        format!(
            "写真の読み込みが完了しました。ただし読めなかったフォルダが {unreadable} か所あったため、見つからなかった写真の整理はしていません。"
        )
    } else {
        "写真の読み込みが完了しました。".to_string()
    };
    progress(
        &app,
        &project_id,
        "scan",
        "complete",
        total,
        total,
        complete_message,
    );
    // ここから先はアイドル時の事前生成。利用者が設定画面を触っている間に
    // 撮影時刻・サムネイル・dHash を少しずつ作っておく。「選別を開始」を
    // 押した時点では、出来ているぶんをそのまま使って即座に始められる。
    let handle = app.clone();
    let background_project = project_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let _ = spawn_analysis(
            handle.clone(),
            &registry,
            background_project,
            AnalysisMode::Background,
        );
    });
    Ok(())
}

/// Amazon の写真の行を登録・更新する。`path` と `relative_path` は node id。
/// 撮影時刻は一覧の `contentDate`（その土地の時計）をそのまま入れる。
/// 大きさ（`fingerprint_size`）が変わった行だけ、解析の結果を捨てる。
pub(crate) fn upsert_amazon_photo(
    conn: &Connection,
    project_id: &str,
    node: &amazon::AmazonNode,
) -> Result<(), String> {
    let size = node
        .content_properties
        .as_ref()
        .and_then(|properties| properties.size)
        .map(|size| size as i64);
    let stem = Path::new(&node.name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let (captured_at, source) = match amazon::captured_at_of(node) {
        Some(at) => (Some(at), TimestampSource::ExifOriginal),
        None => match filename_capture_time_of(stem) {
            Some(at) => (Some(at), TimestampSource::FilenameInferred),
            None => (None, TimestampSource::Unknown),
        },
    };
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,timestamp_source,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
         VALUES (?1,?2,?3,?3,?4,?5,?6,NULL,0,NULL,?7,0)
         ON CONFLICT(project_id,path) DO UPDATE SET
           relative_path=excluded.relative_path, name=excluded.name, is_missing=0,
           captured_at=excluded.captured_at, timestamp_source=excluded.timestamp_source,
           d_hash=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash ELSE NULL END,
           d_hash_version=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.d_hash_version ELSE NULL END,
           thumbnail_path=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_path ELSE NULL END,
           thumbnail_version=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.thumbnail_version ELSE NULL END,
           display_path=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_path ELSE NULL END,
           display_edge=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.display_edge ELSE NULL END,
           analysis_error=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error ELSE NULL END,
           analysis_error_at=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_at ELSE NULL END,
           analysis_error_kind=CASE WHEN photos.fingerprint_size IS excluded.fingerprint_size THEN photos.analysis_error_kind ELSE NULL END,
           fingerprint_size=excluded.fingerprint_size",
        params![
            Uuid::new_v4().to_string(),
            project_id,
            node.id,
            node.name,
            captured_at,
            source.as_str(),
            size
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// `run_scan` の Amazon 版。共有 → アルバム → FILE の一覧を読み、写真の行と tempLink を作る。
/// 共有リンクが消えていたら、状態を `missing` にして理由を返す（自動ではやり直さない）。
pub(crate) fn run_amazon_scan(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
    source: amazon::AmazonSource,
) -> Result<(), String> {
    let task_key = format!("scan:{project_id}");
    progress(&app, &project_id, "scan", "indexing", 0, 0, "Amazon Photos の一覧を読んでいます…");
    let conn = connection(&app)?;
    conn.execute(
        "UPDATE projects SET status='scanning', updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;

    let listed = amazon::fetch_share_root(&source)
        .and_then(|root| amazon::list_photos(&source, &root.node_id));
    let nodes = match listed {
        Ok(nodes) => nodes,
        Err(message) => {
            if message == amazon::GONE_MESSAGE {
                mark_amazon_gone(&conn, &project_id);
            } else {
                settle_project_status(&conn, &project_id);
            }
            return Err(message);
        }
    };
    let total = nodes.len();
    if registry.is_cancelled(&task_key) {
        settle_project_status(&conn, &project_id);
        progress(&app, &project_id, "scan", "cancelled", 0, total, "Scanning was cancelled.");
        return Ok(());
    }

    // 欠損の印は登録と同じトランザクションに入れる。途中で失敗しても走査前のまま残る。
    // 失敗で 'scanning' のまま残さない。
    let written = (|| -> Result<i64, String> {
        let transaction = conn.unchecked_transaction().map_err(|error| error.to_string())?;
        transaction
            .execute("UPDATE photos SET is_missing=1 WHERE project_id=?1", params![project_id])
            .map_err(|error| error.to_string())?;
        for (index, node) in nodes.iter().enumerate() {
            upsert_amazon_photo(&transaction, &project_id, node)?;
            if (index + 1) % 250 == 0 || index + 1 == total {
                progress(&app, &project_id, "scan", "indexing", index + 1, total, "Recording photo locations…");
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        // tempLink は node id で控える。拡大のたびに一覧をたどり直さないため。
        amazon::save_links(&conn, &project_id, &amazon::links_of(&nodes))?;
        recount_photos(&conn, &project_id)
    })();
    let count = match written {
        Ok(count) => count,
        Err(message) => {
            settle_project_status(&conn, &project_id);
            return Err(message);
        }
    };
    conn.execute(
        "UPDATE projects SET status='ready',updated_at=?1 WHERE id=?2",
        params![now(), project_id],
    )
    .map_err(|error| error.to_string())?;
    progress(&app, &project_id, "scan", "complete", count as usize, count as usize, "写真の読み込みが完了しました。");
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = handle.state::<TaskRegistry>();
        let _ = spawn_analysis(handle.clone(), &registry, project_id, AnalysisMode::Background);
    });
    Ok(())
}
