//! ローカルの SQLite（DB の場所・接続・整備（列の追加・移行・目印の補い）・置き場のフォルダ・設定の読み出し）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

pub(crate) fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    // ハッシュ値の計算式を core に替えたので、古いハッシュ値が混ざらないよう別のファイルにする。
    // 旧 `photo-curator.sqlite3` は読まない・消さない。
    Ok(dir.join("photo-curator-v2.sqlite3"))
}

/// スキーマ整備（`open_database`）を済ませた DB ファイルのパス。プロセスの中で 1 本の
/// ファイルにつき 1 回だけ整備し、以後の `connection()` は軽く開くだけにする（U27 R1）。
pub(crate) static MIGRATED_DATABASES: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn connection(app: &AppHandle) -> Result<Connection, String> {
    open_connection(&db_path(app)?)
}

/// 初回（プロセスで最初に開くとき）だけ `open_database` で整備し、2 回目以降は
/// 開いて接続の設定をするだけ。整備が失敗したら記録しないので、次回また整備する。
pub(crate) fn open_connection(path: &Path) -> Result<Connection, String> {
    // 整備中に別スレッドが同じ整備を重ねないよう、ロックを持ったまま整備する。
    let mut migrated = MIGRATED_DATABASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if migrated.iter().any(|known| known == path) {
        drop(migrated);
        let conn = Connection::open(path)
            .map_err(|error| database_error("ローカルデータベースを開けませんでした", error))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;
        // journal_mode=WAL はファイルに残る。synchronous だけは接続ごとの設定。
        conn.execute_batch("PRAGMA synchronous=NORMAL;")
            .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;
        return Ok(conn);
    }
    let conn = open_database(path)?;
    migrated.push(path.to_path_buf());
    Ok(conn)
}

// サムネイルの置き場。DB と同じ app_data_dir 配下に置くので、
// 利用者のデータをまたいで散らばらない。
pub(crate) fn thumbnail_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(THUMBNAIL_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// 表示用画像の置き場。サムネイルと同じ app_data_dir 配下。
pub(crate) fn display_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(DISPLAY_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// アプリのデータフォルダの下の置き場（なければ作る）。
pub(crate) fn data_subdir(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(name);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

pub(crate) fn database_error(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}: {error}")
}

pub(crate) fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
    Ok(names.iter().any(|name| name == column))
}

pub(crate) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if !has_column(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    }
    Ok(())
}

// 旧ビルドで作られた photos 行は fingerprint 列が NULL のまま残っている。
// その状態ではハッシュの再利用判定が常に失敗し、キャッシュが原理的に効かない。
// さらに scan の upsert が fingerprint の一致で captured_at / d_hash の保持を
// 判定するため、NULL のままだと再 scan のたびに解析結果が消える。
// ここでは captured_at と d_hash には一切触れず、現在のファイルから
// fingerprint だけを補う。冪等。
pub(crate) fn backfill_missing_fingerprints(conn: &Connection) -> Result<(), String> {
    let pending: Vec<(String, String)> = {
        let mut statement = conn
            .prepare(
                // Amazon の行は fingerprint を持たない（実在しない相対パスの stat が毎回
                // 失敗するだけ）ので、フォルダ以外のプロジェクトの行は除く。
                "SELECT id,path FROM photos
                 WHERE (fingerprint_mtime IS NULL OR fingerprint_size IS NULL)
                   AND project_id NOT IN (SELECT id FROM projects WHERE source_kind<>'folder')
                 LIMIT ?1",
            )
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
        let rows = statement
            .query_map(params![FINGERPRINT_BACKFILL_LIMIT as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| database_error("ローカルデータベースを確認できませんでした", error))?
    };
    if pending.is_empty() {
        return Ok(());
    }

    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    for (id, path) in &pending {
        // ファイルが見つからない行はそのままにする。次回の scan で
        // is_missing として扱われる。
        if let Some((mtime, size)) = fingerprint(Path::new(path)) {
            transaction
                .execute(
                    "UPDATE photos SET fingerprint_mtime=?1,fingerprint_size=?2 WHERE id=?3",
                    params![mtime, size, id],
                )
                .map_err(|error| {
                    database_error("ローカルデータベースを更新できませんでした", error)
                })?;
        }
    }
    transaction
        .commit()
        .map_err(|error| database_error("ローカルデータベースを更新できませんでした", error))?;
    Ok(())
}

pub(crate) fn open_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|error| database_error("ローカルデータベースを開けませんでした", error))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| database_error("ローカルデータベースを設定できませんでした", error))?;

    // Keep this bootstrap schema compatible with databases created by earlier
    // builds. Indexes that reference migrated columns must be created only
    // after those columns have been added below.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
           photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS photos (
           id TEXT PRIMARY KEY, project_id TEXT NOT NULL, path TEXT NOT NULL,
           relative_path TEXT NOT NULL, name TEXT NOT NULL, captured_at INTEGER,
           d_hash TEXT, rating INTEGER NOT NULL DEFAULT 0,
           fingerprint_mtime INTEGER, fingerprint_size INTEGER,
           is_missing INTEGER NOT NULL DEFAULT 0,
           UNIQUE(project_id, path)
         );
         CREATE TABLE IF NOT EXISTS project_states (
           project_id TEXT PRIMARY KEY, state_json TEXT NOT NULL, updated_at INTEGER NOT NULL
         );",
    )
    .map_err(|error| database_error("ローカルデータベースを初期化できませんでした", error))?;

    add_column_if_missing(&conn, "photos", "fingerprint_mtime", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "fingerprint_size", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "is_missing", "INTEGER NOT NULL DEFAULT 0")?;
    // captured_at をどの経路で得たか。連写判定の重み付けに使う。
    // 連写まとめの閾値はプロジェクトごとに学習する。固定値
    // HASH_DISTANCE_LIMIT は実データの距離分布の最も密な領域に当たっており、
    // どのフォルダでも同じ値が正しいという前提が成り立たない。
    add_column_if_missing(&conn, "projects", "burst_threshold", "INTEGER")?;
    add_column_if_missing(&conn, "projects", "burst_threshold_learned_at", "INTEGER")?;

    // 選別結果。以前は session の JSON にしか無く、DB へは一度も書かれていなかった
    // ため、アプリ側で結果を見ることができなかった。
    add_column_if_missing(&conn, "photos", "eliminated_round", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "confirmed", "INTEGER NOT NULL DEFAULT 0")?;

    add_column_if_missing(&conn, "photos", "timestamp_source", "TEXT")?;
    // 使い回すサムネイルの参照と、それを作った時点のソースの fingerprint。
    add_column_if_missing(&conn, "photos", "thumbnail_path", "TEXT")?;
    add_column_if_missing(&conn, "photos", "thumbnail_mtime", "INTEGER")?;
    add_column_if_missing(&conn, "photos", "thumbnail_size", "INTEGER")?;
    // どのデコード経路でサムネイルを作ったか。速い経路がどれだけ効いているかを
    // あとから実データで確かめられるようにしておく。
    add_column_if_missing(&conn, "photos", "thumbnail_source", "TEXT")?;
    // サムネイルの生成方式。旧ビルドが作った行は NULL になり、版が合わないので
    // 原本から作り直される。回転を無視して保存された古いサムネイルは、これで
    // 自動的に置き換わる。
    add_column_if_missing(&conn, "photos", "thumbnail_version", "INTEGER")?;
    // 選別画面に出す表示用画像と、**実際に生成した長辺**。
    // 長辺を持たないと、設定を変えても古い画像が使われ続ける
    // （thumbnail_version と同じ轍）。
    add_column_if_missing(&conn, "photos", "display_path", "TEXT")?;
    add_column_if_missing(&conn, "photos", "display_edge", "INTEGER")?;
    // プロジェクトごとの上書き。NULL なら全体の設定に従う。
    add_column_if_missing(&conn, "projects", "display_edge", "INTEGER")?;
    // 写真の出所（T9）。既存のプロジェクトはすべてフォルダ。Amazon は `source_key` に
    // `"{host}|{shareId}"` を持ち、`folder_path` にはリンクの URL を置く。
    add_column_if_missing(&conn, "projects", "source_kind", "TEXT NOT NULL DEFAULT 'folder'")?;
    add_column_if_missing(&conn, "projects", "source_key", "TEXT")?;
    // 同名の JPEG と RAW を 1 枚の写真として扱う（U46）。既存のプロジェクトも既定の 1（オン）。
    add_column_if_missing(&conn, "projects", "pair_raw_jpeg", "INTEGER NOT NULL DEFAULT 1")?;
    // その設定を切り替えた時刻（U48）。既存のプロジェクトは 0（一度も切り替えていない）。
    add_column_if_missing(&conn, "projects", "pair_raw_jpeg_at", "INTEGER NOT NULL DEFAULT 0")?;
    // d_hash の算出方式。旧ビルドの行は NULL になり、キャッシュとして使われない。
    // 古い方式のハッシュと新しい方式のハッシュが混ざると連写判定が壊れるため、
    // 値を消さずに「使わない」ことで移行する。
    add_column_if_missing(&conn, "photos", "d_hash_version", "INTEGER")?;
    // 解析できなかった理由と、その時刻。デコードエラー・非対応形式・権限エラー・
    // timeout がここに残る。成功したら NULL に戻す。UI はこの列の件数を出して
    // 「◯件を解析できませんでした」と伝え、**解析自体は続行する**。
    add_column_if_missing(&conn, "photos", "analysis_error", "TEXT")?;
    add_column_if_missing(&conn, "photos", "analysis_error_at", "INTEGER")?;
    // 解析できなかった理由の種類（U58）。'unsupported'＝読めたのに復号できない（原本が変わるまで
    // 再試行しない）／'transient'＝読めない・打ち切り・網の失敗（次に開いたとき再試行）。
    // NULL は「失敗なし」か「U58 より前の失敗」（後者は一時的として 1 回やり直す）。
    add_column_if_missing(&conn, "photos", "analysis_error_kind", "TEXT")?;

    // 手で直したまとめ。**グループ単位では持てない。** まとめは dHash と閾値から
    // そのつど導出していて実体が無く、閾値が変われば別物になって紐づかないため。
    // 導出の材料である「隣り合うペア」に対する例外として持つ。
    //   split … 閾値では繋がるが、利用者が切った
    //   join  … 閾値では切れるが、利用者が繋いだ
    // グループは「連続するペアがすべて閾値を満たす区間」なので、これだけで
    // 分割・切り離し・結合・全解除のすべてを表せる。
    // アプリ全体の設定。いまは表示用サイズの既定だけだが、キーと値だけの表なので
    // 増えても migration が要らない。
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_settings (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );",
    )
    .map_err(|error| error.to_string())?;

    conn.execute_batch(
        // 手で直した連写の例外。core の `PairOverride`（鍵は relativePath）をそのまま持つ。
        // 旧版の `burst_pair_overrides`（photo id の組）は使わない。既存の DB には残るが読まない。
        "CREATE TABLE IF NOT EXISTS pair_overrides (
           project_id TEXT NOT NULL,
           left_path TEXT NOT NULL,
           right_path TEXT NOT NULL,
           decision TEXT NOT NULL,
           PRIMARY KEY (project_id, left_path, right_path)
         );",
    )
    .map_err(|error| error.to_string())?;

    // サイドカー（T8）。端末が覚える 3 つの値。
    sidecar::ensure_tables(&conn)?;
    // Amazon の共有リンクの tempLink の控え（T9）。
    amazon::ensure_tables(&conn)?;

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS photos_project_visible
           ON photos(project_id, is_missing, relative_path);
         CREATE INDEX IF NOT EXISTS photos_project_capture
           ON photos(project_id, is_missing, captured_at);",
    )
    .map_err(|error| database_error("ローカルデータベースの更新を完了できませんでした", error))?;

    backfill_missing_fingerprints(&conn)?;

    Ok(conn)
}
pub(crate) fn read_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key=?1",
        params![key],
        |row| row.get(0),
    )
    .ok()
}
