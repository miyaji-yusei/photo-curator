use super::*;
use image::GenericImageView;

#[test]
fn migrates_the_legacy_photo_schema_before_creating_indexes() {
    let directory =
        std::env::temp_dir().join(format!("photo-curator-schema-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).expect("create test directory");
    let path = directory.join("legacy.sqlite3");

    let legacy = Connection::open(&path).expect("open legacy database");
    legacy
        .execute_batch(
            "CREATE TABLE projects (
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                   photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                   created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE photos (
                   id TEXT PRIMARY KEY, project_id TEXT NOT NULL, path TEXT NOT NULL,
                   relative_path TEXT NOT NULL, name TEXT NOT NULL, captured_at INTEGER,
                   d_hash TEXT, rating INTEGER NOT NULL DEFAULT 0,
                   UNIQUE(project_id, path)
                 );",
        )
        .expect("create legacy schema");
    drop(legacy);

    let migrated = open_database(&path).expect("migrate legacy database");
    assert!(has_column(&migrated, "photos", "fingerprint_mtime").unwrap());
    assert!(has_column(&migrated, "photos", "fingerprint_size").unwrap());
    assert!(has_column(&migrated, "photos", "is_missing").unwrap());
    let visible_index: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='photos_project_visible'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(visible_index, 1);
    drop(migrated);

    // Running migrations on every command must remain safe and idempotent.
    open_database(&path).expect("reopen migrated database");
    fs::remove_dir_all(&directory).expect("remove test directory");
}

fn test_directory(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("photo-curator-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).expect("create test directory");
    directory
}

// -----------------------------------------------------------------------
// 画像・EXIF のフィクスチャ
//
// デコード経路の分岐は「EXIF サムネイルが実在するか」で決まる。合成画像を
// 置くだけでは②の経路を一度も通らないので、APP1 に本物の TIFF ブロックを
// 組み立てて埋め込む。
// -----------------------------------------------------------------------

fn synthetic_image(width: u32, height: u32, seed: u8) -> DynamicImage {
    let mut buffer = image::RgbImage::new(width, height);
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        // 一様な塗りだと resize 後に全画素が同値になり dHash が縮退する。
        let value = ((x * 7 + y * 13) % 256) as u8;
        *pixel = image::Rgb([value, value.wrapping_add(seed), 255 - value]);
    }
    DynamicImage::ImageRgb8(buffer)
}

fn jpeg_bytes(width: u32, height: u32, seed: u8) -> Vec<u8> {
    encode_thumbnail(&synthetic_image(width, height, seed)).expect("encode fixture jpeg")
}

enum Val {
    Ascii(String),
    /// SHORT 1個。4バイトの値欄に収まるので inline に置く。
    Short(u16),
    ExifPointer,
    ThumbOffset,
    ThumbLength,
}

fn ifd_size(entries: usize) -> usize {
    2 + 12 * entries + 4
}

fn resolve_entries(
    entries: &[(u16, Val)],
    data: &mut Vec<u8>,
    data_off: usize,
    exif_off: u32,
    thumb: (u32, u32),
) -> Vec<(u16, u16, u32, u32)> {
    entries
        .iter()
        .map(|(tag, value)| match value {
            Val::ExifPointer => (*tag, 4u16, 1u32, exif_off),
            Val::Short(value) => (*tag, 3, 1, u32::from(*value)),
            Val::ThumbOffset => (*tag, 4, 1, thumb.0),
            Val::ThumbLength => (*tag, 4, 1, thumb.1),
            Val::Ascii(text) => {
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(0);
                assert!(bytes.len() > 4, "4バイト以下の ASCII は inline 格納になる");
                let offset = (data_off + data.len()) as u32;
                data.extend_from_slice(&bytes);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
                (*tag, 2, bytes.len() as u32, offset)
            }
        })
        .collect()
}

fn write_ifd(out: &mut Vec<u8>, entries: &[(u16, u16, u32, u32)], next: u32) {
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&next.to_le_bytes());
}

/// IFD0（Orientation・DateTime）/ Exif サブIFD（DateTimeOriginal・
/// OffsetTimeOriginal）/ IFD1（Orientation・サムネイル）を持つ TIFF ブロックを
/// 組む。リトルエンディアン。**IFD の項目はタグの昇順**に並べる（規格の要求）。
fn tiff_block(
    datetime: Option<&str>,
    datetime_original: Option<&str>,
    offset_original: Option<&str>,
    orientation: Option<u16>,
    thumbnail_orientation: Option<u16>,
    thumbnail: Option<&[u8]>,
) -> Vec<u8> {
    let mut exif_entries = Vec::new();
    if let Some(value) = datetime_original {
        exif_entries.push((0x9003, Val::Ascii(value.to_owned())));
    }
    if let Some(value) = offset_original {
        exif_entries.push((0x9011, Val::Ascii(value.to_owned())));
    }
    let mut ifd0_entries = Vec::new();
    if let Some(value) = orientation {
        ifd0_entries.push((0x0112, Val::Short(value)));
    }
    if let Some(value) = datetime {
        ifd0_entries.push((0x0132, Val::Ascii(value.to_owned())));
    }
    if !exif_entries.is_empty() {
        ifd0_entries.push((0x8769, Val::ExifPointer));
    }
    let mut ifd1_entries = Vec::new();
    if let Some(value) = thumbnail_orientation {
        ifd1_entries.push((0x0112, Val::Short(value)));
    }
    if thumbnail.is_some() {
        ifd1_entries.push((0x0201, Val::ThumbOffset));
        ifd1_entries.push((0x0202, Val::ThumbLength));
    }

    let ifd0_off = 8usize;
    let exif_off = ifd0_off + ifd_size(ifd0_entries.len());
    let ifd1_off = exif_off
        + if exif_entries.is_empty() {
            0
        } else {
            ifd_size(exif_entries.len())
        };
    let data_off = ifd1_off
        + if ifd1_entries.is_empty() {
            0
        } else {
            ifd_size(ifd1_entries.len())
        };

    // ASCII を先に敷き、そのあとにサムネイル本体を置く。
    let mut data = Vec::new();
    let ifd0 = resolve_entries(&ifd0_entries, &mut data, data_off, exif_off as u32, (0, 0));
    let exif = resolve_entries(&exif_entries, &mut data, data_off, exif_off as u32, (0, 0));
    let thumb = match thumbnail {
        Some(bytes) => {
            let offset = (data_off + data.len()) as u32;
            data.extend_from_slice(bytes);
            (offset, bytes.len() as u32)
        }
        None => (0, 0),
    };
    let ifd1 = resolve_entries(&ifd1_entries, &mut data, data_off, exif_off as u32, thumb);

    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&(ifd0_off as u32).to_le_bytes());
    write_ifd(
        &mut out,
        &ifd0,
        if ifd1_entries.is_empty() {
            0
        } else {
            ifd1_off as u32
        },
    );
    if !exif_entries.is_empty() {
        write_ifd(&mut out, &exif, 0);
    }
    if !ifd1_entries.is_empty() {
        write_ifd(&mut out, &ifd1, 0);
    }
    assert_eq!(
        out.len(),
        data_off,
        "IFD の配置と計算した offset が一致する"
    );
    out.extend_from_slice(&data);
    out
}

/// SOI の直後に APP1(Exif) を差し込んだ JPEG を書き出す。
fn write_jpeg_with_exif(path: &Path, body: &[u8], tiff: Option<&[u8]>) {
    let mut out = Vec::new();
    out.extend_from_slice(&body[..2]); // SOI
    if let Some(tiff) = tiff {
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(tiff);
    }
    out.extend_from_slice(&body[2..]);
    fs::write(path, out).expect("write jpeg fixture");
}

struct Fixture {
    datetime: Option<String>,
    datetime_original: Option<String>,
    offset_original: Option<String>,
    /// IFD0 の Orientation。本体の画素に対する向きの指定。
    orientation: Option<u16>,
    /// IFD1 の Orientation。埋め込みサムネイルを既に正立させて保存する
    /// カメラを再現するために、IFD0 とは別に指定できる。
    thumbnail_orientation: Option<u16>,
    thumbnail: Option<(u32, u32)>,
    size: (u32, u32),
    seed: u8,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            datetime: None,
            datetime_original: None,
            offset_original: None,
            orientation: None,
            thumbnail_orientation: None,
            thumbnail: None,
            size: (600, 400),
            seed: 0,
        }
    }
}

impl Fixture {
    fn write(&self, path: &Path) {
        let body = jpeg_bytes(self.size.0, self.size.1, self.seed);
        let thumbnail = self
            .thumbnail
            .map(|(width, height)| jpeg_bytes(width, height, self.seed));
        let has_exif = self.datetime.is_some()
            || self.datetime_original.is_some()
            || self.orientation.is_some()
            || thumbnail.is_some();
        let tiff = has_exif.then(|| {
            tiff_block(
                self.datetime.as_deref(),
                self.datetime_original.as_deref(),
                self.offset_original.as_deref(),
                self.orientation,
                self.thumbnail_orientation,
                thumbnail.as_deref(),
            )
        });
        write_jpeg_with_exif(path, &body, tiff.as_deref());
    }
}

// 旧ビルドの行は fingerprint が NULL で、そのままではハッシュキャッシュが
// 一切効かない。補完のついでに解析結果を壊してはいけない。
#[test]
fn backfills_missing_fingerprints_without_discarding_analysis() {
    let directory = test_directory("backfill");
    let photo = directory.join("photo.jpg");
    fs::write(&photo, b"stand-in for a real photo").expect("write photo");
    let database = directory.join("legacy.sqlite3");

    {
        let conn = open_database(&database).expect("open database");
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES ('photo-1','project-1',?1,'photo.jpg','photo.jpg',1234567890,'aabbccddeeff0011',3,NULL,NULL,0)",
            params![photo.to_string_lossy().to_string()],
        )
        .expect("insert legacy row");
    }

    // 再接続のたびに backfill が走る。
    let conn = open_database(&database).expect("reopen database");
    let (captured_at, hash, rating, mtime, size): (
        Option<i64>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size FROM photos WHERE id='photo-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("read migrated row");

    assert_eq!(captured_at, Some(1_234_567_890), "captured_at を失わない");
    assert_eq!(
        hash.as_deref(),
        Some("aabbccddeeff0011"),
        "d_hash を失わない"
    );
    assert_eq!(rating, 3, "rating を失わない");
    assert!(mtime.is_some(), "fingerprint_mtime が補完される");
    assert_eq!(
        size,
        Some(fs::metadata(&photo).expect("photo metadata").len() as i64),
        "fingerprint_size が実ファイルと一致する"
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 接続の使い回し（U27 R1）: 初回だけ整備し、2 回目以降は整備なしで同じデータが見える。
// Amazon の行は backfill の対象外（fingerprint が NULL のまま、stat も走らない）。
#[test]
fn missing_ratings_lists_only_missing_rows_with_stars() {
    let directory = test_directory("missing-ratings");
    let conn = open_connection(&directory.join("m.sqlite3")).expect("open");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,created_at,updated_at,source_kind) VALUES ('p','p','/x',1,1,'folder')",
        [],
    )
    .unwrap();
    for (id, path, rating, missing) in [("a", "a.jpg", 2, 1), ("b", "b.jpg", 0, 1), ("c", "c.jpg", 3, 0)] {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,rating,is_missing) VALUES (?1,'p',?2,?2,?2,?3,?4)",
            params![id, path, rating, missing],
        )
        .unwrap();
    }
    assert_eq!(
        missing_ratings(&conn, "p").unwrap(),
        vec![MissingRating { relative_path: "a.jpg".into(), rating: 2 }]
    );
    assert!(missing_ratings(&conn, "other").unwrap().is_empty());
}

#[test]
fn open_connection_migrates_once_and_keeps_data_visible() {
    let directory = test_directory("open-connection");
    let database = directory.join("once.sqlite3");
    {
        let conn = open_connection(&database).expect("first open");
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,created_at,updated_at,source_kind) VALUES ('amz','a','https://x',1,1,'amazon')",
            [],
        )
        .expect("insert project");
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,rating,is_missing) VALUES ('p1','amz','node-1','node-1','n',4,0)",
            [],
        )
        .expect("insert photo");
    }
    // 整備済みなら、整備の跡（索引）を消しても 2 回目は作り直さない。
    {
        let conn = open_connection(&database).expect("second open");
        conn.execute_batch("DROP INDEX photos_project_visible;")
            .expect("drop index");
    }
    let conn = open_connection(&database).expect("third open");
    let indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='photos_project_visible'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(indexes, 0, "2 回目以降は整備しない");
    let (rating, mtime): (i64, Option<i64>) = conn
        .query_row(
            "SELECT rating,fingerprint_mtime FROM photos WHERE id='p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");
    assert_eq!((rating, mtime), (4, None), "データはそのまま見える");
    // 整備し直す経路（open_database）は従来どおり索引を作る。
    drop(conn);
    let conn = open_database(&database).expect("full migrate");
    let indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='photos_project_visible'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(indexes, 1);
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 以前は fingerprint の比較に `=` を使っていたため、NULL が絡むと常に偽と
// なり、再 scan のたびに captured_at と d_hash が消えていた。
#[test]
fn rescanning_an_unchanged_photo_keeps_its_analysis() {
    let directory = test_directory("rescan");
    let photo = directory.join("photo.jpg");
    fs::write(&photo, b"original contents").expect("write photo");
    let database = directory.join("scan.sqlite3");
    let conn = open_database(&database).expect("open database");
    let absolute = photo.to_string_lossy().to_string();
    let (mtime, size) = fingerprint(&photo).expect("fingerprint");

    let scan = |mtime: Option<i64>, size: Option<i64>| {
        upsert_photo(
            &conn,
            "project-1",
            &absolute,
            "photo.jpg",
            "photo.jpg",
            mtime,
            size,
        )
        .expect("scan photo");
    };
    let analysis = || -> (Option<i64>, Option<String>) {
        conn.query_row(
            "SELECT captured_at,d_hash FROM photos WHERE path=?1",
            params![absolute],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read analysis")
    };

    scan(Some(mtime), Some(size));
    conn.execute(
        "UPDATE photos SET captured_at=111,d_hash='0f0f0f0f0f0f0f0f' WHERE path=?1",
        params![absolute],
    )
    .expect("record analysis");

    // 中身が変わっていない写真の解析結果は保持される。
    scan(Some(mtime), Some(size));
    let (captured_at, hash) = analysis();
    assert_eq!(captured_at, Some(111), "再 scan で captured_at を失わない");
    assert_eq!(
        hash.as_deref(),
        Some("0f0f0f0f0f0f0f0f"),
        "再 scan で d_hash を失わない"
    );

    // ファイルが変わったときは解析結果を破棄する。
    scan(Some(mtime), Some(size + 1));
    let (captured_at, hash) = analysis();
    assert_eq!(captured_at, None, "内容が変われば captured_at を破棄する");
    assert_eq!(hash, None, "内容が変われば d_hash を破棄する");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// metadata が読めない写真は fingerprint が両方 NULL になる。SQL の `=` は
// `NULL = NULL` すら真にしないため、以前はこの手の写真の解析結果が再 scan の
// たびに破棄されていた。`IS` に変えたことを直接検証する。
#[test]
fn rescanning_a_photo_without_readable_metadata_keeps_its_analysis() {
    let directory = test_directory("rescan-null");
    let database = directory.join("scan.sqlite3");
    let conn = open_database(&database).expect("open database");
    let absolute = "C:/photos/unreadable.jpg";

    upsert_photo(
        &conn,
        "project-1",
        absolute,
        "unreadable.jpg",
        "unreadable.jpg",
        None,
        None,
    )
    .expect("first scan");
    conn.execute(
        "UPDATE photos SET captured_at=222,d_hash='1122334455667788' WHERE path=?1",
        params![absolute],
    )
    .expect("record analysis");

    upsert_photo(
        &conn,
        "project-1",
        absolute,
        "unreadable.jpg",
        "unreadable.jpg",
        None,
        None,
    )
    .expect("second scan");

    let (captured_at, hash): (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT captured_at,d_hash FROM photos WHERE path=?1",
            params![absolute],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read analysis");
    assert_eq!(captured_at, Some(222), "NULL 同士でも captured_at を保つ");
    assert_eq!(
        hash.as_deref(),
        Some("1122334455667788"),
        "NULL 同士でも d_hash を保つ"
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 解析全体をひとつのトランザクションで囲んでいた頃は、キャンセルすると
// すべて rollback され、何度やり直しても d_hash が1件も残らなかった。
#[test]
fn cancelling_keeps_the_chunks_that_were_already_committed() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let directory = test_directory("chunks");
    let database = directory.join("chunks.sqlite3");
    let conn = open_database(&database).expect("open database");

    let total = ANALYSIS_CHUNK_SIZE * 2 + 50;
    let ids: Vec<String> = (0..total).map(|index| format!("photo-{index}")).collect();
    for (index, id) in ids.iter().enumerate() {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES (?1,'project-1',?2,?3,?3,NULL,NULL,0,NULL,NULL,0)",
            params![id, format!("C:/photos/{index}.jpg"), format!("{index}.jpg")],
        )
        .expect("insert photo");
    }

    // チャンク境界(100)を越えた 150 件目の直前でキャンセルする。
    let cancel_after = ANALYSIS_CHUNK_SIZE + 50;
    let checks = AtomicUsize::new(0);
    let outcome = commit_in_chunks(
        &conn,
        &ids,
        &|| checks.fetch_add(1, Ordering::SeqCst) >= cancel_after,
        &mut |tx: &Connection, id: &String, _index: usize| {
            tx.execute(
                "UPDATE photos SET d_hash='ffffffffffffffff' WHERE id=?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        },
    )
    .expect("run chunked processing");

    assert!(outcome.cancelled, "キャンセルが伝わる");
    assert_eq!(
        outcome.committed, ANALYSIS_CHUNK_SIZE,
        "確定済みのチャンクだけが残る"
    );

    let stored: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count hashed photos");
    assert_eq!(
        stored, ANALYSIS_CHUNK_SIZE as i64,
        "commit 済みチャンクの成果は残り、処理中チャンクだけが破棄される"
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// -----------------------------------------------------------------------
// Step 5: EXIF 撮影時刻の解釈
// -----------------------------------------------------------------------

// 基準値は `date -u -d "..." +%s` で外部から取ったもの。
const JUNE_30_2026_UTC_MS: i64 = 1_782_843_572_000; // 2026-06-30 18:19:32Z
const FEB_29_2024_UTC_MS: i64 = 1_709_208_000_000; // 2024-02-29 12:00:00Z
const JAN_15_2026_UTC_MS: i64 = 1_768_435_200_000; // 2026-01-15 00:00:00Z

#[test]
fn parses_representative_exif_timestamps() {
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32", None),
        Some(JUNE_30_2026_UTC_MS),
        "代表フォーマットを UTC の実値として解釈する"
    );
    // 旧実装は 1970 年以降のすべての日付が1日ぶん手前にずれていた
    // （`-(year-1901)/100` が 2100 年以外でも 1 を引いていたため）。
    assert_eq!(
        exif_timestamp_ms(b"2026:01:15 00:00:00", None),
        Some(JAN_15_2026_UTC_MS),
        "年始の日付でも1日ずれない"
    );
    assert_eq!(
        exif_timestamp_ms(b"2024:02:29 12:00:00", None),
        Some(FEB_29_2024_UTC_MS),
        "うるう日を正しく扱う"
    );
    // サブ秒つき（19バイトより長い）も先頭19バイトで解釈できる。
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32.500", None),
        Some(JUNE_30_2026_UTC_MS)
    );
}

#[test]
fn applies_the_exif_time_zone_offset() {
    let naive = exif_timestamp_ms(b"2026:06:30 18:19:32", None).expect("naive");
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"+09:00")),
        Some(naive - 9 * 3_600_000),
        "JST は UTC より9時間進んでいる"
    );
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"-05:00")),
        Some(naive + 5 * 3_600_000),
        "負のオフセットも符号どおりに効く"
    );
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"      ")),
        Some(naive),
        "空欄のオフセットは無視して日時だけ使う"
    );
    assert_eq!(
        exif_timestamp_ms(b"2026:06:30 18:19:32", Some(b"junk!!")),
        Some(naive),
        "壊れたオフセットで日時まで捨てない"
    );
}

#[test]
fn rejects_broken_exif_timestamps() {
    // 旧実装は数字を拾うだけで範囲を見なかったため、これらが
    // 「それらしい値」に化けるか、黙って mtime fallback を誘発していた。
    for broken in [
        &b"2026:13:30 18:19:32"[..], // 13月
        &b"2026:02:30 18:19:32"[..], // 2月30日
        &b"2025:02:29 18:19:32"[..], // 平年の2月29日
        &b"2026:06:00 18:19:32"[..], // 0日
        &b"2026:06:30 25:19:32"[..], // 25時
        &b"2026:06:30 18:60:32"[..], // 60分
        &b"1899:06:30 18:19:32"[..], // 範囲外の年
        &b"    :  :     :  :  "[..], // 空欄
        &b"2026-06-30 18:19:32"[..], // 区切りが規格外
        &b"garbage"[..],             // 論外
        &b""[..],
    ] {
        assert_eq!(
            exif_timestamp_ms(broken, None),
            None,
            "壊れた値を受け入れてしまった: {:?}",
            String::from_utf8_lossy(broken)
        );
    }
    // うるう秒を書く機材があるので 60 秒だけは通す。
    assert!(exif_timestamp_ms(b"2026:06:30 18:19:60", None).is_some());
}

#[test]
fn infers_capture_time_from_common_filename_shapes() {
    let expect = Some(JUNE_30_2026_UTC_MS);
    for name in [
        "2026-06-30_18-19-32.jpg",
        "20260630_181932.jpg",
        "IMG_20260630_181932.jpg",
        "20260630181932.jpg",
        "2026-06-30 18.19.32.jpg",
        "photo_2026-06-30_18-19-32_1.jpg",
    ] {
        assert_eq!(
            filename_capture_time(Path::new(name)),
            expect,
            "ファイル名から読めなかった: {name}"
        );
    }
    for name in [
        "DSC_0001.jpg",
        "photo.jpg",
        "2026.jpg",
        "9999-99-99_99-99-99.jpg", // 数字は並んでいるが暦として不正
        "IMG_20261340_181932.jpg", // 13月40日
    ] {
        assert_eq!(
            filename_capture_time(Path::new(name)),
            None,
            "撮影時刻でないものを読んでしまった: {name}"
        );
    }
}

// -----------------------------------------------------------------------
// Step 5: timestamp_source の分類
// -----------------------------------------------------------------------

#[test]
fn classifies_the_source_of_every_capture_time() {
    let directory = test_directory("timestamp-source");

    let original = directory.join("original.jpg");
    Fixture {
        datetime_original: Some("2026:06:30 18:19:32".into()),
        offset_original: Some("+09:00".into()),
        ..Default::default()
    }
    .write(&original);
    let capture = read_capture_time(&original).expect("read original");
    assert_eq!(capture.source, TimestampSource::ExifOriginal);
    assert_eq!(capture.at, JUNE_30_2026_UTC_MS - 9 * 3_600_000);

    // DateTimeOriginal が無く DateTime だけの写真は経路が変わる。
    let fallback = directory.join("datetime-only.jpg");
    Fixture {
        datetime: Some("2026:06:30 18:19:32".into()),
        ..Default::default()
    }
    .write(&fallback);
    let capture = read_capture_time(&fallback).expect("read datetime-only");
    assert_eq!(capture.source, TimestampSource::ExifDateTime);
    assert_eq!(capture.at, JUNE_30_2026_UTC_MS);

    // EXIF が無くてもファイル名が残っていれば mtime よりは強い。
    let named = directory.join("2026-06-30_18-19-32.jpg");
    Fixture::default().write(&named);
    let capture = read_capture_time(&named).expect("read named");
    assert_eq!(capture.source, TimestampSource::FilenameInferred);
    assert_eq!(capture.at, JUNE_30_2026_UTC_MS);

    // 何の手がかりも無ければ最後の手段として mtime。
    let bare = directory.join("DSC_0001.jpg");
    Fixture::default().write(&bare);
    let capture = read_capture_time(&bare).expect("read bare");
    assert_eq!(capture.source, TimestampSource::FilesystemMtime);
    assert_eq!(capture.at, fingerprint(&bare).expect("fingerprint").0);

    // 壊れた EXIF は「読めなかった」として次の経路へ落ちる。
    let broken = directory.join("broken-exif.jpg");
    Fixture {
        datetime_original: Some("2026:13:40 99:99:99".into()),
        ..Default::default()
    }
    .write(&broken);
    assert_eq!(
        read_capture_time(&broken).expect("read broken").source,
        TimestampSource::FilesystemMtime
    );

    assert!(
        read_capture_time(&directory.join("missing.jpg")).is_none(),
        "存在しないファイルでは何も返さない"
    );

    // 保存した文字列がそのまま読み戻せる（DB 往復の形）。
    for source in [
        TimestampSource::ExifOriginal,
        TimestampSource::ExifDateTime,
        TimestampSource::FilenameInferred,
        TimestampSource::FilesystemMtime,
        TimestampSource::Unknown,
    ] {
        assert_eq!(TimestampSource::parse(Some(source.as_str())), source);
    }
    assert_eq!(TimestampSource::parse(None), TimestampSource::Unknown);
    assert_eq!(
        TimestampSource::parse(Some("将来の値")),
        TimestampSource::Unknown
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// -----------------------------------------------------------------------
// Step 5: 候補爆発の抑制
// -----------------------------------------------------------------------

fn candidates(entries: &[(i64, TimestampSource)]) -> Vec<CandidateInput> {
    entries
        .iter()
        .enumerate()
        .map(|(index, (captured_at, source))| CandidateInput {
            id: format!("photo-{index}"),
            captured_at: *captured_at,
            source: *source,
        })
        .collect()
}

// コピーやダウンロードで数千枚が同一 mtime になるのは普通のことで、
// それを連写の根拠にすると全件が候補になり、全件デコードが走る。
#[test]
fn an_mtime_flood_does_not_explode_the_candidate_set() {
    let flood = candidates(
        &(0..500)
            .map(|_| (1_700_000_000_000, TimestampSource::FilesystemMtime))
            .collect::<Vec<_>>(),
    );
    let selection = select_burst_candidates(&flood);
    assert!(
        selection.ids.is_empty(),
        "mtime 由来だけで 500 枚が候補になった: {}",
        selection.ids.len()
    );
    assert_eq!(selection.weak_pairs_skipped, 499);
    assert!(!selection.narrowed, "除外で足りるので窓を狭める必要はない");

    // 経路が不明な行も同じ扱い。旧ビルドの行が一斉に候補化しない。
    let unknown = candidates(
        &(0..500)
            .map(|index| (1_700_000_000_000 + index * 10, TimestampSource::Unknown))
            .collect::<Vec<_>>(),
    );
    assert!(select_burst_candidates(&unknown).ids.is_empty());

    // 片側が EXIF なら根拠として成立するので候補に残す。
    let mixed = candidates(&[
        (0, TimestampSource::ExifOriginal),
        (1_000, TimestampSource::FilesystemMtime),
        (50_000, TimestampSource::FilesystemMtime),
        (50_100, TimestampSource::FilesystemMtime),
    ]);
    let selection = select_burst_candidates(&mixed);
    assert_eq!(
        selection.ids,
        HashSet::from(["photo-0".to_owned(), "photo-1".to_owned()]),
        "EXIF と隣り合う mtime だけが候補になる"
    );
}

#[test]
fn narrows_the_window_only_when_the_candidate_set_is_actually_expensive() {
    // 3秒間隔（4秒窓では全部が隣接）を、実コストが問題になる規模まで並べる。
    let bulk = CANDIDATE_COUNT_LIMIT + 400;
    let mut entries: Vec<(i64, TimestampSource)> = (0..bulk as i64)
        .map(|index| (index * 3_000, TimestampSource::ExifOriginal))
        .collect();
    // 本物の連写。窓をどこまで詰めても残るべき2枚。
    let far = (bulk as i64 + 100) * 3_000;
    entries.push((far, TimestampSource::ExifOriginal));
    entries.push((far + 100, TimestampSource::ExifOriginal));
    let records = candidates(&entries);

    let wide = candidates_within(&records, BURST_WINDOW_MS).0;
    assert_eq!(wide.len(), bulk + 2, "4秒窓では全枚数が候補になる（前提）");

    let selection = select_burst_candidates(&records);
    assert!(selection.narrowed, "候補が {bulk} 件でも窓が狭まらなかった");
    assert!(selection.window_ms < BURST_WINDOW_MS);
    assert_eq!(
        selection.ids,
        HashSet::from([format!("photo-{bulk}"), format!("photo-{}", bulk + 1)]),
        "本当に近い2枚だけが残る"
    );
}

#[test]
fn a_dense_small_project_keeps_its_full_window() {
    // Routine 2 の直接的な回帰テスト。実データ271枚は撮影間隔がほぼ全て
    // 1〜4秒で候補率が 98.5% になる。比率だけで縮小を判断していた頃は
    // ここで発火し、連写グループを 52 → 16 に減らしていた。
    // 数百枚ぶんのデコードは Step 4 後で 1秒未満。窓を詰める理由が無い。
    let records = candidates(
        &(0..271)
            .map(|index| (index * 2_000, TimestampSource::ExifOriginal))
            .collect::<Vec<_>>(),
    );
    let selection = select_burst_candidates(&records);
    assert!(
        selection.ratio > CANDIDATE_RATIO_LIMIT,
        "候補率が高い状況であることの確認: {}",
        selection.ratio
    );
    assert!(
        !selection.narrowed,
        "密に撮影された数百枚で窓が狭まってしまった"
    );
    assert_eq!(selection.window_ms, BURST_WINDOW_MS);
    assert_eq!(selection.ids.len(), 271, "連写候補が1枚も失われない");
}

#[test]
fn window_narrowing_stops_at_the_floor() {
    // 全枚数が 0.1 秒間隔。どこまで詰めても候補率も件数も下がらない。
    let total = CANDIDATE_COUNT_LIMIT + 400;
    let records = candidates(
        &(0..total as i64)
            .map(|index| (index * 100, TimestampSource::ExifOriginal))
            .collect::<Vec<_>>(),
    );
    let selection = select_burst_candidates(&records);
    assert_eq!(
        selection.window_ms, MIN_BURST_WINDOW_MS,
        "下限で止まる（無限に詰め続けない）"
    );
    assert!(selection.narrowed);
    assert_eq!(selection.ids.len(), total, "本物の連写は候補のまま残る");
}

#[test]
fn an_empty_or_single_photo_project_selects_nothing() {
    assert!(select_burst_candidates(&[]).ids.is_empty());
    let single = candidates(&[(0, TimestampSource::ExifOriginal)]);
    let selection = select_burst_candidates(&single);
    assert!(selection.ids.is_empty());
    assert!(!selection.narrowed);
}

// -----------------------------------------------------------------------
// Step 4: デコード経路とサムネイルキャッシュ
// -----------------------------------------------------------------------

#[test]
fn picks_the_cheapest_decode_path_that_works() {
    let directory = test_directory("decode-path");

    // ① EXIF サムネイルがあればそれで済ませる。
    let with_thumbnail = directory.join("with-thumbnail.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        ..Default::default()
    }
    .write(&with_thumbnail);
    assert_eq!(
        decode_hash_source(&with_thumbnail).map(|(_, source)| source),
        Some(DecodeSource::ExifThumbnail)
    );

    // ② サムネイルが無い JPEG は 1/8 デコードに落ちる。
    let plain = directory.join("plain.jpg");
    Fixture::default().write(&plain);
    assert_eq!(
        decode_hash_source(&plain).map(|(_, source)| source),
        Some(DecodeSource::JpegScaled)
    );

    // ③ 小さすぎるサムネイルは使わず、次の経路へ落とす。
    let tiny = directory.join("tiny-thumbnail.jpg");
    Fixture {
        thumbnail: Some((32, 24)),
        ..Default::default()
    }
    .write(&tiny);
    assert_eq!(
        decode_hash_source(&tiny).map(|(_, source)| source),
        Some(DecodeSource::JpegScaled)
    );

    // ④ JPEG でなければフルデコードしかない。
    let png = directory.join("plain.png");
    synthetic_image(300, 200, 3).save(&png).expect("write png");
    assert_eq!(
        decode_hash_source(&png).map(|(_, source)| source),
        Some(DecodeSource::FullDecode)
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// -----------------------------------------------------------------------
// Routine 10: 解析の入口をバイト列にする
// -----------------------------------------------------------------------

/// 読んだ量を数える `PhotoSource`。**部分読みが効いているか**を測るためだけの実装。
struct CountingSource {
    bytes: Vec<u8>,
    name: String,
    served: std::cell::Cell<usize>,
    all_calls: std::cell::Cell<usize>,
    range_calls: std::cell::Cell<usize>,
}

impl CountingSource {
    fn new(bytes: Vec<u8>, name: &str) -> Self {
        Self {
            bytes,
            name: name.to_owned(),
            served: std::cell::Cell::new(0),
            all_calls: std::cell::Cell::new(0),
            range_calls: std::cell::Cell::new(0),
        }
    }
}

impl PhotoSource for CountingSource {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        let end = want.min(self.bytes.len());
        self.served.set(self.served.get() + end);
        Some(self.bytes[..end].to_vec())
    }
    fn all(&self) -> Option<Vec<u8>> {
        self.all_calls.set(self.all_calls.get() + 1);
        self.served.set(self.served.get() + self.bytes.len());
        Some(self.bytes.clone())
    }
    fn read_range(&self, offset: u64, length: usize) -> Option<Vec<u8>> {
        let start = usize::try_from(offset).ok()?;
        if start >= self.bytes.len() {
            return None;
        }
        let end = start.saturating_add(length).min(self.bytes.len());
        self.range_calls.set(self.range_calls.get() + 1);
        self.served.set(self.served.get() + (end - start));
        Some(self.bytes[start..end].to_vec())
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        Some((1_700_000_000_000, self.bytes.len() as i64))
    }
    fn name(&self) -> Option<String> {
        Some(self.name.clone())
    }
}

/// **これが Android で NAS を読めるかどうかの分かれ目。**
/// EXIF サムネイルがある写真は、原本の全体を一度も要求してはいけない。
#[test]
fn the_exif_thumbnail_path_never_asks_for_the_whole_file() {
    let directory = test_directory("partial-read");

    let with_thumbnail = directory.join("with-thumbnail.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        size: (4000, 3000),
        ..Default::default()
    }
    .write(&with_thumbnail);
    let bytes = fs::read(&with_thumbnail).expect("read fixture");
    let total = bytes.len();
    let source = CountingSource::new(bytes, "with-thumbnail.jpg");

    let (_, decode) = decode_hash_source_from(&source).expect("decode");
    assert_eq!(decode, DecodeSource::ExifThumbnail);
    assert_eq!(
        source.all_calls.get(),
        0,
        "EXIF サムネイルで済むのに全体を読んでいる"
    );
    assert!(
        source.served.get() <= EXIF_HEAD_PROBE,
        "先頭 {EXIF_HEAD_PROBE} バイトを超えて読んでいる（{} / 全体 {total}）",
        source.served.get()
    );

    // 逆に、EXIF サムネイルが無ければ全体が要る。ここを読まないと画像にならない。
    let plain = directory.join("plain.jpg");
    Fixture {
        size: (4000, 3000),
        ..Default::default()
    }
    .write(&plain);
    let plain_source = CountingSource::new(fs::read(&plain).expect("read"), "plain.jpg");
    let (_, decode) = decode_hash_source_from(&plain_source).expect("decode");
    assert_eq!(decode, DecodeSource::JpegScaled);
    assert_eq!(plain_source.all_calls.get(), 1, "全体を 1 回だけ読む");

    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// EXIF が無いときに、`PhotoSource::name()` が撮影時刻の手がかりになること。
/// Android の `content://` はパスを持たないので、名前を別途もらう必要がある。
#[test]
fn the_capture_time_falls_back_to_the_name_then_the_fingerprint() {
    let bytes = jpeg_bytes(64, 48, 0);

    let named = CountingSource::new(bytes.clone(), "2026-06-30_18-19-32.jpg");
    let capture = read_capture_time_from(&named).expect("capture time");
    assert_eq!(capture.source, TimestampSource::FilenameInferred);

    // 名前も手がかりにならなければ fingerprint（mtime）へ落ちる。
    let plain = CountingSource::new(bytes, "photo.jpg");
    let capture = read_capture_time_from(&plain).expect("capture time");
    assert_eq!(capture.source, TimestampSource::FilesystemMtime);
    assert_eq!(capture.at, 1_700_000_000_000);
}

// -----------------------------------------------------------------------
// Routine 10b: 表示用サイズ
// -----------------------------------------------------------------------

#[test]
fn the_display_edge_is_clamped_to_the_offered_choices() {
    for edge in DISPLAY_EDGES {
        assert_eq!(normalize_display_edge(edge as i64), edge);
    }
    // 表に無い値・負・極端な値は既定へ。生成する画像が暴れないように。
    for broken in [0, -1, 999, 4096, i64::MAX] {
        assert_eq!(normalize_display_edge(broken), DISPLAY_EDGE_DEFAULT);
    }
}

/// **下げるときに原本を読み直さないこと。**
/// 1536 → 1024 の切り替えで 2,000 枚ぶん 13.4GB を読むのは無駄でしかない。
#[test]
fn shrinking_the_display_size_reuses_the_saved_image() {
    let directory = test_directory("display-size");
    let photo = directory.join("photo.jpg");
    Fixture {
        size: (4000, 3000),
        ..Default::default()
    }
    .write(&photo);

    // まず大きい方を作る。ここは原本を読む。
    let source = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
    let large = build_display(&source, 1536, None).expect("build large");
    assert_eq!(source.all_calls.get(), 1, "初回は原本が要る");
    let stored = directory.join("display-1536.jpg");
    fs::write(&stored, &large).expect("write display");

    // 次に小さい方へ。**保存済みを縮めるだけで、原本には戻らない。**
    let shrink = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
    let small = build_display(&shrink, 1024, Some((&stored, 1536))).expect("build small");
    assert_eq!(
        shrink.all_calls.get(),
        0,
        "下げるだけなのに原本を読み直している"
    );
    let decoded = image::load_from_memory(&small).expect("decode");
    assert_eq!(decoded.width().max(decoded.height()), 1024);

    // 逆に上げるときは、小さい画像から大きい画像は作れないので原本へ戻る。
    let grow = CountingSource::new(fs::read(&photo).expect("read"), "photo.jpg");
    let bigger = build_display(&grow, 1536, Some((&stored, 1024))).expect("build bigger");
    assert_eq!(grow.all_calls.get(), 1, "上げるときは原本が要る");
    let decoded = image::load_from_memory(&bigger).expect("decode");
    assert_eq!(decoded.width().max(decoded.height()), 1536);

    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// 表示用画像を run_in_parallel で作っても、全行に 1 回ずつ書かれ、並列数で結果が変わらないこと
/// （U27 R2）。時間は `--nocapture` で見られる（debug ビルドの目安）。
#[test]
fn parallel_display_generation_writes_every_row_exactly_once() {
    let directory = test_directory("display-parallel");
    let total = 10usize;
    let originals = directory.join("originals");
    fs::create_dir_all(&originals).expect("create originals");
    let mut jobs = Vec::new();
    for index in 0..total {
        let photo = originals.join(format!("{index}.jpg"));
        Fixture {
            size: (2000, 1333),
            ..Default::default()
        }
        .write(&photo);
        jobs.push(DisplayJob {
            id: format!("photo-{index}"),
            path: photo.to_string_lossy().to_string(),
            stored_path: None,
            stored_edge: None,
        });
    }
    // 読めない 1 枚は、失敗として確定して先へ進む。
    jobs.push(DisplayJob {
        id: "photo-broken".into(),
        path: originals.join("missing.jpg").to_string_lossy().to_string(),
        stored_path: None,
        stored_edge: None,
    });
    let expected = jobs.len();

    let mut timings = Vec::new();
    for workers in [1usize, 4] {
        let display = directory.join(format!("display-{workers}"));
        fs::create_dir_all(&display).expect("create display dir");
        let conn = open_database(&directory.join(format!("w{workers}.sqlite3"))).expect("open");
        for job in &jobs {
            conn.execute(
                "INSERT INTO photos (id,project_id,path,relative_path,name) VALUES (?1,'p',?2,?2,'n')",
                params![job.id, job.path],
            )
            .expect("insert photo");
        }
        let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
            if item.error.is_none() {
                tx.execute(
                    "UPDATE photos SET display_path=?1, display_edge=1024 WHERE id=?2",
                    params![
                        display_file(&display, &item.photo_id).to_string_lossy().to_string(),
                        item.photo_id
                    ],
                )
            } else {
                tx.execute(
                    "UPDATE photos SET display_path=NULL, display_edge=NULL WHERE id=?1",
                    params![item.photo_id],
                )
            }
            .map_err(|error| error.to_string())?;
            Ok(())
        };
        let mut pending: Vec<PhotoWork> = Vec::new();
        let mut committed = 0usize;
        let display_for_workers = display.clone();
        let started = Instant::now();
        let outcome = run_in_parallel(
            Arc::new(jobs.clone()),
            workers,
            Duration::from_secs(60),
            &|| false,
            move |index, job: &DisplayJob| display_one(&display_for_workers, 1024, index, job),
            &mut |item| {
                pending.push(item);
                if pending.len() >= ANALYSIS_CHUNK_SIZE {
                    committed += flush_results(&conn, &mut pending, &apply)?;
                }
                Ok(())
            },
        )
        .expect("run workers");
        committed += flush_results(&conn, &mut pending, &apply).expect("final flush");
        timings.push((workers, started.elapsed()));

        assert_eq!(outcome.completed, expected);
        assert_eq!(committed, expected, "全件が確定する");
        let with_image: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE display_path IS NOT NULL AND display_edge=1024",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(with_image as usize, total, "作れた全行に書かれ、壊れた 1 枚には印が無い");
        for index in 0..total {
            let bytes = fs::read(display_file(&display, &format!("photo-{index}"))).expect("read display");
            let decoded = image::load_from_memory(&bytes).expect("decode display");
            assert_eq!(decoded.width().max(decoded.height()), 1024);
        }
        assert!(!display_file(&display, "photo-broken").exists());
    }
    eprintln!("表示用画像 {} 枚（3000x2000）: {:?}", total, timings);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// 原本より大きくは引き伸ばさない。情報は増えないのに容量だけ増える。
#[test]
fn the_display_image_never_upscales_the_original() {
    let small = synthetic_image(320, 240, 7);
    let bytes = encode_display(&small, 1536).expect("encode");
    let decoded = image::load_from_memory(&bytes).expect("decode");
    assert_eq!((decoded.width(), decoded.height()), (320, 240));
}

// -----------------------------------------------------------------------
// Routine 5: EXIF Orientation
// -----------------------------------------------------------------------

/// 画素を行ごとに取り出す。回転や反転の結果を並びそのままで比べる。
fn grid(image: &DynamicImage) -> Vec<Vec<u8>> {
    (0..image.height())
        .map(|y| {
            (0..image.width())
                .map(|x| image.get_pixel(x, y).0[0])
                .collect()
        })
        .collect()
}

/// 3x2 の非対称な画像。値はすべて異なるので、取り違えれば必ず落ちる。
///
/// ```text
/// 1 2 3
/// 4 5 6
/// ```
fn asymmetric() -> DynamicImage {
    let mut image = image::RgbImage::new(3, 2);
    for y in 0..2u32 {
        for x in 0..3u32 {
            let value = (1 + x + y * 3) as u8;
            image.put_pixel(x, y, image::Rgb([value, value, value]));
        }
    }
    DynamicImage::ImageRgb8(image)
}

#[test]
fn every_exif_orientation_maps_to_its_own_transform() {
    let expected: [(u16, Vec<Vec<u8>>); 8] = [
        (1, vec![vec![1, 2, 3], vec![4, 5, 6]]),
        (2, vec![vec![3, 2, 1], vec![6, 5, 4]]),
        (3, vec![vec![6, 5, 4], vec![3, 2, 1]]),
        (4, vec![vec![4, 5, 6], vec![1, 2, 3]]),
        (5, vec![vec![1, 4], vec![2, 5], vec![3, 6]]),
        (6, vec![vec![4, 1], vec![5, 2], vec![6, 3]]),
        (7, vec![vec![6, 3], vec![5, 2], vec![4, 1]]),
        (8, vec![vec![3, 6], vec![2, 5], vec![1, 4]]),
    ];
    for (orientation, want) in expected {
        assert_eq!(
            grid(&apply_orientation(asymmetric(), orientation)),
            want,
            "Orientation {orientation} の変換が違う"
        );
    }

    // 0 と 9 は規格外。壊れた EXIF で画像を回さない。
    for broken in [0u16, 9, 65535] {
        assert_eq!(
            grid(&apply_orientation(asymmetric(), broken)),
            grid(&asymmetric()),
            "規格外の値 {broken} で画像を回してしまった"
        );
    }
}

// 一覧が横倒しになっていた原因そのもの。サムネイルの生成経路で
// Orientation を焼き込まないと、原本を直接見る選別画面とだけ向きがずれる。
#[test]
fn the_thumbnail_source_comes_back_upright() {
    let directory = test_directory("orientation");

    // ① EXIF サムネイルが無い JPEG（1/8 デコード経路）。IFD0 の指定に従う。
    let rotated = directory.join("rotated.jpg");
    Fixture {
        size: (600, 400),
        orientation: Some(6),
        ..Default::default()
    }
    .write(&rotated);
    let (image, source) = decode_hash_source(&rotated).expect("decode");
    assert_eq!(source, DecodeSource::JpegScaled);
    assert!(
        image.height() > image.width(),
        "横長のまま返っている（{}x{}）",
        image.width(),
        image.height()
    );

    // ② 同じ写真から Orientation を外すと、回らない。①が「たまたま縦長」
    //    ではないことの裏取り。
    let upright = directory.join("upright.jpg");
    Fixture {
        size: (600, 400),
        datetime: Some("2026:06:30 18:19:32".into()),
        ..Default::default()
    }
    .write(&upright);
    let (image, _) = decode_hash_source(&upright).expect("decode");
    assert!(image.width() > image.height(), "回すべきでない画像を回した");

    // ③ EXIF サムネイル経路でも焼き込む。
    let with_thumbnail = directory.join("with-thumbnail.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        orientation: Some(6),
        ..Default::default()
    }
    .write(&with_thumbnail);
    let (image, source) = decode_hash_source(&with_thumbnail).expect("decode");
    assert_eq!(source, DecodeSource::ExifThumbnail);
    assert_eq!(
        (image.width(), image.height()),
        (120, 160),
        "埋め込みサムネイルに Orientation が効いていない"
    );

    // ④ 埋め込みサムネイルを既に正立させて保存するカメラ。IFD1 が 1 なので、
    //    IFD0 が 6 でも回してはいけない（回すと二重になる）。
    let pre_rotated = directory.join("pre-rotated-thumbnail.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        orientation: Some(6),
        thumbnail_orientation: Some(1),
        ..Default::default()
    }
    .write(&pre_rotated);
    let (image, source) = decode_hash_source(&pre_rotated).expect("decode");
    assert_eq!(source, DecodeSource::ExifThumbnail);
    assert_eq!(
        (image.width(), image.height()),
        (160, 120),
        "IFD1 の指定を無視して二重に回している"
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn reuses_the_cached_thumbnail_until_the_photo_changes() {
    let directory = test_directory("thumbnail-cache");
    let thumbnails = directory.join("thumbnails");
    let photo = directory.join("photo.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        ..Default::default()
    }
    .write(&photo);

    let run = |cached: &CachedAnalysis| {
        analyse_photo(&thumbnails, "photo-1", &photo, fingerprint(&photo), cached)
    };
    let store = |outcome: &AnalysisOutcome, photo: &Path| {
        let (mtime, size) = fingerprint(photo).expect("fingerprint");
        CachedAnalysis {
            d_hash: outcome.d_hash.clone(),
            d_hash_version: outcome.d_hash.as_ref().map(|_| D_HASH_VERSION),
            thumbnail_path: outcome.thumbnail_path.clone(),
            thumbnail_mtime: outcome.thumbnail_path.as_ref().map(|_| mtime),
            thumbnail_size: outcome.thumbnail_path.as_ref().map(|_| size),
            thumbnail_version: outcome.thumbnail_path.as_ref().map(|_| THUMBNAIL_VERSION),
        }
    };

    // --- ミス: 何も無い状態からは作る -----------------------------------
    let first = run(&CachedAnalysis::default());
    assert_eq!(
        first.thumbnail_state,
        ThumbnailState::Generated(DecodeSource::ExifThumbnail)
    );
    assert!(!first.hash_reused);
    let hash = first.d_hash.clone().expect("hash");
    let file = thumbnail_file(&thumbnails, "photo-1");
    assert!(file.is_file(), "サムネイルが保存されていない");
    assert_eq!(
        first.thumbnail_path.as_deref(),
        Some(&*file.to_string_lossy())
    );
    // dHash も表示も同じ1枚を使う。キャッシュ経路と単発計算が一致すること。
    assert_eq!(d_hash(&photo), Some(hash.clone()));

    // --- ヒット: 2回目はデコードしない -----------------------------------
    let cached = store(&first, &photo);
    let second = run(&cached);
    assert_eq!(second.thumbnail_state, ThumbnailState::Hit);
    assert!(second.hash_reused, "デコードし直している");
    assert_eq!(second.d_hash, Some(hash.clone()));

    // --- ハッシュ方式が変わったとき: 原本には戻らず、サムネイルから引き直す -
    let stale = CachedAnalysis {
        d_hash_version: Some(D_HASH_VERSION - 1),
        ..cached.clone()
    };
    let rehashed = run(&stale);
    assert_eq!(
        rehashed.thumbnail_state,
        ThumbnailState::Hit,
        "サムネイルは作り直さない"
    );
    assert!(!rehashed.hash_reused);
    assert_eq!(
        rehashed.d_hash,
        Some(hash.clone()),
        "同じサムネイルからは必ず同じハッシュが出る"
    );

    // --- 生成方式が変わったとき: 原本まで戻って作り直す --------------------
    // d_hash の版とは扱いが違う。あちらは保存済みのサムネイルから引き直せば
    // 足りるが、こちらは**サムネイルの中身そのもの**が古いので作り直す。
    let old_format = CachedAnalysis {
        thumbnail_version: Some(THUMBNAIL_VERSION - 1),
        ..cached.clone()
    };
    let rebuilt = run(&old_format);
    assert!(
        matches!(rebuilt.thumbnail_state, ThumbnailState::Generated(_)),
        "生成方式が変わったのに古いサムネイルを使い回している"
    );

    // --- 無効化: サムネイルのファイルが消えたら作り直す -------------------
    fs::remove_file(&file).expect("remove thumbnail");
    let regenerated = run(&cached);
    assert!(matches!(
        regenerated.thumbnail_state,
        ThumbnailState::Generated(_)
    ));
    assert_eq!(regenerated.d_hash, Some(hash.clone()));

    // --- 無効化: 写真そのものが差し替わったら fingerprint がずれる ---------
    Fixture {
        thumbnail: Some((160, 120)),
        seed: 90,
        size: (640, 480),
        ..Default::default()
    }
    .write(&photo);
    let changed = run(&cached);
    assert!(
        matches!(changed.thumbnail_state, ThumbnailState::Generated(_)),
        "写真が変わってもキャッシュを使い回してしまった"
    );
    assert_ne!(
        changed.d_hash,
        Some(hash),
        "別の写真なのに同じハッシュが出ている"
    );

    // --- fingerprint が読めないときはキャッシュを信用しない ---------------
    let unreadable = analyse_photo(&thumbnails, "photo-1", &photo, None, &cached);
    assert!(matches!(
        unreadable.thumbnail_state,
        ThumbnailState::Generated(_)
    ));

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 1枚の失敗で解析全体が止まると、フォルダに壊れたファイルが1つ混ざった
// だけでプロジェクトが使えなくなる。
#[test]
fn a_single_unreadable_photo_does_not_stop_the_others() {
    let directory = test_directory("decode-failures");
    let thumbnails = directory.join("thumbnails");

    let good = directory.join("good.jpg");
    Fixture {
        thumbnail: Some((160, 120)),
        ..Default::default()
    }
    .write(&good);

    // 拡張子は JPEG だが中身が壊れている。
    let truncated = directory.join("truncated.jpg");
    let mut bytes = jpeg_bytes(600, 400, 5);
    bytes.truncate(40);
    fs::write(&truncated, &bytes).expect("write truncated");

    // JPEG ですらない。
    let not_an_image = directory.join("notes.jpg");
    fs::write(&not_an_image, b"this is not an image at all").expect("write text");

    // 空ファイル。
    let empty = directory.join("empty.jpg");
    fs::write(&empty, b"").expect("write empty");

    // 存在しない・開けない（ディレクトリを写真として渡す）。
    let missing = directory.join("missing.jpg");
    let as_directory = directory.join("thumbnails");

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for path in [
        &good,
        &truncated,
        &not_an_image,
        &empty,
        &missing,
        &as_directory,
    ] {
        let outcome = analyse_photo(
            &thumbnails,
            &format!("id-{}", path.display()),
            path,
            fingerprint(path),
            &CachedAnalysis::default(),
        );
        match outcome.d_hash {
            Some(_) => succeeded += 1,
            None => {
                failed += 1;
                assert_eq!(outcome.thumbnail_state, ThumbnailState::Failed);
                assert!(outcome.thumbnail_path.is_none());
            }
        }
    }
    assert_eq!(succeeded, 1, "読める写真が処理されていない");
    assert_eq!(failed, 5, "読めない写真が成功扱いになっている");

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// -----------------------------------------------------------------------
// Step 6: 並列化・timeout・エラー耐性・キャンセル・task registry
// -----------------------------------------------------------------------

/// テスト用の仕事。`work` に渡す 1 件ぶん。
#[derive(Clone, Debug)]
struct FakeJob {
    id: String,
    /// この仕事が居座る時間。timeout の検証に使う。
    delay: Duration,
}

impl PhotoJob for FakeJob {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

fn fake_jobs(count: usize) -> Vec<FakeJob> {
    (0..count)
        .map(|index| FakeJob {
            id: format!("photo-{index}"),
            delay: Duration::ZERO,
        })
        .collect()
}

/// photos 行を count 件だけ用意した DB を作る。
fn database_with_photos(directory: &Path, count: usize) -> Connection {
    let conn = open_database(&directory.join("test.sqlite3")).expect("open database");
    for index in 0..count {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing)
                 VALUES (?1,'project-1',?2,?3,?3,NULL,NULL,0,NULL,NULL,0)",
            params![
                format!("photo-{index}"),
                format!("C:/photos/{index}.jpg"),
                format!("{index}.jpg")
            ],
        )
        .expect("insert photo");
    }
    conn
}

// Amazon の並列数（amazon::WORKERS = 8）が run_in_parallel で 4 に切られないこと（U27 R6）。
#[test]
fn run_in_parallel_honours_a_worker_count_above_the_local_cap() {
    let total = 32;
    let threads = Arc::new(Mutex::new(HashSet::new()));
    let seen = threads.clone();
    let mut received = 0usize;
    run_in_parallel(
        Arc::new(fake_jobs(total)),
        amazon::WORKERS,
        Duration::from_secs(30),
        &|| false,
        move |index, job: &FakeJob| {
            seen.lock().unwrap().insert(std::thread::current().id());
            std::thread::sleep(Duration::from_millis(30));
            PhotoWork::new(index, &job.id)
        },
        &mut |_item| {
            received += 1;
            Ok(())
        },
    )
    .expect("run workers");
    assert_eq!(received, total);
    assert!(
        threads.lock().unwrap().len() > MAX_ANALYSIS_WORKERS,
        "worker は {} 本まで使える",
        amazon::WORKERS
    );
}

// 並列に読んだ結果を単一の writer が書く構造が、重複も欠落も起こさないこと。
// ここが崩れると「解析したはずの写真が消える」「同じ写真が二重に数えられる」
// という、利用者からは再現できない壊れ方をする。
#[test]
fn parallel_workers_write_every_row_exactly_once() {
    let directory = test_directory("parallel-write");
    let total = ANALYSIS_CHUNK_SIZE * 3 + 37;
    let conn = database_with_photos(&directory, total);

    let mut pending: Vec<PhotoWork> = Vec::new();
    let mut committed = 0usize;
    let mut seen: Vec<String> = Vec::new();
    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        // d_hash に自分の index を入れる。取り違えがあれば後で分かる。
        tx.execute(
            "UPDATE photos SET d_hash=?1 WHERE id=?2",
            params![format!("{:016x}", item.index), item.photo_id],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    };

    let outcome = run_in_parallel(
        Arc::new(fake_jobs(total)),
        4,
        Duration::from_secs(30),
        &|| false,
        |index, job: &FakeJob| {
            // 一瞬で返る仕事だと worker が実際には重ならず、共有カーソルの
            // 競合が表に出ない。数十マイクロ秒だけ居座らせて、4本が本当に
            // 同時に次の番号を取りに来る状況を作る。
            let until = Instant::now() + Duration::from_micros(50);
            while Instant::now() < until {
                std::hint::spin_loop();
            }
            PhotoWork::new(index, &job.id)
        },
        &mut |item| {
            seen.push(item.photo_id.clone());
            pending.push(item);
            if pending.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending, &apply)?;
            }
            Ok(())
        },
    )
    .expect("run workers");
    committed += flush_results(&conn, &mut pending, &apply).expect("final flush");

    assert!(!outcome.cancelled);
    assert_eq!(outcome.completed, total, "全件が writer に届く");
    assert_eq!(outcome.timed_out, 0);
    assert_eq!(committed, total, "全件が確定する");

    let unique: HashSet<&String> = seen.iter().collect();
    assert_eq!(unique.len(), total, "同じ写真が二度 writer に届いている");

    // DB 側でも、全行が「自分の index」を持っていること。
    let rows: Vec<(String, Option<String>)> = {
        let mut statement = conn
            .prepare("SELECT id,d_hash FROM photos")
            .expect("prepare verification");
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query");
        rows.collect::<Result<Vec<_>, _>>().expect("collect")
    };
    assert_eq!(rows.len(), total);
    for (id, hash) in rows {
        let index: usize = id.trim_start_matches("photo-").parse().expect("parse id");
        assert_eq!(
            hash,
            Some(format!("{index:016x}")),
            "{id} の結果が別の写真のものになっている"
        );
    }

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 異常に遅い1枚で全体が停滞しないこと。Rust では走っているデコードを
// 安全に中断できないので、遅い1枚は「失敗として確定させて先へ進む」。
#[test]
fn a_slow_photo_times_out_while_the_rest_finish() {
    let total = 12;
    let mut jobs = fake_jobs(total);
    // worker 数より多く居座らせて、全 worker が同時に詰まる状況も通す。
    for index in [3usize, 7] {
        jobs[index].delay = Duration::from_secs(30);
    }

    let mut results: Vec<PhotoWork> = Vec::new();
    let started = Instant::now();
    let outcome = run_in_parallel(
        Arc::new(jobs),
        4,
        Duration::from_millis(300),
        &|| false,
        |index, job: &FakeJob| {
            std::thread::sleep(job.delay);
            PhotoWork::new(index, &job.id)
        },
        &mut |item| {
            results.push(item);
            Ok(())
        },
    )
    .expect("run workers");

    assert_eq!(outcome.completed, total, "残りが完走していない");
    assert_eq!(
        outcome.timed_out, 2,
        "遅い2枚が timeout として確定していない"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "遅い1枚に全体が引きずられている: {:?}",
        started.elapsed()
    );

    let failed: Vec<&PhotoWork> = results.iter().filter(|r| r.error.is_some()).collect();
    assert_eq!(failed.len(), 2);
    for item in failed {
        assert!(
            item.error.as_deref().unwrap_or_default().contains("秒以内"),
            "timeout の理由が記録されていない: {:?}",
            item.error
        );
    }
    // 遅くない 10 枚は成功として届く。
    assert_eq!(results.iter().filter(|r| r.error.is_none()).count(), 10);
}

/// 走らせて、`give_up_after` を過ぎても戻らなければ取り消しで抜ける（試験が
/// 永久に止まらないための安全弁）。`(outcome, results, 経過, 取り消しで抜けたか)`。
fn run_stuck_workers(
    delays: &[(usize, u64)],
    total: usize,
    workers: usize,
    give_up_after: Duration,
) -> (ParallelOutcome, Vec<PhotoWork>, Duration) {
    let mut jobs = fake_jobs(total);
    for (index, seconds) in delays {
        jobs[*index].delay = Duration::from_secs(*seconds);
    }
    let mut results: Vec<PhotoWork> = Vec::new();
    let started = Instant::now();
    let outcome = run_in_parallel(
        Arc::new(jobs),
        workers,
        Duration::from_millis(300),
        &|| started.elapsed() > give_up_after,
        |index, job: &FakeJob| {
            std::thread::sleep(job.delay);
            PhotoWork::new(index, &job.id)
        },
        &mut |item| {
            results.push(item);
            Ok(())
        },
    )
    .expect("run workers");
    (outcome, results, started.elapsed())
}

// NAS が切れて worker が全員固まると、以前は未処理の写真を誰も取らず、
// 進捗が止まったまま永久に戻らなかった（レビュー R13）。固まった worker の
// 代わりを足して先へ進む。
#[test]
fn a_replacement_worker_takes_over_when_every_worker_is_stuck() {
    // 2 本の worker が 0・1 枚目で固まる。残り 4 枚は代わりの worker が処理する。
    let (outcome, results, elapsed) =
        run_stuck_workers(&[(0, 30), (1, 30)], 6, 2, Duration::from_secs(8));
    assert!(!outcome.cancelled, "戻らなかった: {elapsed:?}");
    assert_eq!(outcome.completed, 6);
    assert_eq!(outcome.timed_out, 2);
    assert_eq!(results.iter().filter(|r| r.error.is_none()).count(), 4);
}

// 代わりの worker も固まり続けて上限に達したら、未処理の分を全部失敗として
// 確定して戻る（永久に待たない）。
#[test]
fn the_run_gives_up_when_the_replacements_get_stuck_too() {
    let all: Vec<(usize, u64)> = (0..8).map(|index| (index, 30)).collect();
    let (outcome, results, elapsed) = run_stuck_workers(&all, 8, 2, Duration::from_secs(8));
    assert!(!outcome.cancelled, "戻らなかった: {elapsed:?}");
    assert_eq!(outcome.completed, 8);
    assert!(results.iter().all(|r| r.error.is_some()));
    assert_eq!(results.len(), 8, "1 枚も二重に確定しない");
}

// 解析できなかった写真が DB に残り、その件数がそのまま UI へ渡ること。
#[test]
fn analysis_errors_are_recorded_and_counted() {
    let directory = test_directory("analysis-errors");
    let conn = database_with_photos(&directory, 6);

    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        tx.execute(
            "UPDATE photos SET d_hash=?1,analysis_error=?2,analysis_error_at=?3 WHERE id=?4",
            params![
                item.d_hash,
                item.error,
                item.error.as_ref().map(|_| now()),
                item.photo_id
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    };
    let mut pending: Vec<PhotoWork> = Vec::new();
    let mut failed = 0usize;
    run_in_parallel(
        Arc::new(fake_jobs(6)),
        2,
        Duration::from_secs(30),
        &|| false,
        |index, job: &FakeJob| {
            let mut result = PhotoWork::new(index, &job.id);
            // 2枚に1枚を失敗させる。
            if index % 2 == 0 {
                result.error = Some("画像を読み取れませんでした。".into());
            } else {
                result.d_hash = Some("ffffffffffffffff".into());
            }
            result
        },
        &mut |item| {
            if item.error.is_some() {
                failed += 1;
            }
            pending.push(item);
            Ok(())
        },
    )
    .expect("run workers");
    flush_results(&conn, &mut pending, &apply).expect("flush");

    assert_eq!(failed, 3, "失敗件数の集計が合っていない");
    assert_eq!(
        failed_photo_count(&conn, "project-1").expect("count failures"),
        3,
        "UI に渡す件数が DB の実態と食い違っている"
    );
    // 失敗しても他は普通に解析されている＝続行可能。
    let hashed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count hashed");
    assert_eq!(hashed, 3, "失敗が他の写真の解析を巻き込んでいる");

    // 成功に転じたらエラーは消える（居座らない）。
    let recovered = vec![{
        let mut item = PhotoWork::new(0, "photo-0");
        item.d_hash = Some("0123456789abcdef".into());
        item
    }];
    let mut recovered = recovered;
    flush_results(&conn, &mut recovered, &apply).expect("flush recovery");
    assert_eq!(
        failed_photo_count(&conn, "project-1").expect("recount"),
        2,
        "解析に成功しても古いエラーが残り続けている"
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// キャンセルが素早く効き、かつそれまでに確定した分が残ること。
#[test]
fn cancelling_responds_quickly_and_keeps_committed_work() {
    let directory = test_directory("parallel-cancel");
    let total = 400;
    let conn = database_with_photos(&directory, total);

    let apply = |tx: &Connection, item: &PhotoWork| -> Result<(), String> {
        tx.execute(
            "UPDATE photos SET d_hash='ffffffffffffffff' WHERE id=?1",
            params![item.photo_id],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let mut pending: Vec<PhotoWork> = Vec::new();
    let mut committed = 0usize;
    let watcher = cancel.clone();
    let requested = Mutex::new(None::<Instant>);

    let outcome = run_in_parallel(
        Arc::new(fake_jobs(total)),
        2,
        Duration::from_secs(30),
        &|| watcher.load(Ordering::SeqCst),
        |index, job: &FakeJob| {
            // 1枚 5ms。400枚で 2 秒ぶんの仕事。
            std::thread::sleep(Duration::from_millis(5));
            PhotoWork::new(index, &job.id)
        },
        &mut |item| {
            pending.push(item);
            if pending.len() >= ANALYSIS_CHUNK_SIZE {
                committed += flush_results(&conn, &mut pending, &apply)?;
                // 最初のチャンクが確定した直後にキャンセルする。
                if committed == ANALYSIS_CHUNK_SIZE {
                    *requested.lock().expect("lock") = Some(Instant::now());
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Ok(())
        },
    )
    .expect("run workers");
    committed += flush_results(&conn, &mut pending, &apply).expect("final flush");

    let elapsed = requested
        .lock()
        .expect("lock")
        .expect("キャンセルが要求されていない")
        .elapsed();
    assert!(outcome.cancelled, "キャンセルが伝わっていない");
    assert!(
        elapsed < Duration::from_millis(500),
        "キャンセルの反応が遅い: {elapsed:?}"
    );
    assert!(
        committed >= ANALYSIS_CHUNK_SIZE,
        "確定済みの分が失われている"
    );
    assert!(committed < total, "キャンセルしたのに全件処理されている");

    let stored: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(
        stored as usize, committed,
        "DB の実態と「保存済み」の件数が食い違っている"
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 二重起動の防止と、複数スレッドから同時に start された場合の勝者が
// ちょうど1つであること。
#[test]
fn the_task_registry_admits_exactly_one_runner() {
    let registry = Arc::new(TaskRegistry::default());
    let key = "burst:project-1";

    assert!(registry.start(key).is_ok());
    assert!(
        registry.start(key).is_err(),
        "同じプロジェクトの解析が二重に始まってしまう"
    );
    assert!(registry.is_running(key));
    // 別プロジェクトは独立している。
    assert!(registry.start("burst:project-2").is_ok());
    registry.finish(key);
    registry.finish("burst:project-2");
    assert!(!registry.is_running(key));

    // 32 スレッドから一斉に start しても、通るのは1つだけ。
    let winners = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    let handles: Vec<_> = (0..32)
        .map(|_| {
            let registry = registry.clone();
            let winners = winners.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                while !gate.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                if registry.start(key).is_ok() {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    gate.store(true, Ordering::SeqCst);
    for handle in handles {
        handle.join().expect("join");
    }
    assert_eq!(
        winners.load(Ordering::SeqCst),
        1,
        "同時 start の勝者が1つでない"
    );
}

// start より先に届いたキャンセルが、次の実行を巻き添えにしないこと。
// ここが崩れると「開始した瞬間に中断される」という再現困難な不具合になる。
#[test]
fn a_stale_cancel_does_not_kill_the_next_run() {
    let registry = TaskRegistry::default();
    let key = "burst:project-1";

    registry.start(key).expect("start");
    registry.cancel(key);
    assert!(registry.is_cancelled(key));
    registry.finish(key);

    registry.start(key).expect("restart");
    assert!(
        !registry.is_cancelled(key),
        "前回のキャンセルが次の実行に持ち越されている"
    );
    registry.finish(key);

    // 走っていないタスクへのキャンセルも、次の start が拾い上げて消す。
    registry.cancel(key);
    registry.start(key).expect("start after stray cancel");
    assert!(!registry.is_cancelled(key));
    registry.finish(key);
}

// 事前生成は前面の解析に道を譲る。両方が同じ写真を取り合わないこと。
#[test]
fn the_background_pass_yields_to_the_foreground_one() {
    let registry = TaskRegistry::default();
    let background = format!("{}:project-1", AnalysisMode::Background.task());
    let foreground = format!("{}:project-1", AnalysisMode::Foreground.task());

    registry.start(&background).expect("start background");
    // run_burst_analysis の should_stop と同じ判定。
    let should_stop = |mode: AnalysisMode, key: &str| {
        registry.is_cancelled(key)
            || (mode == AnalysisMode::Background && registry.is_running(&foreground))
    };
    assert!(
        !should_stop(AnalysisMode::Background, &background),
        "前面が走っていないのに事前生成が止まっている"
    );

    registry.start(&foreground).expect("start foreground");
    assert!(
        should_stop(AnalysisMode::Background, &background),
        "前面が始まっても事前生成が居座っている"
    );
    assert!(
        !should_stop(AnalysisMode::Foreground, &foreground),
        "前面が自分自身を止めてしまっている"
    );

    registry.finish(&foreground);
    registry.finish(&background);
}

// worker 数の既定は控えめに、上書きは範囲内に丸める。
#[test]
fn the_worker_count_stays_within_its_bounds() {
    let workers = analysis_worker_count();
    assert!(
        (MIN_ANALYSIS_WORKERS..=MAX_ANALYSIS_WORKERS).contains(&workers),
        "既定の worker 数が範囲外: {workers}"
    );
    // 事前生成は必ず 1 本。前面の操作を邪魔しないため。
    assert_eq!(AnalysisMode::Background.workers(Path::new("C:/photos")), 1);
}

// migration は利用者の実データに触れるため、合成データだけでなく実物の
// コピーに対しても流して無傷を確かめられるようにしておく。
// 環境変数 PHOTO_CURATOR_REAL_DB に既存DBのパスを渡すと実行される。
//   PHOTO_CURATOR_REAL_DB=... cargo test --manifest-path src-tauri/Cargo.toml
// 元のDBは読むだけで、書き込みは一時ディレクトリのコピーに対してのみ行う。
#[test]
fn migrating_a_real_database_copy_preserves_every_row() {
    let Ok(source) = std::env::var("PHOTO_CURATOR_REAL_DB") else {
        eprintln!("PHOTO_CURATOR_REAL_DB が未設定のため skip");
        return;
    };
    let source = PathBuf::from(source);
    assert!(source.is_file(), "指定されたDBが見つからない: {source:?}");

    let directory = test_directory("real-db");
    let copy = directory.join("copy.sqlite3");
    fs::copy(&source, &copy).expect("copy database");

    // migration 前のスナップショット。
    let before: Vec<(String, Option<i64>, Option<String>, i64)> = {
        let conn = Connection::open(&copy).expect("open copy");
        let mut statement = conn
            .prepare("SELECT id,captured_at,d_hash,rating FROM photos ORDER BY id")
            .expect("prepare snapshot");
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query snapshot");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("collect snapshot")
    };

    // 本番と同じ経路で migration + backfill を走らせる。
    let conn = open_database(&copy).expect("migrate real database copy");

    let after: Vec<(String, Option<i64>, Option<String>, i64)> = {
        let mut statement = conn
            .prepare("SELECT id,captured_at,d_hash,rating FROM photos ORDER BY id")
            .expect("prepare verification");
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query verification");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("collect verification")
    };

    assert_eq!(before.len(), after.len(), "行数が変わらない");
    assert_eq!(
        before, after,
        "captured_at / d_hash / rating が1件も書き換わらない"
    );

    let filled: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE fingerprint_mtime IS NOT NULL AND fingerprint_size IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count fingerprints");
    eprintln!(
        "実DBコピー: {} 行 / fingerprint 補完済み {} 行",
        after.len(),
        filled
    );

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn pair_overrides_are_replaced_per_project() {
    let directory = test_directory("pair-overrides");
    let mut conn = open_database(&directory.join("overrides.sqlite3")).expect("open database");
    let row = |left: &str, right: &str, decision: &str| PairOverrideRow {
        left: left.to_string(),
        right: right.to_string(),
        decision: decision.to_string(),
    };
    let read = |conn: &Connection, project: &str| -> Vec<(String, String, String)> {
        let mut statement = conn
            .prepare(
                "SELECT left_path,right_path,decision FROM pair_overrides
                     WHERE project_id=?1 ORDER BY left_path,right_path",
            )
            .expect("prepare");
        statement
            .query_map(params![project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect")
    };

    replace_pair_overrides(&mut conn, "p1", &[row("a.jpg", "b.jpg", "join"), row("b.jpg", "c.jpg", "split")])
        .expect("save p1");
    replace_pair_overrides(&mut conn, "p2", &[row("a.jpg", "b.jpg", "split")]).expect("save p2");
    assert_eq!(read(&conn, "p1").len(), 2);

    // 入れ替えは丸ごと。前にあって今回無いものは消え、他のプロジェクトは触らない。
    replace_pair_overrides(&mut conn, "p1", &[row("b.jpg", "c.jpg", "join")]).expect("replace p1");
    assert_eq!(
        read(&conn, "p1"),
        vec![("b.jpg".to_string(), "c.jpg".to_string(), "join".to_string())]
    );
    assert_eq!(read(&conn, "p2").len(), 1, "他のプロジェクトは変わらない");

    replace_pair_overrides(&mut conn, "p1", &[]).expect("clear p1");
    assert!(read(&conn, "p1").is_empty());

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn duplicate_pair_overrides_keep_the_first_one_like_core() {
    let directory = test_directory("pair-overrides-first");
    let mut conn = open_database(&directory.join("overrides.sqlite3")).expect("open database");
    let row = |decision: &str| PairOverrideRow {
        left: "a.jpg".to_string(),
        right: "b.jpg".to_string(),
        decision: decision.to_string(),
    };
    replace_pair_overrides(&mut conn, "p1", &[row("join"), row("split")]).expect("save");
    let decision: String = conn
        .query_row(
            "SELECT decision FROM pair_overrides WHERE project_id=?1 AND left_path='a.jpg' AND right_path='b.jpg'",
            params!["p1"],
            |r| r.get(0),
        )
        .expect("read");
    assert_eq!(decision, "join", "同じ組が重なったら先のものが残る（core と同じ）");
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn burst_threshold_columns_migrate_without_touching_existing_projects() {
    let directory = test_directory("burst-threshold");
    let database = directory.join("legacy.sqlite3");

    {
        // burst_threshold を持たない旧スキーマ。
        let legacy = Connection::open(&database).expect("open legacy");
        legacy
            .execute_batch(
                "CREATE TABLE projects (
                       id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                       photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                       created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                     );",
            )
            .expect("create legacy projects");
        legacy
            .execute(
                "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                     VALUES ('p1','旧プロジェクト','C:/photos',271,'ready',100,200)",
                [],
            )
            .expect("insert legacy project");
    }

    let conn = open_database(&database).expect("migrate");
    let (name, count, threshold): (String, i64, Option<i64>) = conn
        .query_row(
            "SELECT name,photo_count,burst_threshold FROM projects WHERE id='p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read migrated project");
    assert_eq!(name, "旧プロジェクト", "既存の値を壊さない");
    assert_eq!(count, 271);
    assert_eq!(threshold, None, "未学習は NULL のまま");

    drop(conn);
    // 冪等であること。
    open_database(&database).expect("reopen");

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 削除は写真原本に触れてはいけない。DB とサムネイルだけを消す。
#[test]
fn deleting_a_project_removes_only_app_owned_data() {
    let directory = test_directory("delete-project");
    let original = directory.join("original.jpg");
    let thumbnail = directory.join("thumb.jpg");
    fs::write(&original, b"original photo bytes").expect("write original");
    fs::write(&thumbnail, b"thumb").expect("write thumbnail");
    let database = directory.join("delete.sqlite3");
    let conn = open_database(&database).expect("open database");

    for (project, photo) in [("keep", "photo-keep"), ("drop", "photo-drop")] {
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                 VALUES (?1,?1,?2,1,'ready',1,1)",
            params![project, directory.to_string_lossy().to_string()],
        )
        .expect("insert project");
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing,thumbnail_path)
                 VALUES (?1,?2,?3,'original.jpg','original.jpg',1,'0000000000000000',0,1,1,0,?4)",
            params![
                photo,
                project,
                original.to_string_lossy().to_string(),
                thumbnail.to_string_lossy().to_string()
            ],
        )
        .expect("insert photo");
        conn.execute(
            "INSERT INTO project_states (project_id,state_json,updated_at) VALUES (?1,'{}',1)",
            params![project],
        )
        .expect("insert state");
    }

    // delete_project の中身と同じ手順（AppHandle を要するコマンド本体は
    // ここから呼べないため、同じ SQL とファイル削除を並べて検証する）。
    let thumbnails: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT thumbnail_path FROM photos WHERE project_id=?1 AND thumbnail_path IS NOT NULL")
            .expect("prepare thumbnails");
        statement
            .query_map(params!["drop"], |row| row.get(0))
            .expect("query thumbnails")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect thumbnails")
    };
    for path in &thumbnails {
        let _ = fs::remove_file(Path::new(path));
    }
    conn.execute(
        "DELETE FROM project_states WHERE project_id=?1",
        params!["drop"],
    )
    .expect("delete state");
    conn.execute("DELETE FROM photos WHERE project_id=?1", params!["drop"])
        .expect("delete photos");
    conn.execute("DELETE FROM projects WHERE id=?1", params!["drop"])
        .expect("delete project");

    assert!(original.is_file(), "写真原本は残る（削除テスト）");
    assert!(!thumbnail.is_file(), "サムネイルは消える");

    let projects: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .expect("count projects");
    let photos: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))
        .expect("count photos");
    let states: i64 = conn
        .query_row("SELECT COUNT(*) FROM project_states", [], |row| row.get(0))
        .expect("count states");
    assert_eq!(projects, 1, "他のプロジェクトは残る");
    assert_eq!(photos, 1);
    assert_eq!(states, 1);

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// プロジェクト削除で、そのプロジェクトの表示用画像だけが消える（U27 R7）。
// 他のプロジェクトの画像・原本・置き場の外を指す値は残る。
#[test]
fn deleting_a_project_removes_only_its_display_images() {
    let directory = test_directory("delete-display");
    let display = directory.join("display");
    fs::create_dir_all(&display).expect("create display dir");
    let original = directory.join("original.jpg");
    fs::write(&original, b"original photo bytes").expect("write original");
    let drop_image = display.join("photo-drop.jpg");
    let drop_second = display.join("photo-drop-2.jpg");
    let keep_image = display.join("photo-keep.jpg");
    for file in [&drop_image, &drop_second, &keep_image] {
        fs::write(file, b"display").expect("write display image");
    }
    let conn = open_database(&directory.join("delete.sqlite3")).expect("open database");
    for project in ["keep", "drop"] {
        conn.execute(
            "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                 VALUES (?1,?1,'x',1,'ready',1,1)",
            params![project],
        )
        .expect("insert project");
    }
    for (id, project, display_path) in [
        ("photo-keep", "keep", keep_image.clone()),
        ("photo-drop", "drop", drop_image.clone()),
        ("photo-drop-2", "drop", drop_second.clone()),
        // 壊れた行: 原本を指している。置き場の外なので消してはいけない。
        ("photo-bad", "drop", original.clone()),
        // 置き場の外へ抜ける相対表記。
        ("photo-dots", "drop", display.join("..").join("original.jpg")),
    ] {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,display_path)
                 VALUES (?1,?2,?1,'r','n',?3)",
            params![id, project, display_path.to_string_lossy().to_string()],
        )
        .expect("insert photo");
    }

    let removed = remove_project_display_files(&conn, "drop", &display).expect("remove");

    assert_eq!(removed, 2);
    assert!(!drop_image.exists() && !drop_second.exists(), "そのプロジェクトの画像は消える");
    assert!(keep_image.is_file(), "他のプロジェクトの画像は残る");
    assert!(original.is_file(), "原本・置き場の外を指す値は消さない");
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// -----------------------------------------------------------------------
// レーティングと書き出し
// -----------------------------------------------------------------------

fn insert_rated_photo(conn: &Connection, project: &str, id: &str, name: &str, rating: i64) {
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,d_hash,rating,fingerprint_mtime,fingerprint_size,is_missing,thumbnail_path)
             VALUES (?1,?2,?3,?4,?4,1,'0000000000000000',?5,1,1,0,'C:/thumb.jpg')",
        params![id, project, format!("C:/photos/{name}"), name, rating],
    )
    .expect("insert rated photo");
}

fn rated_fixture(database: &Path) -> Connection {
    let conn = open_database(database).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',4,'ready',1,1)",
        [],
    )
    .expect("insert project");
    insert_rated_photo(&conn, "p1", "five", "e-five.jpg", 5);
    insert_rated_photo(&conn, "p1", "three", "d-three.jpg", 3);
    insert_rated_photo(&conn, "p1", "three-b", "c-three.jpg", 3);
    insert_rated_photo(&conn, "p1", "zero", "b-zero.jpg", 0);
    conn
}

/// `photo_page_query` が返した SQL と bind を**そのまま実行**する。
/// 句だけを組み立てて検証していたときは、bind の数が合わないことに
/// 気づけず「Wrong number of parameters passed to query」で落ちていた。
fn run_photo_page(
    conn: &Connection,
    rating: Option<i64>,
    sort: Option<&str>,
) -> (i64, Vec<String>) {
    let (count_sql, page_sql, binds) = photo_page_query("p1", 0, 80, rating, sort);
    let count_binds: Vec<rusqlite::types::Value> = std::iter::once(binds[0].clone())
        .chain(binds.get(3).cloned())
        .collect();
    let total: i64 = conn
        .query_row(&count_sql, rusqlite::params_from_iter(count_binds), |row| {
            row.get(0)
        })
        .expect("count query");
    let mut statement = conn.prepare(&page_sql).expect("prepare page");
    let names = statement
        .query_map(rusqlite::params_from_iter(binds), |row| {
            row.get::<_, String>(0)
        })
        .expect("page query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect page");
    (total, names)
}

#[test]
fn photo_page_query_binds_match_the_placeholders() {
    let directory = test_directory("page-query");
    let conn = rated_fixture(&directory.join("page.sqlite3"));

    // 星を指定しない場合。以前はここで bind が 1 個余って落ちていた。
    let (total, ids) = run_photo_page(&conn, None, None);
    assert_eq!(total, 4);
    assert_eq!(ids.len(), 4);

    // 星を指定した場合。件数も中身も絞られる。
    let (total, ids) = run_photo_page(&conn, Some(3), None);
    assert_eq!(total, 2, "★3 は 2 枚");
    assert_eq!(ids.len(), 2);

    let (total, _) = run_photo_page(&conn, Some(5), Some("rating"));
    assert_eq!(total, 1);
    let (total, _) = run_photo_page(&conn, Some(4), None);
    assert_eq!(total, 0, "該当なしでも落ちない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn photo_page_query_sorts_by_rating() {
    let directory = test_directory("page-sort");
    let conn = rated_fixture(&directory.join("sort.sqlite3"));

    let (_, by_rating) = run_photo_page(&conn, None, Some("rating"));
    let (_, by_name) = run_photo_page(&conn, None, Some("name"));
    assert_eq!(by_rating, vec!["five", "three-b", "three", "zero"]);
    assert_eq!(by_name, vec!["zero", "three-b", "three", "five"]);

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn selection_targets_are_chosen_by_rating() {
    let directory = test_directory("rating-target");
    let conn = rated_fixture(&directory.join("t.sqlite3"));

    let ids = |rating: Option<i64>| -> Vec<String> {
        let clause = if rating.is_some() {
            " AND rating=?2"
        } else {
            ""
        };
        let mut statement = conn
            .prepare(&format!(
                "SELECT id FROM photos WHERE project_id=?1 AND is_missing=0{clause} ORDER BY relative_path"
            ))
            .expect("prepare");
        statement
            .query_map(
                rusqlite::params_from_iter(
                    std::iter::once(rusqlite::types::Value::from("p1".to_string()))
                        .chain(rating.map(rusqlite::types::Value::from)),
                ),
                |row| row.get(0),
            )
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect")
    };

    // 同じ星の写真は、どの経路で辿り着いたかに関係なく1つの対象になる。
    assert_eq!(ids(Some(3)), vec!["three-b", "three"]);
    assert_eq!(ids(Some(5)), vec!["five"]);
    assert_eq!(ids(Some(0)), vec!["zero"]);
    assert_eq!(ids(None).len(), 4);

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// ---- ハッシュ値（core）・DB のファイル ------------------------------------

/// 同じ絵を JPEG で作り直しても、ハッシュ値はほとんど動かない（core と同じ基準）。
#[test]
fn the_fingerprint_survives_a_jpeg_round_trip() {
    let original = synthetic_image(640, 480, 3);
    let recompressed = image::load_from_memory_with_format(
        &encode_thumbnail(&original).expect("encode"),
        image::ImageFormat::Jpeg,
    )
    .expect("decode");
    let a = d_hash_of(&original).expect("hash of original");
    let b = d_hash_of(&recompressed).expect("hash of recompressed");
    assert!(
        photo_curator_core::hash_distance(a.clone(), b.clone()) <= 2,
        "{a} vs {b}"
    );
    assert_eq!(D_HASH_VERSION, 2);
}

#[test]
fn a_too_small_image_has_no_fingerprint() {
    assert!(d_hash_of(&synthetic_image(4, 4, 0)).is_none());
}

#[test]
fn a_fresh_database_file_creates_every_table() {
    let directory = test_directory("fresh-db");
    let conn = open_database(&directory.join("photo-curator-v2.sqlite3")).expect("open");
    for table in [
        "projects",
        "photos",
        "project_states",
        "app_settings",
        "pair_overrides",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(count, 1, "表 {table} が作られていない");
    }
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// ---- 走査の除外・形式の判定・ネットワークのフォルダ ------------------

fn relative_names(folder: &Path, files: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = files
        .iter()
        .map(|path| {
            path.strip_prefix(folder)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    names.sort();
    names
}

#[test]
fn supported_names_are_images_raws_or_have_no_extension() {
    for name in ["a.jpg", "a.JPEG", "a.png", "a.webp", "a.heic", "a.cr2", "a.dng", "a.rw2x", "noext"] {
        let expected = name != "a.rw2x";
        assert_eq!(is_supported(Path::new(name)), expected, "{name}");
    }
    for name in ["a.mp4", "a.MOV", "a.m4v", "a.avi", "a.mts", "a.m2ts", "a.3gp", "a.mkv", "a.txt"] {
        assert!(!is_supported(Path::new(name)), "{name}");
    }
}

/// 走査の試験の道具。プロジェクト 1 件（フォルダ付き）を持つ DB を作る。
fn scan_fixture(label: &str, names: &[&str]) -> (PathBuf, String, Connection) {
    let directory = test_directory(label);
    let root = directory.to_string_lossy().to_string();
    for name in names {
        fs::write(directory.join(name), b"photo bytes").unwrap();
    }
    let conn = open_database(&directory.join("scan.sqlite3")).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1',?1,0,'ready',1,1)",
        params![root],
    )
    .expect("insert project");
    (directory, root, conn)
}

fn scan_for_test(
    conn: &Connection,
    root: &str,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScanEnd, String> {
    scan_batched(conn, root, SCAN_BATCH, is_cancelled)
}

fn scan_batched(
    conn: &Connection,
    root: &str,
    batch_size: usize,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScanEnd, String> {
    scan_folder(conn, "p1", root, batch_size, is_cancelled, &mut |_, _, _, _| {})
}

fn missing_names(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM photos WHERE project_id='p1' AND is_missing=1 ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn project_row(conn: &Connection) -> (i64, String) {
    conn.query_row(
        "SELECT photo_count,status FROM projects WHERE id='p1'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

// 再走査を途中で取り消すと、以前は `is_missing=1` だけが commit されて残り、
// 件数は前のまま、一覧は 0 件になっていた（レビュー R11）。
#[test]
fn cancelling_a_rescan_leaves_the_missing_flags_as_they_were() {
    let (directory, root, conn) = scan_fixture("scan-cancel", &["a.jpg", "b.jpg", "c.jpg"]);
    let first = scan_for_test(&conn, &root, &|| false).expect("first scan");
    assert!(matches!(first, ScanEnd::Completed { count: 3, .. }));
    assert_eq!(project_row(&conn), (3, "ready".to_string()));

    // 1 枚ずつの塊で、2 つ目の塊の頭で取り消す。b.jpg は消えているが、
    // 走査が最後まで成功していないので、欠損の印は付かない。
    fs::remove_file(directory.join("b.jpg")).unwrap();
    let calls = std::cell::Cell::new(0);
    let ended = scan_batched(&conn, &root, 1, &|| {
        calls.set(calls.get() + 1);
        calls.get() > 1
    })
    .expect("cancelled scan");
    assert!(matches!(ended, ScanEnd::Cancelled { processed: 1, total: 2 }));
    assert!(missing_names(&conn).is_empty(), "取り消しで欠損の印を残さない");
    assert_eq!(project_row(&conn), (3, "ready".to_string()));

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 走査が書き込みロックを持つのは塊の中だけ。塊と塊のあいだに、別の接続の
// 書き込みが待たされずに通る（レビュー R5。以前は走査の間ずっと持っていた）。
#[test]
fn a_scan_does_not_hold_the_write_lock_between_batches() {
    let names: Vec<String> = (0..6).map(|index| format!("p{index}.jpg")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (directory, root, conn) = scan_fixture("scan-lock", &refs);
    let other = Connection::open(directory.join("scan.sqlite3")).expect("open other");
    other.busy_timeout(std::time::Duration::from_millis(0)).unwrap();
    let writes = std::cell::Cell::new(0);
    let ended = scan_batched(&conn, &root, 2, &|| {
        other
            .execute("UPDATE projects SET name='other' WHERE id='p1'", [])
            .expect("another writer must not be locked out between batches");
        writes.set(writes.get() + 1);
        false
    })
    .expect("scan");
    assert!(matches!(ended, ScanEnd::Completed { count: 6, .. }));
    assert_eq!(writes.get(), 3, "6 枚を 2 枚ずつ 3 つの塊");
    drop((conn, other));
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 列挙で得た更新時刻・大きさは、1 枚ずつ stat し直した値と同じ（レビュー R4）。
// 違うと全行の変化の判定が変わり、サムネイル・ハッシュ値が捨てられて再計算になる。
#[test]
fn the_listing_fingerprint_matches_a_separate_stat() {
    let directory = test_directory("scan-list-fp");
    fs::write(directory.join("a.jpg"), b"one").unwrap();
    fs::create_dir_all(directory.join("sub")).unwrap();
    fs::write(directory.join("sub/b.jpg"), b"two two").unwrap();
    let listing = list_photo_files(&directory.to_string_lossy(), true).expect("list");
    assert_eq!(listing.files.len(), 2);
    for file in &listing.files {
        assert!(file.fingerprint.is_some());
        assert_eq!(file.fingerprint, fingerprint(&file.path), "{:?}", file.path);
    }
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// 走査が最後まで成功したときだけ、見つからなかった写真に欠損の印が付く。
#[test]
fn a_completed_rescan_marks_only_the_photos_that_vanished() {
    let (directory, root, conn) = scan_fixture("scan-vanish", &["a.jpg", "b.jpg"]);
    scan_for_test(&conn, &root, &|| false).expect("first scan");
    fs::remove_file(directory.join("b.jpg")).unwrap();
    let ended = scan_for_test(&conn, &root, &|| false).expect("rescan");
    assert!(matches!(ended, ScanEnd::Completed { count: 1, unreadable: 0, .. }));
    assert_eq!(missing_names(&conn), vec!["b.jpg".to_string()]);
    assert_eq!(project_row(&conn), (1, "ready".to_string()));
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

// NAS が落ちている・共有が外れている・フォルダを移動した場合に、以前は列挙が
// 空になって全写真が欠損、「0 枚で準備完了」になっていた（レビュー R12）。
#[test]
fn scanning_a_folder_that_cannot_be_opened_fails_and_keeps_the_photos() {
    let (directory, root, conn) = scan_fixture("scan-gone", &["a.jpg", "b.jpg"]);
    scan_for_test(&conn, &root, &|| false).expect("first scan");
    // フォルダ自体が無くなる（DB は別の場所に置いてある）。
    let gone = directory.join("moved-away").to_string_lossy().to_string();
    let error = scan_for_test(&conn, &gone, &|| false).err().expect("must fail");
    assert!(error.contains("接続できません"), "{error}");
    assert!(missing_names(&conn).is_empty(), "つながらないとき写真を欠損にしない");
    assert_eq!(project_row(&conn), (2, "ready".to_string()), "状態も前のまま");
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// 隠しフォルダ・動画は数えず、中身が JPEG の `.cr2` は数え、中身がテキストの
/// `.jpg` は数から外れる。
#[test]
fn scanning_skips_hidden_and_video_and_judges_the_content() {
    let directory = test_directory("scan-content");
    let root = directory.to_string_lossy().to_string();
    let jpeg = jpeg_bytes(64, 48, 1);
    fs::create_dir_all(directory.join(".hidden")).unwrap();
    fs::write(directory.join(".hidden/a.jpg"), &jpeg).unwrap();
    fs::write(directory.join(".hidden_file.jpg"), &jpeg).unwrap();
    fs::write(directory.join("ok.jpg"), &jpeg).unwrap();
    fs::write(directory.join("fake.cr2"), &jpeg).unwrap();
    fs::write(directory.join("broken.jpg"), b"this is not an image, just text").unwrap();
    fs::write(directory.join("video.mp4"), b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom").unwrap();
    // HEIC は先頭が `ftyp` だが画像。数から外さず、「読めなかった」に数える。
    fs::write(directory.join("photo.heic"), b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").unwrap();

    let files: Vec<PathBuf> = list_photo_files(&root, true)
        .expect("list")
        .files
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(
        relative_names(&directory, &files),
        vec!["broken.jpg", "fake.cr2", "ok.jpg", "photo.heic"]
    );

    let conn = open_database(&directory.join("scan.sqlite3")).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1',?1,0,'ready',1,1)",
        params![root],
    )
    .expect("insert project");
    for path in &files {
        let (mtime, size) = fingerprint(path).map_or((None, None), |(m, s)| (Some(m), Some(s)));
        let relative = path.strip_prefix(&directory).unwrap().to_string_lossy().to_string();
        upsert_photo(
            &conn,
            "p1",
            &path.to_string_lossy(),
            &relative,
            &relative,
            mtime,
            size,
        )
        .expect("upsert");
    }
    assert_eq!(recount_photos(&conn, "p1").unwrap(), 4);

    for (index, path) in files.iter().enumerate() {
        let id: String = conn
            .query_row(
                "SELECT id FROM photos WHERE path=?1",
                params![path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let job = MetadataJob { id, path: path.to_string_lossy().to_string() };
        let work = metadata_one(index, &job);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(work.not_image, name == "broken.jpg", "{name}");
        apply_metadata(&conn, &work).expect("apply");
    }
    assert_eq!(recount_photos(&conn, "p1").unwrap(), 3, "broken.jpg は数から外れる");
    let count: i64 = conn
        .query_row("SELECT photo_count FROM projects WHERE id='p1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 3);
    let missing: String = conn
        .query_row("SELECT name FROM photos WHERE is_missing=1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(missing, "broken.jpg");
    // fake.cr2 は撮影時刻まで読めている（画像として扱われた）。
    let captured: Option<i64> = conn
        .query_row("SELECT captured_at FROM photos WHERE name='fake.cr2'", [], |row| row.get(0))
        .unwrap();
    assert!(captured.is_some());
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// U46: 同じフォルダに同名の JPEG がある RAW は、走査の結果に入れない。
#[test]
fn listing_skips_a_raw_that_has_a_same_named_jpeg_beside_it() {
    let directory = test_directory("scan-paired-raw");
    let root = directory.to_string_lossy().to_string();
    fs::create_dir_all(directory.join("sub")).unwrap();
    for name in [
        "IMG_1.JPG", "IMG_1.CR2", "IMG_2.CR2", "IMG_3.jpeg", "IMG_3.NEF", "sub/IMG_1.CR2",
        "IMG_4.HEIC", "img_5.jpg", "IMG_5.CR2", "IMG_6.png", "IMG_6.DNG", "IMG_7.heic",
        "IMG_7.jpg",
    ] {
        fs::write(directory.join(name), b"photo bytes").unwrap();
    }
    let files: Vec<PathBuf> = list_photo_files(&root, true)
        .expect("list")
        .files
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(
        relative_names(&directory, &files),
        vec![
            "IMG_1.JPG", "IMG_2.CR2", "IMG_3.jpeg", "IMG_4.HEIC", "IMG_6.DNG", "IMG_6.png",
            "IMG_7.heic", "IMG_7.jpg", "img_5.jpg", "sub/IMG_1.CR2",
        ]
    );
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn skip_paired_raw_is_a_pure_function_of_the_paths() {
    let paths: Vec<PathBuf> = ["d/a.jpg", "d/A.cr3", "d/b.cr3", "e/a.arw", "d/c.heif", "d/c.jpeg"]
        .iter()
        .map(PathBuf::from)
        .collect();
    let kept = skip_paired_raw(paths, true, |path| path.as_path());
    let names: Vec<String> = kept
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(names, vec!["d/a.jpg", "d/b.cr3", "e/a.arw", "d/c.heif", "d/c.jpeg"]);
}

/// U46: 既に DB にある組の RAW は、再走査で「見つからなかった」ことになり欠損になる
/// （解析エラーの表示から消える）。組でない RAW は残る。
#[test]
fn rescanning_marks_an_already_registered_paired_raw_as_missing() {
    let (directory, root, conn) = scan_fixture("scan-paired-raw-db", &["a.jpg", "a.cr2", "b.cr2"]);
    // 以前の版が RAW も登録していた状態を作る。
    for name in ["a.jpg", "a.cr2", "b.cr2"] {
        let path = directory.join(name);
        upsert_photo(&conn, "p1", &path.to_string_lossy(), name, name, Some(1), Some(1))
            .expect("upsert");
    }
    let ended = scan_for_test(&conn, &root, &|| false).expect("scan");
    assert!(matches!(ended, ScanEnd::Completed { total: 2, count: 2, unreadable: 0 }));
    assert_eq!(missing_names(&conn), vec!["a.cr2"]);
    assert_eq!(project_row(&conn), (2, "ready".to_string()));
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// U46: 設定がオフなら、同名の JPEG があっても RAW を全部対象にする（今までどおり）。
#[test]
fn with_the_setting_off_every_file_stays_in_the_scan() {
    let kept = skip_paired_raw(
        vec![PathBuf::from("d/a.jpg"), PathBuf::from("d/a.cr2")],
        false,
        |path| path.as_path(),
    );
    assert_eq!(kept.len(), 2);

    let (directory, root, conn) = scan_fixture("scan-paired-raw-off", &["a.jpg", "a.cr2", "b.cr2"]);
    assert!(project_pair_raw(&conn, "p1"), "既定はオン");
    conn.execute("UPDATE projects SET pair_raw_jpeg=0 WHERE id='p1'", []).unwrap();
    assert!(!project_pair_raw(&conn, "p1"));
    let ended = scan_for_test(&conn, &root, &|| false).expect("scan");
    assert!(matches!(ended, ScanEnd::Completed { total: 3, count: 3, unreadable: 0 }));
    assert!(missing_names(&conn).is_empty());
    // オンに戻して再走査すると、組の RAW が欠損になる。
    conn.execute("UPDATE projects SET pair_raw_jpeg=1 WHERE id='p1'", []).unwrap();
    scan_for_test(&conn, &root, &|| false).expect("rescan");
    assert_eq!(missing_names(&conn), vec!["a.cr2"]);
    assert_eq!(project_row(&conn), (2, "ready".to_string()));
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// U46: `pair_raw_jpeg` 列は既存のプロジェクトを壊さずに足され、既定は 1（オン）。
#[test]
fn pair_raw_jpeg_column_migrates_with_the_default_on() {
    let directory = test_directory("pair-raw-migrate");
    let database = directory.join("legacy.sqlite3");
    {
        let legacy = Connection::open(&database).expect("open legacy");
        legacy
            .execute_batch(
                "CREATE TABLE projects (
                       id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                       photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                       created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                     );
                     INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
                     VALUES ('p1','旧プロジェクト','C:/photos',632,'ready',100,200);",
            )
            .expect("create legacy projects");
    }
    let conn = open_database(&database).expect("migrate");
    let (name, count, pair): (String, i64, i64) = conn
        .query_row(
            "SELECT name,photo_count,pair_raw_jpeg FROM projects WHERE id='p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read migrated project");
    assert_eq!((name.as_str(), count, pair), ("旧プロジェクト", 632, 1));
    assert!(project_pair_raw(&conn, "p1"));
    conn.execute("UPDATE projects SET pair_raw_jpeg=0 WHERE id='p1'", []).unwrap();
    drop(conn);
    // 冪等で、保存した値を既定に戻さない。
    let conn = open_database(&database).expect("reopen");
    assert!(!project_pair_raw(&conn, "p1"));
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// U48: 切り替えた時刻の列は既存のプロジェクトを壊さずに 0 で足され、保存で値と時刻が入る。
#[test]
fn pair_raw_jpeg_at_column_migrates_to_zero_and_stores_the_switch_time() {
    let directory = test_directory("pair-raw-at-migrate");
    let database = directory.join("legacy.sqlite3");
    {
        let legacy = Connection::open(&database).expect("open legacy");
        legacy
            .execute_batch(
                "CREATE TABLE projects (
                       id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                       photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                       created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                       pair_raw_jpeg INTEGER NOT NULL DEFAULT 1
                     );
                     INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at,pair_raw_jpeg)
                     VALUES ('p1','U46 のプロジェクト','C:/photos',10,'ready',100,200,0);",
            )
            .expect("create legacy projects");
    }
    let conn = open_database(&database).expect("migrate");
    let read = |conn: &Connection| -> (i64, i64) {
        conn.query_row("SELECT pair_raw_jpeg,pair_raw_jpeg_at FROM projects WHERE id='p1'", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("read project")
    };
    // U46 で保存したオフはそのまま、時刻は 0（一度も切り替えていない扱い）。
    assert_eq!(read(&conn), (0, 0));
    // サイドカーから取り込んだときは、その時刻を残す。
    assert!(store_project_pair_raw(&conn, "p1", true, Some(1_790_955_613_101)).expect("store"));
    assert_eq!(read(&conn), (1, 1_790_955_613_101));
    // 画面で切り替えたときは今の時刻。
    let before = now();
    assert!(!store_project_pair_raw(&conn, "p1", false, None).expect("store"));
    let (value, at) = read(&conn);
    assert_eq!(value, 0);
    assert!(at >= before, "切り替えた時刻: {at}");
    drop(conn);
    // 冪等で、時刻を 0 に戻さない。
    let conn = open_database(&database).expect("reopen");
    assert_eq!(read(&conn).1, at);
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn a_local_folder_keeps_the_default_worker_count() {
    assert!(!is_network_path(Path::new("/home/user/photos")));
    assert!(!is_network_path(Path::new("C:/photos")));
    if std::env::var(WORKER_COUNT_ENV).is_err() {
        assert_eq!(
            analysis_worker_count_for(Path::new("/home/user/photos")),
            analysis_worker_count()
        );
    }
    assert_eq!(NETWORK_ANALYSIS_WORKERS, 2);
}

// ---- レートの移動 ----------------------------------------------------

#[test]
fn moving_a_rating_only_touches_the_source_star() {
    let directory = test_directory("move-rating");
    let path = directory.join("move.sqlite3");
    let conn = rated_fixture(&path);

    // ★3 の 2 枚をまるごと ★5 へ。
    let moved = move_rating_in(&conn, "p1", 3, 5, None, &[]).expect("move");
    assert_eq!(moved, 2);

    let count = |rating: i64| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM photos WHERE project_id='p1' AND rating=?1",
            params![rating],
            |row| row.get(0),
        )
        .expect("count")
    };
    assert_eq!(count(3), 0, "元の星は空になる");
    assert_eq!(count(5), 3, "元から★5 の1枚に2枚が合流する");
    assert_eq!(count(0), 1, "他の星は動かない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn excluded_photos_stay_where_they_are() {
    let directory = test_directory("move-exclude");
    let conn = rated_fixture(&directory.join("ex.sqlite3"));

    let moved =
        move_rating_in(&conn, "p1", 3, 1, None, &["three-b".to_string()]).expect("move");
    assert_eq!(moved, 1);

    let rating = |id: &str| -> i64 {
        conn.query_row(
            "SELECT rating FROM photos WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .expect("read rating")
    };
    assert_eq!(rating("three"), 1);
    assert_eq!(rating("three-b"), 3, "除外した写真は元の星に残る");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn include_ids_move_only_those_photos() {
    let directory = test_directory("move-include");
    let conn = rated_fixture(&directory.join("in.sqlite3"));

    let include = Some(vec!["three-b".to_string()]);
    let moved = move_rating_in(&conn, "p1", 3, 2, include, &[]).expect("move");
    assert_eq!(moved, 1);

    let rating = |id: &str| -> i64 {
        conn.query_row(
            "SELECT rating FROM photos WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .expect("read rating")
    };
    assert_eq!(rating("three-b"), 2);
    assert_eq!(rating("three"), 3, "指定しなかった写真は動かない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn moving_never_leaves_the_project_or_the_source_star() {
    let directory = test_directory("move-scope");
    let conn = rated_fixture(&directory.join("scope.sqlite3"));
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p2','p2','C:/other',1,'ready',1,1)",
        [],
    )
    .expect("insert other project");
    insert_rated_photo(&conn, "p2", "other-three", "other.jpg", 3);

    // include_ids に他プロジェクトの写真を混ぜても動かない。
    let include = Some(vec!["other-three".to_string(), "three".to_string()]);
    let moved = move_rating_in(&conn, "p1", 3, 4, include, &[]).expect("move");
    assert_eq!(moved, 1);

    let rating = |id: &str| -> i64 {
        conn.query_row(
            "SELECT rating FROM photos WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .expect("read rating")
    };
    assert_eq!(rating("other-three"), 3, "別プロジェクトには触れない");
    assert_eq!(rating("five"), 5, "元の星が違う写真も動かない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn moving_to_the_same_star_or_out_of_range_is_rejected() {
    let directory = test_directory("move-guard");
    let conn = rated_fixture(&directory.join("guard.sqlite3"));

    assert_eq!(
        move_rating_in(&conn, "p1", 3, 3, None, &[]).expect("same star"),
        0,
        "同じ星への移動は何もしない"
    );
    assert!(move_rating_in(&conn, "p1", 3, MAX_RATING + 1, None, &[]).is_err());
    assert!(move_rating_in(&conn, "p1", -1, 3, None, &[]).is_err());

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// id を `IN (?, ?, …)` で渡すと SQLite の変数上限に当たる。
/// 一時テーブル経由にしてあることを、上限を超える件数で確かめる。
#[test]
fn moving_handles_more_ids_than_sqlite_allows_as_parameters() {
    let directory = test_directory("move-many");
    let conn = open_database(&directory.join("many.sqlite3")).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',1500,'ready',1,1)",
        [],
    )
    .expect("insert project");
    for index in 0..1500 {
        insert_rated_photo(
            &conn,
            "p1",
            &format!("id-{index}"),
            &format!("{index}.jpg"),
            2,
        );
    }

    // 1 枚だけ残して、残り 1,499 枚を明示指定で動かす。
    let include: Vec<String> = (0..1499).map(|index| format!("id-{index}")).collect();
    let moved = move_rating_in(&conn, "p1", 2, 4, Some(include), &[]).expect("move many");
    assert_eq!(moved, 1499);

    let left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE project_id='p1' AND rating=2",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(left, 1);

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn resetting_ratings_keeps_the_analysis() {
    let directory = test_directory("rating-reset");
    let conn = rated_fixture(&directory.join("r.sqlite3"));

    conn.execute("UPDATE photos SET rating=0 WHERE project_id='p1'", [])
        .expect("reset");

    let (ratings, hashes, thumbs): (i64, i64, i64) = conn
        .query_row(
            "SELECT SUM(rating),SUM(d_hash IS NOT NULL),SUM(thumbnail_path IS NOT NULL)
                 FROM photos WHERE project_id='p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read after reset");
    assert_eq!(ratings, 0, "星は消える");
    assert_eq!(hashes, 4, "d_hash は消さない");
    assert_eq!(thumbs, 4, "サムネイルは消さない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn export_targets_are_the_given_photo_ids() {
    let directory = test_directory("export-ids");
    let conn = rated_fixture(&directory.join("e.sqlite3"));

    let ids = vec!["three-b".to_string(), "five".to_string(), "missing".to_string()];
    let targets = photos_for_export(&conn, "p1", &ids).expect("targets");
    // 相対パス順（c-three, e-five）。星は写真自身の星。
    assert_eq!(
        targets.iter().map(|(_id, path, rating)| (path.as_str(), *rating)).collect::<Vec<_>>(),
        vec![("C:/photos/c-three.jpg", 3), ("C:/photos/e-five.jpg", 5)]
    );
    assert!(photos_for_export(&conn, "p1", &[]).expect("empty").is_empty(), "id が空なら何も書き出さない");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn moved_photo_leaves_the_star_counts() {
    let directory = test_directory("move-count");
    let conn = rated_fixture(&directory.join("m.sqlite3"));
    let before = selection_summary(&conn, "p1").expect("before");
    mark_photos_missing(&conn, "p1", &["three-b".to_string()]).expect("mark");
    let after = selection_summary(&conn, "p1").expect("after");
    assert_eq!(after.total, before.total - 1, "移動した 1 枚だけ減る");
    assert_eq!(after.counts.iter().sum::<i64>(), after.total);
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn moving_one_photo_marks_only_that_photo_missing() {
    let directory = test_directory("move-missing");
    let conn = rated_fixture(&directory.join("m.sqlite3"));

    mark_photos_missing(&conn, "p1", &["three-b".to_string()]).expect("mark");

    let mut statement = conn
        .prepare("SELECT id FROM photos WHERE project_id='p1' AND is_missing=1")
        .expect("prepare");
    let missing: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("collect");
    assert_eq!(missing, vec!["three-b".to_string()], "移動した 1 枚だけが欠損");

    mark_photos_missing(&conn, "p1", &[]).expect("mark none");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos WHERE is_missing=1", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 1, "何も移動しなければ増えない");

    drop(statement);
    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn analysis_backlog_counts_only_photos_the_analysis_targets() {
    let directory = test_directory("backlog");
    let conn = open_database(&directory.join("b.sqlite3")).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
             VALUES ('p1','p1','C:/photos',4,'ready',1,1)",
        [],
    )
    .expect("insert project");
    // a1 と a2 は 1 秒差（連写の候補）。b1 と b2 は隣と 1 時間以上離れている（候補ではない）。
    for (id, at) in [("a1", 1_000_000_i64), ("a2", 1_001_000), ("b1", 9_000_000), ("b2", 20_000_000)] {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,timestamp_source,is_missing)
                 VALUES (?1,'p1',?2,?1,?1,?3,'exif_original',0)",
            params![id, format!("C:/photos/{id}.jpg"), at],
        )
        .expect("insert photo");
    }
    assert_eq!(analysis_backlog(&conn, "p1", false).expect("backlog"), 2, "候補の 2 枚だけが未処理");

    conn.execute(
        "UPDATE photos SET d_hash='0000000000000000',d_hash_version=?1,thumbnail_path='C:/t.jpg',thumbnail_version=?2
             WHERE id IN ('a1','a2')",
        params![D_HASH_VERSION, THUMBNAIL_VERSION],
    )
    .expect("analyse candidates");
    assert_eq!(
        analysis_backlog(&conn, "p1", false).expect("backlog"),
        0,
        "候補でない写真が空のままでも 0 になる"
    );

    conn.execute("UPDATE photos SET timestamp_source=NULL WHERE id='b1'", [])
        .expect("unread time");
    assert_eq!(analysis_backlog(&conn, "p1", false).expect("backlog"), 1, "撮影時刻が未読の写真は数える");

    drop(conn);
    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn unique_destination_never_overwrites() {
    let directory = test_directory("unique-dest");
    fs::write(directory.join("a.jpg"), b"first").expect("write");
    let next = unique_destination(&directory, "a.jpg");
    assert_eq!(next.file_name().unwrap().to_str().unwrap(), "a (2).jpg");

    fs::write(&next, b"second").expect("write second");
    let third = unique_destination(&directory, "a.jpg");
    assert_eq!(third.file_name().unwrap().to_str().unwrap(), "a (3).jpg");
    // 既にあるファイルは書き換えられていない。
    assert_eq!(fs::read(directory.join("a.jpg")).unwrap(), b"first");

    fs::remove_dir_all(&directory).expect("remove test directory");
}

// ---- JPEG への星の書き込み ------------------------------------------

/// 最小構成の JPEG。SOI + APP1(EXIF) + SOS + 画像データ + EOI。
fn tiny_jpeg_with_exif() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    let exif_payload = b"Exif\0\0dummy-exif-payload";
    bytes.push(0xFF);
    bytes.push(0xE1);
    bytes.extend_from_slice(&((exif_payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(exif_payload);
    bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]); // SOS
    bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // 画像データのつもり
    bytes.extend_from_slice(&[0xFF, 0xD9]); // EOI
    bytes
}

fn xmp_ratings_in(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    text.match_indices("xmp:Rating=\"")
        .map(|(index, _)| {
            let rest = &text[index + 12..];
            rest.chars().take_while(|c| *c != '"').collect::<String>()
        })
        .collect()
}

#[test]
fn writing_a_rating_keeps_the_image_data_and_exif() {
    let original = tiny_jpeg_with_exif();
    let updated = jpeg_with_rating(&original, 4).expect("write rating");

    assert_eq!(xmp_ratings_in(&updated), vec!["4"], "星が1つだけ入る");
    assert!(
        updated.windows(4).any(|w| w == b"Exif"),
        "EXIF セグメントを消さない"
    );
    // SOS 以降（画像本体）が一致すること。ここが変わったら画像が壊れる。
    let tail = |bytes: &[u8]| {
        let position = bytes
            .windows(2)
            .position(|w| w == [0xFF, 0xDA])
            .expect("SOS");
        bytes[position..].to_vec()
    };
    assert_eq!(
        tail(&updated),
        tail(&original),
        "画像本体は 1 バイトも変えない"
    );
    assert_eq!(&updated[updated.len() - 2..], &[0xFF, 0xD9], "EOI で終わる");
}

#[test]
fn writing_a_rating_twice_replaces_instead_of_appending() {
    let original = tiny_jpeg_with_exif();
    let once = jpeg_with_rating(&original, 2).expect("first");
    let twice = jpeg_with_rating(&once, 5).expect("second");

    // 何度書いても XMP は1つ。追記され続けるとファイルが際限なく育つ。
    assert_eq!(xmp_ratings_in(&twice), vec!["5"]);
    assert_eq!(
        twice.iter().filter(|b| **b == 0xE1).count(),
        once.iter().filter(|b| **b == 0xE1).count(),
        "APP1 の数が増えない"
    );
}

#[test]
fn rating_zero_is_written_as_zero() {
    // 「0 だったものはメタデータも 0」。xmp:Rating は 0 を持てる。
    let updated = jpeg_with_rating(&tiny_jpeg_with_exif(), 0).expect("write zero");
    assert_eq!(xmp_ratings_in(&updated), vec!["0"]);
}

/// 実写真に対する往復。原本ではなくコピーに対して行う。
/// 環境変数 PHOTO_CURATOR_SAMPLE_JPEG に1枚のパスを渡すと実行される。
#[test]
fn writing_a_rating_to_a_real_photo_keeps_it_readable() {
    let Ok(source) = std::env::var("PHOTO_CURATOR_SAMPLE_JPEG") else {
        eprintln!("PHOTO_CURATOR_SAMPLE_JPEG が未設定のため skip");
        return;
    };
    let original = fs::read(&source).expect("read sample");
    let before = image::ImageReader::open(&source)
        .expect("open sample")
        .into_dimensions()
        .expect("read dimensions");

    let updated = jpeg_with_rating(&original, 4).expect("write rating");

    let directory = test_directory("real-jpeg");
    let target = directory.join("written.jpg");
    fs::write(&target, &updated).expect("write updated");

    let after = image::ImageReader::open(&target)
        .expect("open updated")
        .into_dimensions()
        .expect("read updated dimensions");
    assert_eq!(before, after, "画像の寸法が変わらない");
    assert_eq!(xmp_ratings_in(&updated), vec!["4"]);
    // EXIF が残っていれば撮影時刻も読めるはず。
    assert!(read_capture_time(&target).is_some(), "撮影時刻を読み直せる");
    eprintln!(
        "実写真: {}x{} / {} bytes → {} bytes",
        before.0,
        before.1,
        original.len(),
        updated.len()
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn broken_input_is_rejected_before_touching_anything() {
    assert!(jpeg_with_rating(b"not a jpeg at all", 3).is_err());
    // SOI はあるが長さが壊れている。
    let broken = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFE, 0x00];
    assert!(jpeg_with_rating(&broken, 3).is_err());
}

/// U2: 星を書いた一時ファイル（拡張子 `.photocurator-tmp`）を、画像として確かめられること。
/// 拡張子で形式を決めていた頃は、ここが毎回失敗して「メタデータに反映」が全部取り消されていた。
#[test]
fn written_temporary_file_is_verified_by_its_content() {
    let directory = test_directory("verify-tmp");
    let updated = jpeg_with_rating(&jpeg_bytes(32, 24, 7), 3).expect("write rating");
    let temporary = directory.join("IMG_0001.photocurator-tmp");
    fs::write(&temporary, &updated).expect("write temporary");
    assert!(verify_image_file(&temporary).is_ok(), "拡張子に関係なく中身で確かめる");

    let broken = directory.join("broken.photocurator-tmp");
    fs::write(&broken, b"not an image").expect("write broken");
    assert!(verify_image_file(&broken).is_err(), "壊れた中身は確かめで落ちる");
    let _ = fs::remove_dir_all(&directory);
}

// ---- Amazon（T9）----

fn amazon_node(id: &str, name: &str, date: Option<&str>, size: u64) -> amazon::AmazonNode {
    amazon::AmazonNode {
        id: id.into(),
        name: name.into(),
        kind: "FILE".into(),
        content_properties: Some(amazon::ContentProperties {
            content_type: Some("image/jpeg".into()),
            content_date: date.map(str::to_owned),
            size: Some(size),
        }),
        temp_link: Some(format!("https://content.example/{id}")),
    }
}

fn amazon_test_db(label: &str) -> (PathBuf, Connection) {
    let directory = test_directory(label);
    fs::create_dir_all(&directory).expect("create test directory");
    let database = directory.join("db.sqlite3");
    let conn = open_database(&database).expect("open database");
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at,source_kind,source_key)
             VALUES ('p1','x','https://www.amazon.co.jp/photos/share/abc',0,'new',0,0,'amazon','www.amazon.co.jp|abc')",
        [],
    )
    .expect("insert project");
    (directory, conn)
}

#[test]
fn a_legacy_project_becomes_a_folder_project_and_amazon_tables_exist() {
    let directory = test_directory("amazon-migrate");
    fs::create_dir_all(&directory).expect("create test directory");
    let path = directory.join("legacy.sqlite3");
    let legacy = Connection::open(&path).expect("open legacy database");
    legacy
        .execute_batch(
            "CREATE TABLE projects (
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL,
                   photo_count INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'new',
                   created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 INSERT INTO projects (id,name,folder_path,created_at,updated_at) VALUES ('old','旧','/photos',0,0);",
        )
        .expect("create legacy schema");
    drop(legacy);
    let conn = open_database(&path).expect("migrate");
    let (kind, key) = project_source(&conn, "old").expect("source");
    assert_eq!(kind, "folder");
    assert_eq!(key, None);
    assert!(amazon_source_of(&conn, "old").unwrap().is_none());
    amazon::save_links(&conn, "old", &[("n".into(), "l".into())]).expect("amazon_links exists");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn an_amazon_scan_row_uses_the_node_id_and_the_local_clock() {
    let (directory, conn) = amazon_test_db("amazon-scan");
    upsert_amazon_photo(&conn, "p1", &amazon_node("n1", "IMG_1.jpg", Some("2021-07-23T13:13:29.000Z"), 100))
        .expect("upsert");
    let (path, relative, name, captured, source): (String, String, String, Option<i64>, String) = conn
        .query_row(
            "SELECT path,relative_path,name,captured_at,timestamp_source FROM photos WHERE project_id='p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("row");
    assert_eq!((path.as_str(), relative.as_str(), name.as_str()), ("n1", "n1", "IMG_1.jpg"));
    // Z を信じず、その土地の時計のまま。
    assert_eq!(captured, civil_timestamp_ms(2021, 7, 23, 13, 13, 29));
    assert_eq!(source, "exif_original");

    // 日付が無ければ、ファイル名から。それも無ければ空。
    upsert_amazon_photo(&conn, "p1", &amazon_node("n2", "IMG_20260630_181932.jpg", None, 1)).unwrap();
    upsert_amazon_photo(&conn, "p1", &amazon_node("n3", "photo.jpg", None, 1)).unwrap();
    let read = |id: &str| -> (Option<i64>, String) {
        conn.query_row(
            "SELECT captured_at,timestamp_source FROM photos WHERE path=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(read("n2"), (civil_timestamp_ms(2026, 6, 30, 18, 19, 32), "filename_inferred".to_string()));
    assert_eq!(read("n3"), (None, "unknown".to_string()));
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_rescan_keeps_the_analysis_only_while_the_size_is_unchanged() {
    let (directory, conn) = amazon_test_db("amazon-rescan");
    let node = amazon_node("n1", "a.jpg", Some("2021-07-23T13:13:29.000Z"), 100);
    upsert_amazon_photo(&conn, "p1", &node).unwrap();
    conn.execute(
        "UPDATE photos SET rating=3,d_hash='00ff',d_hash_version=?1,thumbnail_path='/t.jpg',thumbnail_version=?2,display_path='/d.jpg',display_edge=1024",
        params![D_HASH_VERSION, THUMBNAIL_VERSION],
    )
    .unwrap();
    upsert_amazon_photo(&conn, "p1", &node).unwrap();
    let (rating, hash, thumb, display): (i64, Option<String>, Option<String>, Option<String>) = conn
        .query_row("SELECT rating,d_hash,thumbnail_path,display_path FROM photos", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    assert_eq!((rating, hash.as_deref(), thumb.as_deref(), display.as_deref()), (3, Some("00ff"), Some("/t.jpg"), Some("/d.jpg")));

    upsert_amazon_photo(&conn, "p1", &amazon_node("n1", "a.jpg", Some("2021-07-23T13:13:29.000Z"), 999)).unwrap();
    let (rating, hash, thumb, display): (i64, Option<String>, Option<String>, Option<String>) = conn
        .query_row("SELECT rating,d_hash,thumbnail_path,display_path FROM photos", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    // 星は残り、絵とハッシュ値は作り直しになる。
    assert_eq!((rating, hash, thumb, display), (3, None, None, None));
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn an_amazon_thumbnail_that_is_already_there_needs_no_network() {
    let (directory, conn) = amazon_test_db("amazon-hash");
    let source = amazon::parse_key("www.amazon.co.jp|abc").unwrap();
    let book = amazon::LinkBook::load(directory.join("db.sqlite3"), &conn, "p1", source).unwrap();
    let thumbnails = directory.join("thumbnails");
    fs::create_dir_all(&thumbnails).unwrap();
    let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([(x * 4) as u8, (y * 5) as u8, 90])));
    let bytes = encode_thumbnail(&image).unwrap();
    let file = thumbnail_file(&thumbnails, "photo-1");
    fs::write(&file, &bytes).unwrap();
    let expected = hash_thumbnail_bytes(&bytes);
    assert!(expected.is_some());
    let record = HashRecord {
        id: "photo-1".into(),
        path: "n1".into(),
        captured_at: 0,
        source: TimestampSource::ExifOriginal,
        cached: CachedAnalysis {
            d_hash: expected.clone(),
            d_hash_version: Some(D_HASH_VERSION),
            thumbnail_path: Some(file.to_string_lossy().to_string()),
            thumbnail_mtime: None,
            thumbnail_size: None,
            thumbnail_version: Some(THUMBNAIL_VERSION),
        },
    };
    // 網が無い（tempLink も無い）ので、取りにいけば失敗する。それでも成功する = 使い回した。
    let reused = hash_one_amazon(&book, &thumbnails, 0, &record);
    assert!(reused.error.is_none() && reused.hash_reused);
    assert_eq!(reused.d_hash, expected);

    // 版が古いハッシュ値は、サムネイルから作り直す（網は使わない）。
    let mut old = record.clone();
    old.cached.d_hash_version = Some(D_HASH_VERSION - 1);
    let rebuilt = hash_one_amazon(&book, &thumbnails, 0, &old);
    assert!(rebuilt.error.is_none() && !rebuilt.hash_reused);
    assert_eq!(rebuilt.d_hash, expected);
    fs::remove_dir_all(&directory).ok();
}
// ---- 組の RAW の .xmp（U47）-----------------------------------------

fn rating_target(path: &Path, rating: i64) -> (String, String, i64) {
    ("id".to_string(), path.to_string_lossy().to_string(), rating)
}

fn jpeg_ratings(path: &Path) -> Vec<String> {
    xmp_ratings_in(&fs::read(path).expect("read jpeg"))
}

const RAW_BYTES: &[u8] = b"II*\0 not a real raw but must never change \xFF\xD8\x00";

#[test]
fn rating_a_jpeg_also_writes_the_xmp_next_to_its_paired_raw() {
    let directory = test_directory("u47-basic");
    let jpeg = directory.join("IMG_1.JPG");
    let raw = directory.join("IMG_1.CR2");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(&raw, RAW_BYTES).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 3)], true);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 1, 0));
    assert_eq!(jpeg_ratings(&jpeg), vec!["3"]);
    let xmp = fs::read_to_string(directory.join("IMG_1.xmp")).expect("xmp is created");
    assert_eq!(xmp_ratings_of_document(&xmp).unwrap(), vec!["3"]);
    assert_eq!(fs::read(&raw).unwrap(), RAW_BYTES, "RAW 本体は不変");
    let leftovers: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("photocurator-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "一時ファイルを残さない");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn existing_xmp_only_has_its_rating_updated() {
    let directory = test_directory("u47-existing");
    let jpeg = directory.join("IMG_2.jpg");
    let raw = directory.join("IMG_2.CR2");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(&raw, RAW_BYTES).unwrap();
    let existing = "<?xpacket begin=\"\" id=\"x\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" xmp:Rating=\"1\" crs:Exposure2012=\"+0.50\">\n   <crs:ToneCurve><rdf:Seq><rdf:li>0, 0</rdf:li></rdf:Seq></crs:ToneCurve>\n  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>";
    fs::write(directory.join("IMG_2.xmp"), existing).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 5)], true);

    assert_eq!((report.paired_raw_processed, report.failed), (1, 0));
    let after = fs::read_to_string(directory.join("IMG_2.xmp")).unwrap();
    assert_eq!(after, existing.replace("xmp:Rating=\"1\"", "xmp:Rating=\"5\""), "Rating 以外は 1 文字も変わらない");
    assert!(after.contains("crs:Exposure2012=\"+0.50\"") && after.contains("<rdf:li>0, 0</rdf:li>"));
    assert_eq!(fs::read(&raw).unwrap(), RAW_BYTES);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn existing_xmp_with_rating_element_or_without_rating_is_handled() {
    let element = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"r\"><rdf:Description xmlns:xmp=\"n\"><xmp:Rating>2</xmp:Rating><a:b xmlns:a=\"a\">keep</a:b></rdf:Description></rdf:RDF></x:xmpmeta>";
    let updated = xmp_with_rating(element, 4).unwrap();
    assert_eq!(updated, element.replace("<xmp:Rating>2<", "<xmp:Rating>4<"));

    let none = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"r\"><rdf:Description rdf:about=\"\" xmlns:crs=\"c\" crs:Exposure=\"1\"/></rdf:RDF></x:xmpmeta>";
    let added = xmp_with_rating(none, 3).unwrap();
    assert_eq!(xmp_ratings_of_document(&added).unwrap(), vec!["3"]);
    assert!(added.contains("crs:Exposure=\"1\"") && added.contains("xmlns:xmp="));

    // 読めない XML・Description の無い XML は Err（何も書かせない）。
    assert!(xmp_with_rating("<a><b></a>", 1).is_err());
    assert!(xmp_with_rating("not xml", 1).is_err());
    assert!(xmp_with_rating("<a/>", 1).is_err());
    // 値の中の `>` や、コメントの中の偽のタグに惑わされない。
    let tricky = "<x><!-- <xmp:Rating>9</xmp:Rating> --><rdf:Description a=\"1>2\" xmp:Rating=\"1\"/></x>";
    assert_eq!(xmp_with_rating(tricky, 2).unwrap(), tricky.replace("Rating=\"1\"", "Rating=\"2\""));
}

#[test]
fn a_jpeg_without_a_paired_raw_creates_no_xmp() {
    let directory = test_directory("u47-nopair");
    let jpeg = directory.join("IMG_3.jpg");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(directory.join("IMG_30.CR2"), RAW_BYTES).unwrap();
    fs::write(directory.join("IMG_3.png"), b"x").unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 2)], true);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 0, 0));
    assert!(!directory.join("IMG_3.xmp").exists());
    assert!(!directory.join("IMG_30.xmp").exists());
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_raw_in_another_folder_is_not_a_pair() {
    let directory = test_directory("u47-folders");
    fs::create_dir_all(directory.join("a")).unwrap();
    fs::create_dir_all(directory.join("b")).unwrap();
    let jpeg = directory.join("a").join("IMG_4.jpg");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(directory.join("b").join("IMG_4.CR2"), RAW_BYTES).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 2)], true);

    assert_eq!(report.paired_raw_processed, 0);
    assert!(!directory.join("b").join("IMG_4.xmp").exists());
    assert!(!directory.join("a").join("IMG_4.xmp").exists());

    // 純関数の側でも同じ。
    let found = paired_raw_files(
        &jpeg,
        &[directory.join("b").join("IMG_4.CR2"), directory.join("a").join("IMG_4.CR2"), directory.join("a").join("IMG_4.png")],
    );
    assert_eq!(found, vec![directory.join("a").join("IMG_4.CR2")]);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn pairing_ignores_case_of_names_and_extensions() {
    let directory = test_directory("u47-case");
    let jpeg = directory.join("img_5.jpg");
    let raw = directory.join("IMG_5.CR2");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(&raw, RAW_BYTES).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 4)], true);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 1, 0));
    let xmp = fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.to_lowercase().ends_with(".xmp"))
        .collect::<Vec<_>>();
    assert_eq!(xmp.len(), 1, "大文字小文字違いの .xmp を 2 つ作らない");
    assert_eq!(fs::read(&raw).unwrap(), RAW_BYTES);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn nothing_is_written_next_to_raw_when_pairing_is_off() {
    let directory = test_directory("u47-off");
    let jpeg = directory.join("IMG_6.jpg");
    let raw = directory.join("IMG_6.CR2");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(&raw, RAW_BYTES).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 3)], false);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 0, 0));
    assert!(!directory.join("IMG_6.xmp").exists());
    // RAW が自分の行で渡されても、今までどおり飛ばす（RAW は書き換えない）。
    let report = write_ratings_to_targets(&[rating_target(&raw, 3)], false);
    assert_eq!((report.processed, report.skipped), (0, 1));
    assert_eq!(fs::read(&raw).unwrap(), RAW_BYTES);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn zero_stars_writes_rating_zero() {
    let directory = test_directory("u47-zero");
    let jpeg = directory.join("IMG_7.jpg");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(directory.join("IMG_7.NEF"), RAW_BYTES).unwrap();
    fs::write(directory.join("IMG_7.xmp"), new_sidecar_xmp(4)).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 0)], true);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 1, 0));
    let xmp = fs::read_to_string(directory.join("IMG_7.xmp")).unwrap();
    assert_eq!(xmp_ratings_of_document(&xmp).unwrap(), vec!["0"]);
    assert_eq!(jpeg_ratings(&jpeg), vec!["0"]);
    assert!(new_sidecar_xmp(0).contains("xmp:Rating=\"0\""));
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_failing_xmp_is_reported_while_the_jpeg_write_still_counts() {
    let directory = test_directory("u47-fail");
    let jpeg = directory.join("IMG_8.jpg");
    fs::write(&jpeg, jpeg_bytes(32, 24, 7)).unwrap();
    fs::write(directory.join("IMG_8.CR2"), RAW_BYTES).unwrap();
    let xmp = directory.join("IMG_8.xmp");
    let original_xmp = new_sidecar_xmp(1);
    fs::write(&xmp, &original_xmp).unwrap();
    let mut permissions = fs::metadata(&xmp).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&xmp, permissions).unwrap();

    let report = write_ratings_to_targets(&[rating_target(&jpeg, 5)], true);

    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 0, 1));
    assert!(report.errors[0].contains("IMG_8.CR2"));
    assert_eq!(jpeg_ratings(&jpeg), vec!["5"], "JPEG の書き込みは成功のまま");
    assert_eq!(fs::read_to_string(&xmp).unwrap(), original_xmp, "既存の .xmp は変わらない");

    let mut permissions = fs::metadata(&xmp).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(&xmp, permissions).unwrap();

    // 壊れた .xmp も触らない。
    fs::write(&xmp, "<broken><x></broken>").unwrap();
    let report = write_ratings_to_targets(&[rating_target(&jpeg, 2)], true);
    assert_eq!((report.processed, report.paired_raw_processed, report.failed), (1, 0, 1));
    assert_eq!(fs::read_to_string(&xmp).unwrap(), "<broken><x></broken>");
    fs::remove_dir_all(&directory).ok();
}

/// RAW（`image` で開けない）は、EXIF のサムネイルが取れなかったら、本体を丸ごと読まずに諦める。
#[test]
fn a_raw_without_a_thumbnail_gives_up_without_reading_the_whole_file() {
    let source = CountingSource::new(vec![0u8; 4 * 1024 * 1024], "IMG_0001.CR2");
    assert!(decode_hash_source_with(&source, false).is_none());
    assert_eq!(source.all_calls.get(), 0, "RAW の本体を丸ごと読んでいる");
    assert!(source.served.get() <= EXIF_HEAD_PROBE);
}

/// 解析できなかった件数は、DB の行を数える。前回までの失敗を、今回の失敗として二重に足さない。
#[test]
fn failed_photo_count_counts_rows_once() {
    let conn = Connection::open_in_memory().expect("open");
    conn.execute_batch(
        "CREATE TABLE photos (project_id TEXT, is_missing INTEGER, analysis_error TEXT);
         INSERT INTO photos VALUES ('p',0,'x'),('p',0,NULL),('p',1,'x'),('q',0,'x');",
    )
    .expect("seed");
    assert_eq!(failed_photo_count(&conn, "p").expect("count"), 1);
}

// -----------------------------------------------------------------------
// U57: RAW に埋め込まれたプレビュー JPEG
// -----------------------------------------------------------------------

/// 小さな TIFF を手で組む入れ物。
struct TiffBuf {
    d: Vec<u8>,
    little: bool,
}

impl TiffBuf {
    fn new(size: usize, little: bool) -> Self {
        let mut t = Self { d: vec![0; size], little };
        t.d[0..2].copy_from_slice(if little { b"II" } else { b"MM" });
        t.u16(2, 42);
        t
    }
    fn u16(&mut self, at: usize, v: u16) {
        let b = if self.little { v.to_le_bytes() } else { v.to_be_bytes() };
        self.d[at..at + 2].copy_from_slice(&b);
    }
    fn u32(&mut self, at: usize, v: u32) {
        let b = if self.little { v.to_le_bytes() } else { v.to_be_bytes() };
        self.d[at..at + 4].copy_from_slice(&b);
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.d[at..at + bytes.len()].copy_from_slice(bytes);
    }
    /// (tag, type, count, value)。type 3（SHORT）の値は先頭 2 バイトに置く。
    fn ifd(&mut self, at: usize, entries: &[(u16, u16, u32, u32)], next: u32) {
        self.u16(at, entries.len() as u16);
        for (i, (tag, ty, n, value)) in entries.iter().enumerate() {
            let e = at + 2 + i * 12;
            self.u16(e, *tag);
            self.u16(e + 2, *ty);
            self.u32(e + 4, *n);
            if *ty == 3 {
                self.u16(e + 8, *value as u16);
            } else {
                self.u32(e + 8, *value);
            }
        }
        self.u32(at + 2 + entries.len() * 12, next);
    }
    fn first_ifd(&mut self, at: u32) {
        self.u32(4, at);
    }
}

/// SOF だけが正しい、デコードはできない JPEG 風のバイト列。
fn fake_jpeg(width: u16, height: u16, len: usize, sof: u8) -> Vec<u8> {
    let mut v = vec![0u8; len];
    v[0..2].copy_from_slice(&[0xFF, 0xD8]);
    v[2..6].copy_from_slice(&[0xFF, 0xE0, 0x00, 0x10]); // APP0 16 バイト
    v[6 + 14..6 + 14 + 4].copy_from_slice(&[0xFF, sof, 0x00, 0x11]);
    let s = 6 + 14 + 4; // SOF の中身
    v[s] = 8;
    v[s + 1..s + 3].copy_from_slice(&height.to_be_bytes());
    v[s + 3..s + 5].copy_from_slice(&width.to_be_bytes());
    v[s + 5] = 3;
    v[len - 2..].copy_from_slice(&[0xFF, 0xD9]);
    v
}

/// 実際にデコードできる JPEG。
fn real_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, ((x ^ y) & 0xFF) as u8])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .expect("encode");
    out
}

/// CR2 風: IFD0（向き・JPEG 圧縮 1 本の strip）→ IFD1（JPEGInterchangeFormat のサムネイル）。
/// `big` を IFD0 の strip（大きいプレビュー）、`small` を IFD1 のサムネイルとして `total` バイトの中に置く。
fn cr2_like(big: &[u8], small: &[u8], big_at: usize, small_at: usize, total: usize, orientation: u16) -> Vec<u8> {
    let mut t = TiffBuf::new(total, true);
    t.first_ifd(8);
    t.ifd(
        8,
        &[
            (0x0103, 3, 1, 6),
            (0x0111, 4, 1, big_at as u32),
            (0x0112, 3, 1, orientation as u32),
            (0x0117, 4, 1, big.len() as u32),
        ],
        100,
    );
    t.ifd(
        100,
        &[(0x0201, 4, 1, small_at as u32), (0x0202, 4, 1, small.len() as u32)],
        0,
    );
    t.put(big_at, big);
    t.put(small_at, small);
    t.d
}

fn raw_info_of(bytes: &[u8]) -> RawInfo {
    inspect(&SliceSource(bytes))
}

#[test]
fn raw_preview_picks_the_largest_jpeg_in_a_cr2_like_file() {
    let big = fake_jpeg(1620, 1080, 3000, 0xC0);
    let small = fake_jpeg(160, 120, 800, 0xC0);
    let bytes = cr2_like(&big, &small, 1000, 5000, 8000, 6);
    let info = raw_info_of(&bytes);
    assert_eq!(info.previews.len(), 2);
    assert_eq!(info.orientation, Some(6));
    let top = largest(&info).expect("largest");
    assert_eq!((top.width, top.height, top.offset, top.length), (1620, 1080, 1000, 3000));
    // サムネイル用: 長辺 256 以上で最小 → 大きい方。100 以上なら小さい方。
    assert_eq!(for_thumb(&info, 256).unwrap().width, 1620);
    assert_eq!(for_thumb(&info, 100).unwrap().width, 160);
    // 足りるものが無ければ一番大きいもの。
    assert_eq!(for_thumb(&info, 5000).unwrap().width, 1620);
    assert_eq!(bytes_of(&SliceSource(&bytes), &top).unwrap(), big);
}

#[test]
fn raw_preview_follows_sub_ifds_in_a_big_endian_file() {
    // NEF 風: IFD0 に SubIFD（0x014A）が 2 つ。片方に大きい JPEG、もう片方に可逆（C3）の本体。
    let mut t = TiffBuf::new(9000, false);
    t.first_ifd(8);
    t.ifd(8, &[(0x0112, 3, 1, 1), (0x014A, 4, 2, 200)], 0);
    t.u32(200, 300);
    t.u32(204, 400);
    t.ifd(300, &[(0x0201, 4, 1, 1000), (0x0202, 4, 1, 4000)], 0);
    t.ifd(400, &[(0x0201, 4, 1, 6000), (0x0202, 4, 1, 2000)], 0);
    t.put(1000, &fake_jpeg(4000, 3000, 4000, 0xC2)); // プログレッシブ
    t.put(6000, &fake_jpeg(6000, 4000, 2000, 0xC3)); // 可逆 = RAW 本体
    let info = raw_info_of(&t.d);
    assert_eq!(info.previews.len(), 1, "可逆 JPEG（RAW 本体）は数えない");
    assert_eq!(largest(&info).unwrap().width, 4000);
    assert_eq!(info.orientation, Some(1));
}

#[test]
fn raw_preview_is_empty_without_a_jpeg() {
    let mut t = TiffBuf::new(2000, true);
    t.first_ifd(8);
    t.ifd(8, &[(0x0103, 3, 1, 7), (0x0111, 4, 1, 500), (0x0112, 3, 1, 1), (0x0117, 4, 1, 800)], 0);
    t.put(500, &fake_jpeg(6000, 4000, 800, 0xC3));
    let info = raw_info_of(&t.d);
    assert!(info.previews.is_empty());
    assert!(largest(&info).is_none());
    // 見出しが違う・短い・空。
    assert!(raw_info_of(b"not a raw file at all, just text").previews.is_empty());
    assert!(raw_info_of(&[]).previews.is_empty());
    assert!(raw_info_of(&[0x49, 0x49]).previews.is_empty());
}

#[test]
fn raw_preview_survives_broken_offsets_and_loops() {
    let big = fake_jpeg(1620, 1080, 3000, 0xC0);
    let mut bytes = cr2_like(&big, &fake_jpeg(160, 120, 800, 0xC0), 1000, 5000, 8000, 1);
    // 範囲外のオフセット・長さ（IFD1 の JPEG）。IFD0 の方だけが残る。
    let mut t = TiffBuf { d: bytes.clone(), little: true };
    t.ifd(100, &[(0x0201, 4, 1, 7_900), (0x0202, 4, 1, 4_000_000_000)], 0);
    assert_eq!(raw_info_of(&t.d).previews.len(), 1, "範囲外の方だけ捨てる");
    // IFD の鎖がループ（自分自身を指す）。
    let mut t = TiffBuf { d: bytes.clone(), little: true };
    t.ifd(100, &[(0x0201, 4, 1, 5000), (0x0202, 4, 1, 800)], 100);
    t.ifd(8, &[(0x0112, 3, 1, 1)], 100);
    assert!(raw_info_of(&t.d).previews.len() <= 1);
    // SubIFD が自分を指す・IFD0 の次が IFD0。
    let mut t = TiffBuf { d: bytes.clone(), little: true };
    t.ifd(8, &[(0x014A, 4, 1, 8)], 8);
    let _ = raw_info_of(&t.d);
    // 最初の IFD が範囲外（u32 の上限）。
    t.first_ifd(0xFFFF_FFFF);
    assert!(raw_info_of(&t.d).previews.is_empty());
    // どの 1 バイトを壊しても、途中で切っても panic しない。
    for i in 0..bytes.len().min(400) {
        let saved = bytes[i];
        for value in [0u8, 0xFF, 0x7F] {
            bytes[i] = value;
            let _ = raw_info_of(&bytes);
        }
        bytes[i] = saved;
    }
    for cut in [0, 1, 7, 8, 9, 50, 99, 101, 130, 1500, 4000, 7999] {
        let _ = raw_info_of(&bytes[..cut]);
    }
}

#[test]
fn raw_preview_reads_the_raf_header() {
    let jpeg = fake_jpeg(1920, 1280, 5000, 0xC0);
    let mut bytes = vec![0u8; 12000];
    bytes[..15].copy_from_slice(b"FUJIFILMCCD-RAW");
    bytes[84..88].copy_from_slice(&2000u32.to_be_bytes());
    bytes[88..92].copy_from_slice(&5000u32.to_be_bytes());
    bytes[2000..7000].copy_from_slice(&jpeg);
    let info = raw_info_of(&bytes);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.orientation, None);
    assert_eq!(largest(&info).unwrap().length, 5000);
    // 見出しが範囲外を指すなら空。
    bytes[84..88].copy_from_slice(&900_000u32.to_be_bytes());
    assert!(raw_info_of(&bytes).previews.is_empty());
}

fn hex16(text: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}

fn iso_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

/// CR3 風: ftyp → moov（uuid(CMT) → CMT1 の TIFF）→ uuid(PRVW) → mdat。
fn cr3_like(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(iso_box(b"ftyp", b"crx \0\0\0\x01crx isom"));
    let mut t = TiffBuf::new(40, true);
    t.first_ifd(8);
    t.ifd(8, &[(0x0112, 3, 1, orientation as u32)], 0);
    let mut uuid_cmt = hex16("85c0b687820f11e08111f4ce462b6a48").to_vec();
    uuid_cmt.extend(iso_box(b"CMT1", &t.d));
    out.extend(iso_box(b"moov", &iso_box(b"uuid", &uuid_cmt)));
    // PRVW: uuid(16) + 詰め物 8 + 箱（サイズ 4・PRVW 4・詰め物 12・JPEG の長さ 4・JPEG）
    let mut body = hex16("eaf42b5e1c984b88b9fbb7dc406e4d16").to_vec();
    body.extend_from_slice(&[0u8; 8]);
    body.extend_from_slice(&((24 + jpeg.len()) as u32).to_be_bytes());
    body.extend_from_slice(b"PRVW");
    body.extend_from_slice(&[0u8; 12]);
    body.extend_from_slice(&(jpeg.len() as u32).to_be_bytes());
    body.extend_from_slice(jpeg);
    out.extend(iso_box(b"uuid", &body));
    out.extend(iso_box(b"mdat", &[0u8; 64]));
    out
}

#[test]
fn raw_preview_reads_the_cr3_prvw_box_and_orientation() {
    let jpeg = fake_jpeg(1620, 1080, 3000, 0xC0);
    let bytes = cr3_like(&jpeg, 8);
    let info = raw_info_of(&bytes);
    assert_eq!(info.previews.len(), 1, "PRVW の JPEG を見つける");
    assert_eq!(info.orientation, Some(8), "向きは moov の CMT1 から");
    let region = largest(&info).unwrap();
    assert_eq!((region.width, region.height), (1620, 1080));
    assert_eq!(bytes_of(&SliceSource(&bytes), &region).unwrap(), jpeg);
    // 途中で切れていたら（JPEG が全部読めない）使わない。
    assert!(raw_info_of(&bytes[..bytes.len() - 100]).previews.is_empty());
    for cut in (0..bytes.len()).step_by(37) {
        let _ = raw_info_of(&bytes[..cut]);
    }
}

#[test]
fn raw_preview_does_not_hand_back_a_truncated_jpeg() {
    let big = fake_jpeg(1620, 1080, 3000, 0xC0);
    let bytes = cr2_like(&big, &fake_jpeg(160, 120, 800, 0xC0), 1000, 5000, 8000, 1);
    let region = largest(&raw_info_of(&bytes)).unwrap();
    // 実際のファイルが JPEG の途中までしか無い。
    assert!(bytes_of(&SliceSource(&bytes[..2000]), &region).is_none());
}

#[test]
fn a_raw_preview_reads_only_the_head_and_the_jpeg_range() {
    // 8MB の「RAW」。大きいプレビューは 5MB 付近、サムネイル用は 6MB 付近に置く。
    let big = real_jpeg(1200, 800);
    let small = real_jpeg(320, 240);
    let total = 8 * 1024 * 1024;
    let bytes = cr2_like(&big, &small, 5 * 1024 * 1024, 6 * 1024 * 1024, total, 1);
    let source = CountingSource::new(bytes, "IMG_0001.CR2");
    let (image, how) = decode_hash_source_with(&source, false).expect("raw preview");
    assert_eq!(how, DecodeSource::RawPreview);
    assert_eq!((image.width(), image.height()), (320, 240), "長辺 256 以上で一番小さいもの");
    assert_eq!(source.all_calls.get(), 0, "RAW の本体を丸ごと読んでいる");
    assert!(source.range_calls.get() >= 1);
    // 先頭 64KB ＋ 各 JPEG の確認の区画（64KB ずつ）＋ JPEG 本体。本体の丸読み（8MB）には遠い。
    let budget = EXIF_HEAD_PROBE + 4 * 64 * 1024 + big.len() + small.len();
    assert!(source.served.get() <= budget, "読んだ量 {} > {}", source.served.get(), budget);
    assert!(source.served.get() < total / 4);
}

#[test]
fn a_raw_preview_gets_the_raw_orientation() {
    let big = real_jpeg(600, 400);
    let bytes = cr2_like(&big, &real_jpeg(160, 120), 3000, 70_000, 80_000, 6);
    let source = CountingSource::new(bytes, "IMG_0002.CR2");
    let (image, _) = decode_hash_source_with(&source, false).expect("raw preview");
    // 向き 6（時計回りに 90 度）→ 縦長になる。
    assert_eq!((image.width(), image.height()), (400, 600));
}

#[test]
fn a_raw_without_a_usable_preview_still_fails() {
    let mut t = TiffBuf::new(5000, true);
    t.first_ifd(8);
    t.ifd(8, &[(0x0112, 3, 1, 1)], 0);
    let source = CountingSource::new(t.d, "IMG_0003.CR2");
    assert!(decode_hash_source_with(&source, false).is_none());
    assert_eq!(source.all_calls.get(), 0);
    // 壊れた JPEG（SOF は正しいが中身がデコードできない）。
    let bytes = cr2_like(&fake_jpeg(1620, 1080, 3000, 0xC0), &fake_jpeg(160, 120, 800, 0xC0), 1000, 5000, 8000, 1);
    let source = CountingSource::new(bytes, "IMG_0004.CR2");
    assert!(decode_hash_source_with(&source, false).is_none());
}

#[test]
fn display_of_a_raw_comes_from_the_largest_preview() {
    let big = real_jpeg(1600, 1000);
    let bytes = cr2_like(&big, &real_jpeg(320, 240), 3000, 200_000, 300_000, 1);
    let source = CountingSource::new(bytes, "IMG_0005.CR2");
    let built = build_display(&source, 1024, None).expect("display");
    let image = image::load_from_memory(&built).expect("decode");
    assert_eq!((image.width(), image.height()), (1024, 640));
    assert_eq!(source.all_calls.get(), 0);
    // 取り出せない RAW は None（表示用を作れない）。
    let broken = CountingSource::new(vec![0u8; 4096], "IMG_0006.NEF");
    assert!(build_display(&broken, 1024, None).is_none());
}

#[test]
fn a_local_file_serves_ranges() {
    let directory = test_directory("read-range");
    let path = directory.join("IMG_0007.CR2");
    let bytes: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&path, &bytes).unwrap();
    let local = LocalPhoto(&path);
    assert_eq!(local.read_range(10, 5).unwrap(), bytes[10..15]);
    assert_eq!(local.read_range(990, 100).unwrap(), bytes[990..], "終わりにかかれば短く返す");
    assert!(local.read_range(1000, 1).is_none(), "ファイルの外");
    assert!(local.read_range(5000, 1).is_none());
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn analysing_a_raw_makes_a_thumbnail_and_a_hash() {
    let directory = test_directory("raw-analyse");
    let path = directory.join("IMG_0008.CR2");
    let bytes = cr2_like(&real_jpeg(1200, 800), &real_jpeg(320, 240), 3000, 90_000, 100_000, 1);
    fs::write(&path, &bytes).unwrap();
    let outcome = analyse_photo(
        &directory.join("thumbs"),
        "p1",
        &path,
        fingerprint(&path),
        &CachedAnalysis::default(),
    );
    assert_eq!(outcome.thumbnail_state, ThumbnailState::Generated(DecodeSource::RawPreview));
    assert!(outcome.d_hash.is_some());
    assert!(outcome.thumbnail_path.is_some());
    fs::remove_dir_all(&directory).ok();
}

// -----------------------------------------------------------------------
// U58: 非対応の形式は、原本が変わるまで再試行しない・数に含めない・選別の対象から外す
// -----------------------------------------------------------------------

/// 読めない `PhotoSource`（先頭が読めない／先頭は読めるが全体が読めない）。
struct FlakySource {
    bytes: Vec<u8>,
    head_ok: bool,
}

impl PhotoSource for FlakySource {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        self.head_ok
            .then(|| self.bytes[..want.min(self.bytes.len())].to_vec())
    }
    fn all(&self) -> Option<Vec<u8>> {
        None
    }
    fn read_range(&self, _offset: u64, _length: usize) -> Option<Vec<u8>> {
        None
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        None
    }
    fn name(&self) -> Option<String> {
        Some("photo.jpg".into())
    }
}

#[test]
fn decode_failures_tell_unreadable_from_undecodable() {
    // 読めたのに復号できない（中身が JPEG でない .jpg）。
    let garbage = CountingSource::new(vec![7u8; 5000], "garbage.jpg");
    assert_eq!(
        try_decode_hash_source_with(&garbage, true).err(),
        Some(DecodeFailure::Undecodable)
    );
    // RAW でプレビューが取れない。
    let raw = CountingSource::new(vec![0u8; 4096], "IMG_0001.CR2");
    assert_eq!(
        try_decode_hash_source_with(&raw, false).err(),
        Some(DecodeFailure::Undecodable)
    );
    // 先頭が読めない。
    let closed = FlakySource { bytes: vec![0; 10], head_ok: false };
    assert_eq!(
        try_decode_hash_source_with(&closed, true).err(),
        Some(DecodeFailure::Unreadable)
    );
    // 先頭は読めたが全体を読む途中で切れた（NAS の瞬断）。
    let cut = FlakySource { bytes: vec![7; 5000], head_ok: true };
    assert_eq!(
        try_decode_hash_source_with(&cut, true).err(),
        Some(DecodeFailure::Unreadable)
    );
}

fn hash_record_of(conn: &Connection, id: &str) -> HashRecord {
    load_hash_records(conn, "project-1", false)
        .expect("records")
        .into_iter()
        .find(|record| record.id == id)
        .expect("record")
}

/// 1 枚を解析して、本番と同じ書き方（`apply_hash`）で DB に書く。
fn analyse_and_store(conn: &Connection, thumbnails: &Path, id: &str) -> PhotoWork {
    let record = hash_record_of(conn, id);
    let work = hash_one(thumbnails, 0, &record);
    apply_hash(conn, &work, false).expect("apply");
    work
}

fn put_photo(conn: &Connection, id: &str, path: &Path, captured_at: i64) {
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,timestamp_source,d_hash,rating,is_missing)
         VALUES (?1,'project-1',?2,?3,?3,?4,'exif',NULL,0,0)",
        params![
            id,
            path.to_string_lossy().to_string(),
            path.file_name().unwrap().to_string_lossy().to_string(),
            captured_at
        ],
    )
    .expect("insert");
}

fn kind_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT analysis_error_kind FROM photos WHERE id=?1",
        params![id],
        |row| row.get(0),
    )
    .expect("kind")
}

#[test]
fn an_unsupported_original_is_not_read_again_until_it_changes() {
    let directory = test_directory("u58-retry");
    let conn = database_with_photos(&directory, 0);
    let thumbnails = directory.join("thumbs");
    let broken = directory.join("broken.jpg"); // 中身が JPEG でない
    let good = directory.join("good.jpg");
    let raw = directory.join("IMG_0001.CR2"); // プレビューの無い RAW
    let missing = directory.join("gone.jpg"); // 開けない
    fs::write(&broken, vec![9u8; 6000]).unwrap();
    fs::write(&good, real_jpeg(320, 240)).unwrap();
    fs::write(&raw, vec![0u8; 8192]).unwrap();
    for (index, (id, path)) in [("broken", &broken), ("good", &good), ("raw", &raw), ("gone", &missing)]
        .into_iter()
        .enumerate()
    {
        put_photo(&conn, id, path, 1_000 + index as i64);
    }

    // 1 回目: 全部読まれる。
    assert_eq!(load_hash_records(&conn, "project-1", true).unwrap().len(), 4);
    for id in ["broken", "good", "raw", "gone"] {
        analyse_and_store(&conn, &thumbnails, id);
    }
    assert_eq!(kind_of(&conn, "broken").as_deref(), Some("unsupported"));
    assert_eq!(kind_of(&conn, "raw").as_deref(), Some("unsupported"));
    assert_eq!(kind_of(&conn, "gone").as_deref(), Some("transient"), "開けないのは一時的");
    assert_eq!(kind_of(&conn, "good"), None);

    // 2 回目: 非対応の 2 枚は対象に入らない（読まない）。一時的な失敗と成功済みは入る。
    let second: Vec<String> = load_hash_records(&conn, "project-1", true)
        .unwrap()
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert!(!second.contains(&"broken".to_string()));
    assert!(!second.contains(&"raw".to_string()));
    assert!(second.contains(&"gone".to_string()), "一時的な失敗は再試行する");
    assert!(second.contains(&"good".to_string()));
    // 撮影時刻の段も同じ。
    conn.execute("UPDATE photos SET captured_at=NULL", []).unwrap();
    let metadata: Vec<String> = load_metadata_records(&conn, "project-1")
        .unwrap()
        .into_iter()
        .map(|record| record.0)
        .collect();
    assert!(!metadata.contains(&"broken".to_string()));
    assert!(metadata.contains(&"gone".to_string()));

    // 原本が変わったら（大きさが違う）もう一度試す。
    fs::write(&broken, vec![9u8; 6001]).unwrap();
    let third: Vec<String> = load_hash_records(&conn, "project-1", false)
        .unwrap()
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert!(third.contains(&"broken".to_string()), "原本が変わったら再試行する");
    assert!(!third.contains(&"raw".to_string()), "変わっていない方は読まない");

    // 直った（本物の JPEG に差し替わった）ら成功して種類が消える。
    fs::write(&broken, real_jpeg(200, 100)).unwrap();
    analyse_and_store(&conn, &thumbnails, "broken");
    assert_eq!(kind_of(&conn, "broken"), None);
    assert!(conn
        .query_row("SELECT d_hash FROM photos WHERE id='broken'", [], |row| row.get::<_, Option<String>>(0))
        .unwrap()
        .is_some());
    drop(conn);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_legacy_failure_without_a_kind_is_retried_once_then_classified() {
    let directory = test_directory("u58-legacy");
    let conn = database_with_photos(&directory, 0);
    let thumbnails = directory.join("thumbs");
    let broken = directory.join("old.jpg");
    fs::write(&broken, vec![1u8; 3000]).unwrap();
    put_photo(&conn, "old", &broken, 5);
    // U58 より前の失敗（理由はあるが種類が無い）。
    conn.execute(
        "UPDATE photos SET analysis_error='画像を読み取れませんでした（破損または非対応の形式）。',analysis_error_at=1,
           fingerprint_mtime=1,fingerprint_size=3000 WHERE id='old'",
        [],
    )
    .unwrap();
    assert_eq!(load_hash_records(&conn, "project-1", true).unwrap().len(), 1, "1 回は試す");
    analyse_and_store(&conn, &thumbnails, "old");
    assert_eq!(kind_of(&conn, "old").as_deref(), Some("unsupported"));
    assert!(load_hash_records(&conn, "project-1", true).unwrap().is_empty(), "分類されたら読まない");
    drop(conn);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn unsupported_rows_stay_in_the_table_but_leave_the_counts() {
    let directory = test_directory("u58-counts");
    let conn = database_with_photos(&directory, 0);
    for (id, index) in [("a", 1_000_000i64), ("b", 1_000_500), ("c", 1_001_000)] {
        conn.execute(
            "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,timestamp_source,rating,is_missing)
             VALUES (?1,'project-1',?2,?1,?1,?3,'exif_original',0,0)",
            params![id, format!("C:/p/{id}.jpg"), index],
        )
        .unwrap();
    }
    conn.execute("INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at) VALUES ('project-1','n','C:/p',3,'ready',0,0)", []).unwrap();
    conn.execute("UPDATE photos SET rating=3 WHERE id='b'", []).unwrap();

    // 3 枚とも未処理（連写の候補になる近さ）。
    let before = analysis_backlog(&conn, "project-1", false).unwrap();
    assert_eq!(before, 3);
    let edge = 1536u32;
    assert_eq!(display_backlog_count(&conn, "project-1", edge).unwrap(), 3);

    // b を非対応にする。
    conn.execute(
        "UPDATE photos SET analysis_error='x',analysis_error_at=1,analysis_error_kind='unsupported' WHERE id='b'",
        [],
    )
    .unwrap();
    assert_eq!(analysis_backlog(&conn, "project-1", false).unwrap(), 2, "準備の分母から外れる");
    assert_eq!(display_backlog_count(&conn, "project-1", edge).unwrap(), 2, "表示用画像も作りに行かない");
    assert_eq!(recount_photos(&conn, "project-1").unwrap(), 2, "枚数から外れる");

    // 行は消えず、★も残る。core に渡す行（get_core_inputs の SELECT）には印付きで載る。
    let rows: Vec<Photo> = conn
        .prepare(&format!(
            "SELECT {PHOTO_COLUMNS} FROM photos WHERE project_id=?1 AND is_missing=0 ORDER BY id"
        ))
        .unwrap()
        .query_map(params!["project-1"], photo_from_row)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 3);
    let b = rows.iter().find(|photo| photo.id == "b").unwrap();
    assert_eq!(b.rating, 3);
    assert_eq!(b.analysis_error_kind.as_deref(), Some("unsupported"));
    assert_eq!(rows.iter().find(|photo| photo.id == "a").unwrap().analysis_error_kind, None);
    drop(conn);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn the_failure_list_has_names_reasons_and_kinds() {
    let directory = test_directory("u58-list");
    let conn = database_with_photos(&directory, 3);
    conn.execute(
        "UPDATE photos SET analysis_error='壊れている',analysis_error_at=5,analysis_error_kind='unsupported' WHERE id='photo-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE photos SET analysis_error='開けない',analysis_error_at=6,analysis_error_kind='transient' WHERE id='photo-0'",
        [],
    )
    .unwrap();
    // 種類が無い古い失敗は一時的として出す。
    conn.execute(
        "UPDATE photos SET analysis_error='古い失敗',analysis_error_at=7 WHERE id='photo-2'",
        [],
    )
    .unwrap();
    // 欠損の行は出さない。
    conn.execute("UPDATE photos SET is_missing=1 WHERE id='photo-2'", []).unwrap();
    let list = analysis_failures(&conn, "project-1").unwrap();
    assert_eq!(
        list,
        vec![
            AnalysisFailure {
                relative_path: "0.jpg".into(),
                name: "0.jpg".into(),
                kind: "transient".into(),
                reason: "開けない".into(),
                at: Some(6),
            },
            AnalysisFailure {
                relative_path: "1.jpg".into(),
                name: "1.jpg".into(),
                kind: "unsupported".into(),
                reason: "壊れている".into(),
                at: Some(5),
            },
        ]
    );
    conn.execute("UPDATE photos SET is_missing=0 WHERE id='photo-2'", []).unwrap();
    let list = analysis_failures(&conn, "project-1").unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].kind, "transient", "種類の無い古い失敗は一時的");
    drop(conn);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_rescan_clears_the_kind_only_when_the_original_changed() {
    let directory = test_directory("u58-rescan");
    let conn = database_with_photos(&directory, 0);
    conn.execute(
        "INSERT INTO photos (id,project_id,path,relative_path,name,captured_at,rating,fingerprint_mtime,fingerprint_size,is_missing,analysis_error,analysis_error_at,analysis_error_kind)
         VALUES ('p','project-1','C:/p/a.jpg','a.jpg','a.jpg',1,0,10,20,0,'x',1,'unsupported')",
        [],
    )
    .unwrap();
    upsert_photo(&conn, "project-1", "C:/p/a.jpg", "a.jpg", "a.jpg", Some(10), Some(20)).unwrap();
    assert_eq!(kind_of(&conn, "p").as_deref(), Some("unsupported"), "変わっていなければ残る");
    upsert_photo(&conn, "project-1", "C:/p/a.jpg", "a.jpg", "a.jpg", Some(11), Some(20)).unwrap();
    assert_eq!(kind_of(&conn, "p"), None, "更新時刻が変わったら消えて再試行になる");
    let error: Option<String> = conn
        .query_row("SELECT analysis_error FROM photos WHERE id='p'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(error, None);
    drop(conn);
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn timeouts_and_unresponsive_workers_are_transient() {
    let mut work = PhotoWork::new(0, "p");
    work.error = Some("解析が 15 秒以内に終わりませんでした。".into());
    assert_eq!(work.error_kind(), Some("transient"), "既定は一時的（迷ったら安全側）");
    work.error_unsupported = true;
    assert_eq!(work.error_kind(), Some("unsupported"));
    assert_eq!(PhotoWork::new(1, "q").error_kind(), None, "失敗が無ければ種類も無い");
}

/// 範囲読みだけ壊せる `PhotoSource`（NAS の瞬断の代わり）。
struct FlakyRange {
    inner: CountingSource,
    mode: u8, // 0=正常 1=読めない 2=半分で切れる
}

impl PhotoSource for FlakyRange {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        self.inner.head(want)
    }
    fn all(&self) -> Option<Vec<u8>> {
        self.inner.all()
    }
    fn read_range(&self, offset: u64, length: usize) -> Option<Vec<u8>> {
        match self.mode {
            1 => None,
            2 => self
                .inner
                .read_range(offset, length)
                .map(|bytes| bytes[..bytes.len() / 2].to_vec()),
            _ => self.inner.read_range(offset, length),
        }
    }
    fn fingerprint(&self) -> Option<(i64, i64)> {
        self.inner.fingerprint()
    }
    fn name(&self) -> Option<String> {
        self.inner.name()
    }
}

#[test]
fn a_raw_range_read_that_fails_midway_is_transient_not_unsupported() {
    let bytes = cr2_like(&real_jpeg(1200, 800), &real_jpeg(320, 240), 3000, 90_000, 100_000, 1);
    let make = |mode| FlakyRange { inner: CountingSource::new(bytes.clone(), "IMG_0100.CR2"), mode };
    // 正常に読めればプレビューが取れる。
    assert!(try_decode_hash_source_with(&make(0), false).is_ok());
    // 範囲読みが途中で読めない・短く返る＝一時的。
    assert_eq!(
        try_decode_hash_source_with(&make(1), false).err(),
        Some(DecodeFailure::Unreadable)
    );
    assert_eq!(
        try_decode_hash_source_with(&make(2), false).err(),
        Some(DecodeFailure::Unreadable)
    );
    // 最後まで正常に読めたのにプレビューが無い＝非対応のまま。
    let none = FlakyRange { inner: CountingSource::new(vec![0u8; 100_000], "IMG_0101.CR2"), mode: 0 };
    assert_eq!(
        try_decode_hash_source_with(&none, false).err(),
        Some(DecodeFailure::Undecodable)
    );
}
