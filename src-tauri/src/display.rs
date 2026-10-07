//! 表示用画像の生成（DisplayJob・display_one・run_display_generation・Amazon の run_amazon_display）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

/// 表示用画像がまだ要る写真の条件（`?1`＝プロジェクト、`?2`＝長辺）。
/// 非対応の形式（U58）は、何度やっても作れないので含めない（開くたびに原本を丸読みして失敗しないように）。
pub(crate) const DISPLAY_PENDING_WHERE: &str = "project_id=?1 AND is_missing=0
       AND COALESCE(analysis_error_kind,'')<>'unsupported'
       AND (display_path IS NULL OR display_edge IS NULL OR display_edge <> ?2)";

/// まだ表示用画像が要る枚数。
pub(crate) fn display_backlog_count(conn: &Connection, project_id: &str, edge: u32) -> Result<i64, String> {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM photos WHERE {DISPLAY_PENDING_WHERE}"),
        params![project_id, edge as i64],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

/// 表示用画像を作る 1 枚。
#[derive(Clone, Debug)]
pub(crate) struct DisplayJob {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) stored_path: Option<String>,
    pub(crate) stored_edge: Option<i64>,
}

impl PhotoJob for DisplayJob {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

/// 表示用画像 1 枚を作ってファイルへ書く（worker 側。DB には触れない）。
/// 作れなかったら `error` に理由を入れる（writer が印を NULL に戻す）。
pub(crate) fn display_one(dir: &Path, edge: u32, index: usize, job: &DisplayJob) -> PhotoWork {
    let mut work = PhotoWork::new(index, &job.id);
    let existing = match (job.stored_path.as_deref(), job.stored_edge) {
        (Some(p), Some(e)) if e > 0 => Some((Path::new(p), e as u32)),
        _ => None,
    };
    let built = build_display(&LocalPhoto(Path::new(&job.path)), edge, existing);
    let file = display_file(dir, &job.id);
    let saved = built.and_then(|bytes| fs::write(&file, &bytes).ok());
    if saved.is_none() {
        work.fail(PhotoFailure::DisplayFailed);
    }
    work
}

/// 表示用画像をまとめて作る。**走査とは分ける。**
///
/// 表示用は原本を全部読むので、走査に混ぜると解析が桁で遅くなる
/// （Routine 1 実測: EXIF サムネイル経路 1.72ms/枚 に対しフルデコード 132ms/枚）。
/// 走査が終われば dHash は揃うので、連写のまとめと閾値学習はすぐ始められる。
/// こちらは裏で溜めていく。
pub(crate) fn run_display_generation(
    app: AppHandle,
    registry: &TaskRegistry,
    project_id: String,
) -> Result<(), String> {
    let task_key = format!("display:{project_id}");
    let edge = resolve_display_edge(&app, &project_id)?;
    let dir = display_dir(&app)?;

    let pending: Vec<(String, String, Option<String>, Option<i64>)> = {
        let conn = connection(&app)?;
        let mut statement = conn
            .prepare(
                &format!(
                    "SELECT id,path,display_path,display_edge FROM photos
                     WHERE {DISPLAY_PENDING_WHERE}
                     ORDER BY captured_at IS NULL, captured_at"
                ),
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id, edge as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    // Amazon は縮小を Amazon にさせる（`viewBox=<長辺>`）。原本は読まない。
    if let Some(book) = amazon_book_of(&app, &project_id)? {
        let targets = pending
            .into_iter()
            .map(|(photo_id, node_id, _, _)| (photo_id, node_id))
            .collect();
        return run_amazon_display(&app, registry, &project_id, book, edge, &dir, targets);
    }

    let total = pending.len();
    progress(
        &app,
        &project_id,
        "display",
        "hashing",
        0,
        total,
        "選別用の画像を作っています…",
    );

    // 原本を読んで表示用に縮める工程は `run_in_parallel` に載せる（U27 R2）。worker は
    // 画像を作ってファイルへ書くだけで DB に触れない。単一の writer が 100 件ずつ UPDATE する。
    // 並列数は解析と同じ（ローカルは 2〜4、ネットワークのフォルダは 2）。
    let folder = project_folder(&app, &project_id)?;
    let workers = analysis_worker_count_for(Path::new(&folder));
    let jobs: Vec<DisplayJob> = pending
        .into_iter()
        .map(|(id, path, stored_path, stored_edge)| DisplayJob {
            id,
            path,
            stored_path,
            stored_edge,
        })
        .collect();
    let conn = connection(&app)?;
    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        if item.error.is_none() {
            tx.execute(
                "UPDATE photos SET display_path=?1, display_edge=?2 WHERE id=?3",
                params![
                    display_file(&dir, &item.photo_id).to_string_lossy().to_string(),
                    edge as i64,
                    item.photo_id
                ],
            )
        } else {
            // 作れなかった写真は次回また拾えるよう、印を残さない。
            tx.execute(
                "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                params![item.photo_id],
            )
        }
        .map_err(|error| error.to_string())?;
        Ok(())
    };
    let mut pending_results: Vec<PhotoWork> = Vec::new();
    let mut committed = 0usize;
    let dir_for_workers = dir.clone();
    let outcome = run_in_parallel(
        Arc::new(jobs),
        workers,
        Duration::from_millis(DISPLAY_TIMEOUT_MS),
        &|| registry.is_cancelled(&task_key),
        move |index, job: &DisplayJob| display_one(&dir_for_workers, edge, index, job),
        &mut |item| {
            pending_results.push(item);
            if pending_results.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending_results, &apply)?;
            }
            let done = committed + pending_results.len();
            if done % 10 == 0 || done == total {
                progress(
                    &app,
                    &project_id,
                    "display",
                    "hashing",
                    done,
                    total,
                    "選別用の画像を作っています…",
                );
            }
            Ok(())
        },
    )?;
    committed += flush_results(&conn, &mut pending_results, &apply)?;
    if outcome.cancelled {
        progress_note(
            &app,
            &project_id,
            "display",
            "cancelled",
            committed,
            total,
            format!("中断しました。{committed} 件まで作成済みです。"),
            ProgressNote {
                warning: None,
                failed: 0,
            },
        );
        return Ok(());
    }

    progress(
        &app,
        &project_id,
        "display",
        "complete",
        total,
        total,
        "選別用の画像がそろいました。",
    );
    Ok(())
}

/// `run_display_generation` の Amazon 版。`viewBox=<表示用の長辺>` のバイトを、そのまま表示用のファイルに書く。
/// 並列 4。まとまり（16 枚）ごとに DB へ書き、中断とリンク切れを見る。
pub(crate) fn run_amazon_display(
    app: &AppHandle,
    registry: &TaskRegistry,
    project_id: &str,
    book: Arc<amazon::LinkBook>,
    edge: u32,
    dir: &Path,
    pending: Vec<(String, String)>,
) -> Result<(), String> {
    let task_key = format!("display:{project_id}");
    let total = pending.len();
    let message = "選別用の画像を作っています…";
    progress(app, project_id, "display", "hashing", 0, total, message);
    let mut done = 0usize;
    let mut failed = 0usize;
    for batch in pending.chunks(amazon::WORKERS * 4) {
        if registry.is_cancelled(&task_key) {
            progress_note(
                app,
                project_id,
                "display",
                "cancelled",
                done,
                total,
                format!("中断しました。{done} 件まで作成済みです。"),
                ProgressNote { warning: None, failed },
            );
            return Ok(());
        }
        if book.is_gone() {
            break;
        }
        let next = AtomicUsize::new(0);
        let saved: Mutex<Vec<(usize, bool)>> = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..amazon::WORKERS.min(batch.len()) {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some((photo_id, node_id)) = batch.get(index) else { break };
                    let ok = book
                        .fetch(node_id, Some(edge))
                        .ok()
                        // 画像として読める形のときだけ置く（エラーの文書を表示用にしない）。
                        .filter(|bytes| image::guess_format(bytes).is_ok())
                        .is_some_and(|bytes| write_atomically(&display_file(dir, photo_id), &bytes).is_ok());
                    if let Ok(mut list) = saved.lock() {
                        list.push((index, ok));
                    }
                });
            }
        });
        let results = saved.into_inner().map_err(|_| "表示用画像の結果を読めません。".to_string())?;
        let conn = connection(app)?;
        for (index, ok) in results {
            let photo_id = &batch[index].0;
            if ok {
                conn.execute(
                    "UPDATE photos SET display_path=?1, display_edge=?2 WHERE id=?3",
                    params![display_file(dir, photo_id).to_string_lossy().to_string(), edge as i64, photo_id],
                )
                .map_err(|error| error.to_string())?;
            } else {
                // 作れなかった写真は次回また拾えるよう、印を残さない。
                failed += 1;
                conn.execute(
                    "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                    params![photo_id],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        done += batch.len();
        progress_note(
            app,
            project_id,
            "display",
            "hashing",
            done,
            total,
            message,
            ProgressNote { warning: None, failed },
        );
    }
    if book.is_gone() {
        mark_amazon_gone(&connection(app)?, project_id);
        return Err(amazon::GONE_MESSAGE.to_string());
    }
    progress_note(
        app,
        project_id,
        "display",
        "complete",
        total,
        total,
        "選別用の画像がそろいました。",
        ProgressNote { warning: None, failed },
    );
    Ok(())
}
