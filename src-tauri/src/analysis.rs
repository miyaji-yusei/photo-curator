//! 解析（撮影時刻の段 metadata_one・ハッシュの段 hash_one／hash_one_amazon・run_burst_analysis）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

/// 撮影時刻の段の worker 1本ぶん。先頭 64KB を 1 回だけ読み、画像かどうかと
/// 撮影時刻を決める。**DB には触れない。**
pub(crate) fn metadata_one(index: usize, job: &MetadataJob) -> PhotoWork {
    let mut result = PhotoWork::new(index, &job.id);
    let path = Path::new(&job.path);
    let source = LocalPhoto(path);
    let head = source.head(EXIF_HEAD_PROBE);
    if let Some(head) = head.as_deref() {
        if content_is_not_image(head, path) {
            result.not_image = true;
            return result;
        }
    }
    match capture_time_from_head(&source, head.as_deref()) {
        Some(capture) => {
            result.captured_at = Some(capture.at);
            result.timestamp_source = Some(capture.source);
        }
        None => {
            result.error = Some("撮影時刻を読み取れませんでした。".into());
        }
    }
    result
}

/// 撮影時刻の段の結果を書く。画像ではなかった行は数から外す（`is_missing=1`）。
/// 撮影時刻は空のままにするので、再走査で行が戻っても次の解析でまた判定される。
pub(crate) fn apply_metadata(tx: &Connection, item: &PhotoWork) -> Result<(), String> {
    if item.not_image {
        tx.execute(
            "UPDATE photos SET is_missing=1,captured_at=NULL,timestamp_source=NULL,
               analysis_error=NULL,analysis_error_at=NULL
             WHERE id=?1",
            params![item.photo_id],
        )
        .map_err(|error| error.to_string())?;
        return Ok(());
    }
    tx.execute(
        "UPDATE photos SET captured_at=?1,timestamp_source=?2,
           analysis_error=?3,analysis_error_at=?4
         WHERE id=?5",
        params![
            item.captured_at,
            item.timestamp_source
                .unwrap_or(TimestampSource::Unknown)
                .as_str(),
            item.error,
            item.error.as_ref().map(|_| now()),
            item.photo_id
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// プロジェクトの写真の数を、欠損を除いて数え直す。
pub(crate) fn recount_photos(conn: &Connection, project_id: &str) -> Result<i64, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE project_id=?1 AND is_missing=0",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE projects SET photo_count=?1 WHERE id=?2",
        params![count, project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(count)
}

/// worker 1本ぶんの仕事。1枚を読み、サムネイルと dHash を作る。
/// **DB には触れない。**書き込みは writer が `flush_results` でまとめて行う。
pub(crate) fn hash_one(thumbnails: &Path, index: usize, record: &HashRecord) -> PhotoWork {
    let mut result = PhotoWork::new(index, &record.id);
    let photo_path = Path::new(&record.path);
    let current = fingerprint(photo_path);
    let analysed = analyse_photo(thumbnails, &record.id, photo_path, current, &record.cached);
    result.fingerprint = current;
    result.d_hash = analysed.d_hash;
    result.thumbnail_path = analysed.thumbnail_path;
    result.thumbnail_source = analysed.thumbnail_state.decode_source();
    result.hash_reused = analysed.hash_reused;
    if result.d_hash.is_none() {
        result.error = Some(match current {
            // ファイルは在るのに読めない = 壊れている / 非対応形式。
            Some(_) => "画像を読み取れませんでした（破損または非対応の形式）。".into(),
            None => "ファイルを開けませんでした（移動・削除・権限）。".into(),
        });
    }
    result
}

/// `hash_one` の Amazon 版。`viewBox=160` の画像を取り、旧版と同じサムネイルとハッシュ値にする。
/// サムネイルが残っていて版も合えば、網を使わない。**DB には触れない。**
pub(crate) fn hash_one_amazon(
    book: &amazon::LinkBook,
    thumbnails: &Path,
    index: usize,
    record: &HashRecord,
) -> PhotoWork {
    let mut result = PhotoWork::new(index, &record.id);
    let file = thumbnail_file(thumbnails, &record.id);
    let stored = file.to_string_lossy().to_string();
    let cached = &record.cached;
    if cached.thumbnail_path.as_deref() == Some(stored.as_str())
        && cached.thumbnail_version == Some(THUMBNAIL_VERSION)
        && file.is_file()
    {
        if cached.d_hash.is_some() && cached.d_hash_version == Some(D_HASH_VERSION) {
            result.d_hash = cached.d_hash.clone();
            result.thumbnail_path = Some(stored);
            result.hash_reused = true;
            return result;
        }
        if let Some(hash) = fs::read(&file).ok().as_deref().and_then(hash_thumbnail_bytes) {
            result.d_hash = Some(hash);
            result.thumbnail_path = Some(stored);
            return result;
        }
    }
    let bytes = match book.fetch(&record.path, Some(AMAZON_THUMBNAIL_EDGE)) {
        Ok(bytes) => bytes,
        Err(message) => {
            result.error = Some(message);
            return result;
        }
    };
    let Some(image) = image::load_from_memory(&bytes).ok() else {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
        return result;
    };
    let Some(thumbnail) = encode_thumbnail(&scale_for_thumbnail(&image)) else {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
        return result;
    };
    result.d_hash = hash_thumbnail_bytes(&thumbnail);
    if write_atomically(&file, &thumbnail).is_ok() {
        result.thumbnail_path = Some(stored);
        result.thumbnail_source = Some("amazon");
    }
    if result.d_hash.is_none() {
        result.error = Some("画像を読み取れませんでした（破損または非対応の形式）。".into());
    }
    result
}

/// Amazon に縮小させるときの長辺（サムネイルとハッシュ値のもと）。
pub(crate) const AMAZON_THUMBNAIL_EDGE: u32 = 160;

/// 解析できなかった写真の件数。UI へそのまま渡す。
pub(crate) fn failed_photo_count(conn: &Connection, project_id: &str) -> Result<usize, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM photos
         WHERE project_id=?1 AND is_missing=0 AND analysis_error IS NOT NULL",
        params![project_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as usize)
    .map_err(|error| error.to_string())
}

pub(crate) fn run_burst_analysis(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
    mode: AnalysisMode,
) -> Result<(), String> {
    let task = mode.task();
    let task_key = format!("{task}:{project_id}");
    // 事前生成は前面の解析に道を譲る。同じ行を両方が触っても結果は同じだが、
    // ディスクを取り合って前面を遅くする意味が無い。
    let foreground_key = format!("{}:{project_id}", AnalysisMode::Foreground.task());
    let should_stop = || {
        registry.is_cancelled(&task_key)
            || (mode == AnalysisMode::Background && registry.is_running(&foreground_key))
    };
    let timeout = Duration::from_millis(PHOTO_TIMEOUT_MS);
    let thumbnails = thumbnail_dir(&app)?;
    // ネットワークのフォルダは worker 数を抑えるので、フォルダの場所を先に知る。
    let folder = project_folder(&app, &project_id)?;
    // Amazon の共有リンクは、撮影時刻を一覧の contentDate から走査で入れてあり、
    // サムネイル・ハッシュ値は viewBox=160 の画像から作る。並列は WORKERS（8）。
    let amazon_book = amazon_book_of(&app, &project_id)?;
    let workers = if amazon_book.is_some() {
        amazon::WORKERS
    } else {
        mode.workers(Path::new(&folder))
    };
    let conn = connection(&app)?;
    let records: Vec<MetadataRecord> = {
        let mut statement = conn
            .prepare("SELECT id,path,captured_at,timestamp_source FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY path")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    // 旧ビルドの行は captured_at はあっても timestamp_source が無い。経路が
    // 分からないままだと連写判定で信用度を測れないので、その場合も読み直す
    // （EXIF 読取は 0.14 ms/枚）。既に両方ある行は仕事そのものを作らない。
    let metadata_jobs: Vec<MetadataJob> = records
        .iter()
        .filter(|(_, _, captured_at, source)| {
            amazon_book.is_none() && (captured_at.is_none() || source.is_none())
        })
        .map(|(id, path, _, _)| MetadataJob {
            id: id.clone(),
            path: path.clone(),
        })
        .collect();
    let metadata_total = metadata_jobs.len();
    let mut failed = failed_photo_count(&conn, &project_id)?;

    if metadata_total > 0 {
        progress(
            &app,
            &project_id,
            task,
            "metadata",
            0,
            metadata_total,
            "撮影時刻を読み取っています…",
        );
        let conn = connection(&app)?;
        let interval = progress_interval(metadata_total);
        let mut pending: Vec<PhotoWork> = Vec::with_capacity(ANALYSIS_CHUNK_SIZE);
        let mut committed = 0usize;
        let apply = apply_metadata;
        let outcome = run_in_parallel(
            Arc::new(metadata_jobs),
            workers,
            timeout,
            &should_stop,
            // worker はファイルを読むだけ。DB には触れない。
            |index, job: &MetadataJob| metadata_one(index, job),
            &mut |item| {
                if item.error.is_some() {
                    failed += 1;
                }
                pending.push(item);
                if pending.len() >= ANALYSIS_CHUNK_SIZE {
                    committed += flush_results(&conn, &mut pending, &apply)?;
                    progress_note(
                        &app,
                        &project_id,
                        task,
                        "metadata",
                        committed,
                        metadata_total,
                        "撮影時刻を読み取っています…",
                        ProgressNote {
                            warning: None,
                            failed,
                        },
                    );
                    if mode == AnalysisMode::Background {
                        std::thread::sleep(Duration::from_millis(BACKGROUND_PAUSE_MS));
                    }
                } else if (committed + pending.len()) % interval == 0 {
                    progress_note(
                        &app,
                        &project_id,
                        task,
                        "metadata",
                        committed + pending.len(),
                        metadata_total,
                        "撮影時刻を読み取っています…",
                        ProgressNote {
                            warning: None,
                            failed,
                        },
                    );
                }
                Ok(())
            },
        )?;
        // キャンセルされていても、読み終わっているぶんは書いてから抜ける。
        committed += flush_results(&conn, &mut pending, &apply)?;
        // 中身が画像ではなかった写真は数から外れているので、件数を数え直す。
        recount_photos(&conn, &project_id)?;
        if outcome.cancelled {
            progress_note(
                &app,
                &project_id,
                task,
                "cancelled",
                committed,
                metadata_total,
                format!("解析を中断しました。{committed} 件まで保存済みです。"),
                ProgressNote {
                    warning: None,
                    failed,
                },
            );
            return Ok(());
        }
    }

    let candidates: Vec<HashRecord> = {
        let conn = connection(&app)?;
        // Amazon は撮影時刻の無い写真にもサムネイルが要る（一覧の絵になる）ので絞らない。
        let dated_only = if amazon_book.is_some() {
            ""
        } else {
            "AND captured_at IS NOT NULL"
        };
        let mut statement = conn
            .prepare(&format!(
                "SELECT id,path,captured_at,timestamp_source,d_hash,d_hash_version,
                        thumbnail_path,thumbnail_mtime,thumbnail_size,thumbnail_version
                 FROM photos
                 WHERE project_id=?1 AND is_missing=0 {dated_only}
                 ORDER BY captured_at IS NULL, captured_at, path"
            ))
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                let source: Option<String> = row.get(3)?;
                Ok(HashRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    captured_at: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    source: TimestampSource::parse(source.as_deref()),
                    cached: CachedAnalysis {
                        d_hash: row.get(4)?,
                        d_hash_version: row.get(5)?,
                        thumbnail_path: row.get(6)?,
                        thumbnail_mtime: row.get(7)?,
                        thumbnail_size: row.get(8)?,
                        thumbnail_version: row.get(9)?,
                    },
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    // 時間が近いものだけがハッシュを必要とする。ただし mtime しか根拠が無い
    // ペアは連写とみなさず、候補が多すぎるときは時間窓そのものを詰める。
    let selection = select_burst_candidates(
        &candidates
            .iter()
            .map(|record| CandidateInput {
                id: record.id.clone(),
                captured_at: record.captured_at,
                source: record.source,
            })
            .collect::<Vec<_>>(),
    );
    let needed_records: Vec<_> = if amazon_book.is_some() {
        // Amazon は小さい画像を取るだけなので、近い写真に絞らず全部を作る。
        candidates.clone()
    } else {
        candidates
            .iter()
            .filter(|record| selection.ids.contains(&record.id))
            .cloned()
            .collect()
    };
    let needed_total = needed_records.len();
    let warning = (selection.narrowed && amazon_book.is_none()).then(|| {
        format!(
            "撮影間隔が近い写真が多すぎたため、連写とみなす時間の幅を {:.1} 秒に狭めました（候補 {} / {} 枚）。",
            selection.window_ms as f64 / 1000.0,
            needed_total,
            candidates.len()
        )
    });
    progress_note(
        &app,
        &project_id,
        task,
        "hashing",
        0,
        needed_total,
        "近い構図を比較しています…",
        ProgressNote { warning, failed },
    );

    let conn = connection(&app)?;
    let interval = progress_interval(needed_total.max(1));
    let mut pending: Vec<PhotoWork> = Vec::with_capacity(ANALYSIS_CHUNK_SIZE);
    let mut committed = 0usize;
    let is_amazon = amazon_book.is_some();
    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        // 据え置きで済んだ1枚は、書き込む理由が無い。
        if item.hash_reused {
            return Ok(());
        }
        if is_amazon {
            // 原本の mtime は無い。走査が入れた大きさ（fingerprint_size）は触らない。
            tx.execute(
                "UPDATE photos SET d_hash=?1,d_hash_version=?2,thumbnail_path=?3,
                   thumbnail_source=COALESCE(?4,thumbnail_source),
                   thumbnail_version=?5,
                   analysis_error=?6,analysis_error_at=?7
                 WHERE id=?8",
                params![
                    item.d_hash,
                    item.d_hash.as_ref().map(|_| D_HASH_VERSION),
                    item.thumbnail_path,
                    item.thumbnail_source,
                    item.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
                    item.error,
                    item.error.as_ref().map(|_| now()),
                    item.photo_id
                ],
            )
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
        // metadata が読めない場合は fingerprint を NULL のままにする。(0,0) を
        // 入れると、読めないファイル同士が同じ fingerprint に見えてキャッシュが
        // 誤ヒットする。
        let (mtime, size) = match item.fingerprint {
            Some((mtime, size)) => (Some(mtime), Some(size)),
            None => (None, None),
        };
        // サムネイルを保存できたときだけ、それを作った時点の fingerprint を
        // 控える。次回の無効化判定はこの一致で行う。
        let (thumb_mtime, thumb_size) = match item.thumbnail_path {
            Some(_) => (mtime, size),
            None => (None, None),
        };
        tx.execute(
            "UPDATE photos SET d_hash=?1,d_hash_version=?2,thumbnail_path=?3,
               thumbnail_mtime=?4,thumbnail_size=?5,
               thumbnail_source=COALESCE(?6,thumbnail_source),
               thumbnail_version=?7,
               fingerprint_mtime=?8,fingerprint_size=?9,
               analysis_error=?10,analysis_error_at=?11
             WHERE id=?12",
            params![
                item.d_hash,
                item.d_hash.as_ref().map(|_| D_HASH_VERSION),
                item.thumbnail_path,
                thumb_mtime,
                thumb_size,
                item.thumbnail_source,
                // 版はサムネイルを保存できたときだけ立てる。パスが NULL のまま
                // 版だけ残ると、次回「使える」と誤判定する。
                item.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
                mtime,
                size,
                item.error,
                item.error.as_ref().map(|_| now()),
                item.photo_id
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    };
    let thumbnails_for_workers = thumbnails.clone();
    let book_for_workers = amazon_book.clone();
    let outcome = run_in_parallel(
        Arc::new(needed_records),
        workers,
        timeout,
        &should_stop,
        // worker はファイルを読んでサムネイルを書くだけ。DB には触れない。
        move |index, record: &HashRecord| match &book_for_workers {
            Some(book) => hash_one_amazon(book, &thumbnails_for_workers, index, record),
            None => hash_one(&thumbnails_for_workers, index, record),
        },
        &mut |item| {
            if item.error.is_some() {
                failed += 1;
            }
            pending.push(item);
            if pending.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending, &apply)?;
                progress_note(
                    &app,
                    &project_id,
                    task,
                    "hashing",
                    committed,
                    needed_total,
                    "近い構図を比較しています…",
                    ProgressNote {
                        warning: None,
                        failed,
                    },
                );
                if mode == AnalysisMode::Background {
                    std::thread::sleep(Duration::from_millis(BACKGROUND_PAUSE_MS));
                }
            } else if (committed + pending.len()) % interval == 0 {
                progress_note(
                    &app,
                    &project_id,
                    task,
                    "hashing",
                    committed + pending.len(),
                    needed_total,
                    "近い構図を比較しています…",
                    ProgressNote {
                        warning: None,
                        failed,
                    },
                );
            }
            Ok(())
        },
    )?;
    committed += flush_results(&conn, &mut pending, &apply)?;
    // リンクが消えていたら、ここで止めて理由を伝える。開いたときに自動では続けない。
    if amazon_book.as_ref().is_some_and(|book| book.is_gone()) {
        mark_amazon_gone(&conn, &project_id);
        return Err(amazon::GONE_MESSAGE.to_string());
    }
    if outcome.cancelled {
        progress_note(
            &app,
            &project_id,
            task,
            "cancelled",
            committed,
            needed_total,
            format!(
                "解析を中断しました。{committed} 件まで保存済みです。次回はここから再開します。"
            ),
            ProgressNote {
                warning: None,
                failed,
            },
        );
        return Ok(());
    }
    progress_note(
        &app,
        &project_id,
        task,
        "complete",
        needed_total,
        needed_total,
        "連写候補の準備ができました。",
        ProgressNote {
            warning: None,
            failed,
        },
    );
    Ok(())
}
