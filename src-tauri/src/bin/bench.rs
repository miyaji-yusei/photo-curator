//! Photo Curator 計測ハーネス（Routine 1 / Step 3）
//!
//! `--features bench` を付けたときだけビルドされる。アプリ本体は一切呼ばず、
//! 逆にアプリ本体からも参照されないため、実行しても製品挙動は変わらない。
//! 計測対象のロジックは `photo_curator_lib::bench_api` から**実物をそのまま**
//! 借りている。再実装すると「計測したコード」と「動くコード」がずれるため。
//!
//! ```text
//! cargo run --release --features bench --bin bench -- real   <photo-dir> <out-dir> [real-db]
//! cargo run --release --features bench --bin bench -- synth  <count> <out-dir>
//! cargo run --release --features bench --bin bench -- decode <photo-dir> <out-dir> [count]
//! ```
//!
//! 写真原本は読み取りのみ。本番 DB も読み取り専用でコピーするだけで、
//! 書き込みは常に一時ディレクトリ上の DB に対して行う。

use image::{imageops::FilterType, DynamicImage, GenericImageView, GrayImage};
use photo_curator_lib::bench_api::{
    analyse_photo, build_burst_groups, capture_time, d_hash, fingerprint, open_database,
    select_burst_candidates, upsert_photo, CachedAnalysis, CandidateInput, DecodeSource,
    ThumbnailState, TimestampSource, D_HASH_VERSION, HASH_DISTANCE_LIMIT,
};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// 小道具
// ---------------------------------------------------------------------------

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// 経過時間つきで閉包を走らせる。
fn timed<T>(body: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = body();
    (value, start.elapsed())
}

/// Windows のピーク working set（MB）。kernel32 の K32GetProcessMemoryInfo は
/// psapi を別途リンクせずに呼べるので、依存を増やさずに済む。
#[cfg(windows)]
fn peak_memory_mb() -> f64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<Counters>() as u32,
        )
    };
    if ok == 0 {
        return f64::NAN;
    }
    counters.peak_working_set_size as f64 / (1024.0 * 1024.0)
}

#[cfg(not(windows))]
fn peak_memory_mb() -> f64 {
    f64::NAN
}

fn write_csv(path: &Path, header: &str, rows: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    writeln!(file, "{header}")?;
    for row in rows {
        writeln!(file, "{row}")?;
    }
    println!("  CSV -> {}", path.display());
    Ok(())
}

/// CSV のセルとして安全な形にする。Windows のパスにカンマはまず入らないが、
/// 入っていたときに列がずれると数字が全部読めなくなるので念のため。
fn cell(value: &str) -> String {
    if value.contains(',') || value.contains('"') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// 実データを踏まないよう、書き込み先は必ず一時ディレクトリの下に作る。
fn scratch(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "photo-curator-bench-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase()),
        Some(ext) if ["jpg", "jpeg", "png", "webp"].contains(&ext.as_str())
    )
}

fn collect_photos(folder: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && supported(entry.path()))
        .map(|entry| entry.into_path())
        .collect();
    entries.sort();
    entries
}

// ---------------------------------------------------------------------------
// dHash（lib と同一のビット抽出）
// ---------------------------------------------------------------------------

/// `photo_curator_lib::d_hash` の中身のうち、デコード後の部分だけを取り出したもの。
/// デコード方式を差し替えて比較するために必要。`--bin bench decode` の実行時に
/// フルデコード経路が本物の `d_hash()` と一致することを毎回検証している
/// （一致しなければ比較そのものが無意味になるため）。
fn hash_bits(image: &DynamicImage) -> String {
    bits_of(&image.resize_exact(9, 8, FilterType::Triangle).grayscale())
}

/// 縮小に `thumbnail_exact` を使う版。`resize_exact(Triangle)` は元画素を全部
/// 舐めるため 12〜24Mpx では**デコードより高くつく**（実測 84.8 ms/枚 対 59.9 ms/枚）。
/// `thumbnail_exact` は先に整数倍で間引いてから縮めるので桁が変わる。
fn hash_bits_thumbnail(image: &DynamicImage) -> String {
    bits_of(&image.thumbnail_exact(9, 8).grayscale())
}

fn bits_of(image: &DynamicImage) -> String {
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            if image.get_pixel(x, y).0[0] > image.get_pixel(x + 1, y).0[0] {
                bits |= 1 << (y * 8 + x);
            }
        }
    }
    format!("{bits:016x}")
}

fn hamming(left: &str, right: &str) -> u32 {
    u64::from_str_radix(left, 16)
        .ok()
        .zip(u64::from_str_radix(right, 16).ok())
        .map(|(a, b)| (a ^ b).count_ones())
        .unwrap_or(64)
}

/// EXIF に埋め込まれたサムネイル JPEG を取り出す。
/// APP1 の TIFF ブロック内 IFD1 の JPEGInterchangeFormat（オフセット）と
/// JPEGInterchangeFormatLength（長さ）が実体を指す。
fn exif_thumbnail(path: &Path) -> Option<Vec<u8>> {
    use exif::{In, Tag, Value};
    let bytes = fs::read(path).ok()?;
    let tiff = exif::get_exif_attr_from_jpeg(&mut std::io::Cursor::new(&bytes)).ok()?;
    let (fields, _) = exif::parse_exif(&tiff).ok()?;
    let find = |tag: Tag| -> Option<usize> {
        fields
            .iter()
            .find(|field| field.tag == tag && field.ifd_num == In::THUMBNAIL)
            .and_then(|field| match &field.value {
                Value::Long(values) => values.first().map(|value| *value as usize),
                Value::Short(values) => values.first().map(|value| *value as usize),
                _ => None,
            })
    };
    let offset = find(Tag::JPEGInterchangeFormat)?;
    let length = find(Tag::JPEGInterchangeFormatLength)?;
    let end = offset.checked_add(length)?;
    if end > tiff.len() {
        return None;
    }
    Some(tiff[offset..end].to_vec())
}

/// jpeg-decoder の IDCT スケーリングで 1/8 相当まで小さくデコードする。
/// `scale()` は 1/8・1/4・1/2・1/1 のうち要求以上で最小のものを選ぶ。
fn scaled_decode(path: &Path) -> Option<DynamicImage> {
    let file = fs::File::open(path).ok()?;
    let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    decoder
        .scale(info.width.div_ceil(8), info.height.div_ceil(8))
        .ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as u32, info.height as u32);
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// パイプライン（run_scan / run_burst_analysis と同じ順序・同じ判定）
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PhaseTimes {
    walk: Duration,
    scan_fingerprint: Duration,
    scan_db: Duration,
    exif: Duration,
    meta_db: Duration,
    hash_fingerprint: Duration,
    decode_hash: Duration,
    hash_db: Duration,
    groups_query: Duration,
    groups_build: Duration,
}

impl PhaseTimes {
    fn total(&self) -> Duration {
        self.walk
            + self.scan_fingerprint
            + self.scan_db
            + self.exif
            + self.meta_db
            + self.hash_fingerprint
            + self.decode_hash
            + self.hash_db
            + self.groups_query
            + self.groups_build
    }
}

#[derive(Default)]
struct PipelineStats {
    photos: usize,
    exif_hits: usize,
    mtime_fallbacks: usize,
    captured_at_reused: usize,
    burst_candidates: usize,
    hash_cache_hits: usize,
    hash_computed: usize,
    hash_failures: usize,
    groups: usize,
    grouped_photos: usize,
    /// timestamp_source ごとの内訳（exif_original など）。
    timestamp_sources: BTreeMap<String, usize>,
    /// どのデコード経路でサムネイルを作ったかの内訳。
    decode_sources: BTreeMap<String, usize>,
    thumbnail_hits: usize,
    thumbnail_generated: usize,
    thumbnail_failures: usize,
    /// 自動縮小後に実際に使われた時間窓。
    candidate_window_ms: i64,
    candidate_window_narrowed: bool,
    weak_pairs_skipped: usize,
    /// サムネイルキャッシュの総バイト数。
    thumbnail_bytes: u64,
}

/// scan → metadata → hashing → grouping を通しで走らせる。
/// 判定条件（4秒窓、fingerprint 一致でのキャッシュ再利用、EXIF→mtime フォールバック）は
/// `run_burst_analysis` と同じものを使っている。
fn run_pipeline(
    conn: &Connection,
    project_id: &str,
    folder: &Path,
    thumbnails: &Path,
) -> (PhaseTimes, PipelineStats) {
    let mut times = PhaseTimes::default();
    let mut stats = PipelineStats::default();

    // --- scan ---------------------------------------------------------------
    let (entries, walk) = timed(|| collect_photos(folder));
    times.walk = walk;
    stats.photos = entries.len();

    let (_, scan_db) = timed(|| {
        conn.execute(
            "UPDATE photos SET is_missing=1 WHERE project_id=?1",
            params![project_id],
        )
        .expect("mark missing");
    });
    times.scan_db += scan_db;

    let transaction = conn.unchecked_transaction().expect("begin scan");
    for path in &entries {
        let (print, elapsed) = timed(|| fingerprint(path));
        times.scan_fingerprint += elapsed;
        let (mtime, size) = match print {
            Some((mtime, size)) => (Some(mtime), Some(size)),
            None => (None, None),
        };
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
        let (_, elapsed) = timed(|| {
            upsert_photo(
                &transaction,
                project_id,
                &absolute,
                &relative,
                &name,
                mtime,
                size,
            )
            .expect("upsert");
        });
        times.scan_db += elapsed;
    }
    let (_, elapsed) = timed(|| transaction.commit().expect("commit scan"));
    times.scan_db += elapsed;

    // --- metadata -----------------------------------------------------------
    let records: Vec<(String, String, Option<i64>, Option<String>)> = {
        let mut statement = conn
            .prepare(
                "SELECT id,path,captured_at,timestamp_source FROM photos
                 WHERE project_id=?1 AND is_missing=0 ORDER BY path",
            )
            .expect("prepare metadata");
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query metadata");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("collect metadata")
    };

    let transaction = conn.unchecked_transaction().expect("begin metadata");
    for (id, path, existing, stored_source) in &records {
        if let (Some(_), Some(source)) = (existing, stored_source) {
            stats.captured_at_reused += 1;
            *stats.timestamp_sources.entry(source.clone()).or_default() += 1;
            continue;
        }
        let photo_path = Path::new(path);
        let (capture, elapsed) = timed(|| capture_time(photo_path));
        times.exif += elapsed;
        let source = capture.map_or("unknown", |(_, source)| source);
        *stats
            .timestamp_sources
            .entry(source.to_owned())
            .or_default() += 1;
        match source {
            "exif_original" | "exif_datetime" => stats.exif_hits += 1,
            "filesystem_mtime" => stats.mtime_fallbacks += 1,
            _ => {}
        }
        let (_, elapsed) = timed(|| {
            transaction
                .execute(
                    "UPDATE photos SET captured_at=?1,timestamp_source=?2 WHERE id=?3",
                    params![capture.map(|(at, _)| at), source, id],
                )
                .expect("update captured_at");
        });
        times.meta_db += elapsed;
    }
    let (_, elapsed) = timed(|| transaction.commit().expect("commit metadata"));
    times.meta_db += elapsed;

    // --- 時間近接候補の絞り込み ----------------------------------------------
    struct Candidate {
        id: String,
        path: String,
        captured_at: i64,
        source: TimestampSource,
        cached: CachedAnalysis,
    }
    let candidates: Vec<Candidate> = {
        let mut statement = conn
            .prepare(
                "SELECT id,path,captured_at,timestamp_source,d_hash,d_hash_version,
                        thumbnail_path,thumbnail_mtime,thumbnail_size,thumbnail_version
                 FROM photos
                 WHERE project_id=?1 AND is_missing=0 AND captured_at IS NOT NULL
                 ORDER BY captured_at",
            )
            .expect("prepare candidates");
        let rows = statement
            .query_map(params![project_id], |row| {
                let source: Option<String> = row.get(3)?;
                Ok(Candidate {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    captured_at: row.get(2)?,
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
            .expect("query candidates");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("collect candidates")
    };
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
    let needed_records: Vec<_> = candidates
        .iter()
        .filter(|record| selection.ids.contains(&record.id))
        .collect();
    stats.burst_candidates = needed_records.len();
    stats.candidate_window_ms = selection.window_ms;
    stats.candidate_window_narrowed = selection.narrowed;
    stats.weak_pairs_skipped = selection.weak_pairs_skipped;

    // --- hashing ------------------------------------------------------------
    let transaction = conn.unchecked_transaction().expect("begin hashing");
    for record in &needed_records {
        let photo_path = Path::new(&record.path);
        let (current, elapsed) = timed(|| fingerprint(photo_path));
        times.hash_fingerprint += elapsed;
        let (outcome, elapsed) =
            timed(|| analyse_photo(thumbnails, &record.id, photo_path, current, &record.cached));
        times.decode_hash += elapsed;
        match outcome.thumbnail_state {
            ThumbnailState::Hit => stats.thumbnail_hits += 1,
            ThumbnailState::Generated(source) => {
                stats.thumbnail_generated += 1;
                *stats
                    .decode_sources
                    .entry(decode_source_name(source).to_owned())
                    .or_default() += 1;
            }
            ThumbnailState::Failed => stats.thumbnail_failures += 1,
        }
        if outcome.hash_reused {
            stats.hash_cache_hits += 1;
            continue;
        }
        if outcome.d_hash.is_some() {
            stats.hash_computed += 1;
        } else {
            stats.hash_failures += 1;
        }
        let (mtime, size) = match current {
            Some((mtime, size)) => (Some(mtime), Some(size)),
            None => (None, None),
        };
        let (thumb_mtime, thumb_size) = match outcome.thumbnail_path {
            Some(_) => (mtime, size),
            None => (None, None),
        };
        let (_, elapsed) = timed(|| {
            transaction
                .execute(
                    "UPDATE photos SET d_hash=?1,d_hash_version=?2,thumbnail_path=?3,
                       thumbnail_mtime=?4,thumbnail_size=?5,fingerprint_mtime=?6,fingerprint_size=?7
                     WHERE id=?8",
                    params![
                        outcome.d_hash,
                        outcome.d_hash.as_ref().map(|_| D_HASH_VERSION),
                        outcome.thumbnail_path,
                        thumb_mtime,
                        thumb_size,
                        mtime,
                        size,
                        record.id
                    ],
                )
                .expect("update hash");
        });
        times.hash_db += elapsed;
    }
    let (_, elapsed) = timed(|| transaction.commit().expect("commit hashing"));
    times.hash_db += elapsed;

    // --- get_burst_groups ---------------------------------------------------
    let (entries, elapsed) = timed(|| {
        let mut statement = conn
            .prepare(
                "SELECT id,captured_at,d_hash FROM photos
                 WHERE project_id=?1 AND is_missing=0 AND captured_at IS NOT NULL
                   AND d_hash IS NOT NULL ORDER BY captured_at",
            )
            .expect("prepare groups");
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("query groups");
        rows.collect::<Result<Vec<(String, i64, String)>, _>>()
            .expect("collect groups")
    });
    times.groups_query = elapsed;
    // 計測は既定値で行う。閾値は本来プロジェクトごとの学習値だが、
    // ここで測りたいのはグルーピング処理そのものの速度。
    let (groups, elapsed) = timed(|| build_burst_groups(entries, HASH_DISTANCE_LIMIT));
    times.groups_build = elapsed;
    stats.groups = groups.len();
    stats.grouped_photos = groups.iter().map(Vec::len).sum();
    stats.thumbnail_bytes = directory_bytes(thumbnails);

    (times, stats)
}

fn decode_source_name(source: DecodeSource) -> &'static str {
    match source {
        DecodeSource::ExifThumbnail => "exif_thumbnail",
        DecodeSource::JpegScaled => "jpeg_scaled",
        DecodeSource::FullDecode => "full_decode",
    }
}

fn directory_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or(0)
}

fn seed_project(conn: &Connection, folder: &Path) -> String {
    let project_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id,name,folder_path,photo_count,status,created_at,updated_at)
         VALUES (?1,'bench',?2,0,'new',0,0)",
        params![project_id, folder.to_string_lossy()],
    )
    .expect("insert project");
    project_id
}

fn phase_rows(label: &str, times: &PhaseTimes, stats: &PipelineStats) -> Vec<String> {
    let total = ms(times.total()).max(f64::MIN_POSITIVE);
    let mut rows = Vec::new();
    let mut push = |phase: &str, duration: Duration| {
        rows.push(format!(
            "{},{},{:.1},{:.2},{}",
            cell(label),
            phase,
            ms(duration),
            ms(duration) / total * 100.0,
            stats.photos
        ));
    };
    push("walkdir", times.walk);
    push("scan_fingerprint", times.scan_fingerprint);
    push("scan_db", times.scan_db);
    push("exif", times.exif);
    push("metadata_db", times.meta_db);
    push("hash_fingerprint", times.hash_fingerprint);
    push("decode_hash", times.decode_hash);
    push("hash_db", times.hash_db);
    push("groups_query", times.groups_query);
    push("groups_build", times.groups_build);
    rows
}

fn report(label: &str, times: &PhaseTimes, stats: &PipelineStats) {
    let total = ms(times.total()).max(f64::MIN_POSITIVE);
    let share = |duration: Duration| ms(duration) / total * 100.0;
    println!("\n=== {label} ===");
    println!("  写真          {}", stats.photos);
    println!("  合計          {:.0} ms", total);
    println!(
        "  walkdir       {:8.1} ms ({:5.2}%)",
        ms(times.walk),
        share(times.walk)
    );
    println!(
        "  scan fp       {:8.1} ms ({:5.2}%)",
        ms(times.scan_fingerprint),
        share(times.scan_fingerprint)
    );
    println!(
        "  scan DB       {:8.1} ms ({:5.2}%)",
        ms(times.scan_db),
        share(times.scan_db)
    );
    println!(
        "  EXIF          {:8.1} ms ({:5.2}%)",
        ms(times.exif),
        share(times.exif)
    );
    println!(
        "  metadata DB   {:8.1} ms ({:5.2}%)",
        ms(times.meta_db),
        share(times.meta_db)
    );
    println!(
        "  hash fp       {:8.1} ms ({:5.2}%)",
        ms(times.hash_fingerprint),
        share(times.hash_fingerprint)
    );
    println!(
        "  decode+hash   {:8.1} ms ({:5.2}%)",
        ms(times.decode_hash),
        share(times.decode_hash)
    );
    println!(
        "  hash DB       {:8.1} ms ({:5.2}%)",
        ms(times.hash_db),
        share(times.hash_db)
    );
    println!(
        "  groups query  {:8.1} ms ({:5.2}%)",
        ms(times.groups_query),
        share(times.groups_query)
    );
    println!(
        "  groups build  {:8.1} ms ({:5.2}%)",
        ms(times.groups_build),
        share(times.groups_build)
    );
    let attempted = stats.exif_hits + stats.mtime_fallbacks;
    println!(
        "  EXIF成功率      {} / {} ({:.1}%)   captured_at 再利用 {}",
        stats.exif_hits,
        attempted,
        if attempted == 0 {
            0.0
        } else {
            stats.exif_hits as f64 / attempted as f64 * 100.0
        },
        stats.captured_at_reused
    );
    println!(
        "  mtime fallback率 {} / {} ({:.1}%)",
        stats.mtime_fallbacks,
        attempted,
        if attempted == 0 {
            0.0
        } else {
            stats.mtime_fallbacks as f64 / attempted as f64 * 100.0
        }
    );
    println!(
        "  時間近接候補率   {} / {} ({:.1}%)",
        stats.burst_candidates,
        stats.photos,
        if stats.photos == 0 {
            0.0
        } else {
            stats.burst_candidates as f64 / stats.photos as f64 * 100.0
        }
    );
    println!(
        "  hashキャッシュヒット率 {} / {} ({:.1}%)   算出 {} 失敗 {}",
        stats.hash_cache_hits,
        stats.burst_candidates,
        if stats.burst_candidates == 0 {
            0.0
        } else {
            stats.hash_cache_hits as f64 / stats.burst_candidates as f64 * 100.0
        },
        stats.hash_computed,
        stats.hash_failures
    );
    println!(
        "  burst グループ {} 件 / 所属 {} 枚",
        stats.groups, stats.grouped_photos
    );
    println!(
        "  時間窓 {} ms{}   弱い根拠で除外したペア {}",
        stats.candidate_window_ms,
        if stats.candidate_window_narrowed {
            "（自動縮小）"
        } else {
            ""
        },
        stats.weak_pairs_skipped
    );
    println!("  timestamp_source {:?}", stats.timestamp_sources);
    println!(
        "  サムネイル ヒット {} / 生成 {} / 失敗 {}   経路 {:?}",
        stats.thumbnail_hits,
        stats.thumbnail_generated,
        stats.thumbnail_failures,
        stats.decode_sources
    );
    println!(
        "  サムネイルキャッシュ {:.1} MB",
        stats.thumbnail_bytes as f64 / 1_048_576.0
    );
    println!("  peak working set {:.1} MB", peak_memory_mb());
}

fn summary_row(label: &str, times: &PhaseTimes, stats: &PipelineStats) -> String {
    let attempted = (stats.exif_hits + stats.mtime_fallbacks).max(1);
    format!(
        "{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{},{},{},{},{},{:.2},{:.2},{:.2},{:.1}",
        cell(label),
        stats.photos,
        ms(times.total()),
        ms(times.walk),
        ms(times.scan_fingerprint),
        ms(times.scan_db),
        ms(times.exif),
        ms(times.meta_db),
        ms(times.hash_fingerprint),
        ms(times.decode_hash),
        ms(times.hash_db),
        ms(times.groups_query),
        ms(times.groups_build),
        stats.exif_hits,
        stats.mtime_fallbacks,
        stats.burst_candidates,
        stats.hash_cache_hits,
        stats.groups,
        stats.exif_hits as f64 / attempted as f64 * 100.0,
        if stats.photos == 0 { 0.0 } else { stats.burst_candidates as f64 / stats.photos as f64 * 100.0 },
        if stats.burst_candidates == 0 { 0.0 } else { stats.hash_cache_hits as f64 / stats.burst_candidates as f64 * 100.0 },
        peak_memory_mb(),
    ) + &format!(
        ",{},{},{},{},{},{},{:.2},{}",
        stats.candidate_window_ms,
        stats.candidate_window_narrowed,
        stats.weak_pairs_skipped,
        stats.thumbnail_hits,
        stats.thumbnail_generated,
        stats.thumbnail_failures,
        stats.thumbnail_bytes as f64 / 1_048_576.0,
        cell(&format!("{:?}", stats.decode_sources)),
    )
}

const SUMMARY_HEADER: &str = "label,photos,total_ms,walk_ms,scan_fingerprint_ms,scan_db_ms,exif_ms,metadata_db_ms,hash_fingerprint_ms,decode_hash_ms,hash_db_ms,groups_query_ms,groups_build_ms,exif_hits,mtime_fallbacks,burst_candidates,hash_cache_hits,groups,exif_success_pct,burst_candidate_pct,hash_cache_hit_pct,peak_rss_mb,candidate_window_ms,candidate_window_narrowed,weak_pairs_skipped,thumbnail_hits,thumbnail_generated,thumbnail_failures,thumbnail_cache_mb,decode_sources";
const PHASE_HEADER: &str = "label,phase,ms,share_pct,photos";

// ---------------------------------------------------------------------------
// real: 実データ 271 枚
// ---------------------------------------------------------------------------

fn command_real(folder: &Path, out: &Path, real_db: Option<&Path>) {
    println!("実データ計測: {}", folder.display());
    let work = scratch("real");

    // 本番 DB は読むだけ。コピーに対して open_database を通し、backfill の
    // 実測値と移行後の行数だけを取る。
    if let Some(source) = real_db {
        if source.exists() {
            let copy = work.join("real-copy.sqlite3");
            fs::copy(source, &copy).expect("copy real database");
            let (conn, elapsed) = timed(|| open_database(&copy).expect("open real copy"));
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))
                .unwrap_or(-1);
            let filled: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM photos WHERE fingerprint_mtime IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(-1);
            let hashed: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM photos WHERE d_hash IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(-1);
            println!(
                "\n本番DBコピーの open_database(): {:.1} ms / {rows} 行 / fingerprint 補完 {filled} 行 / d_hash {hashed} 行",
                ms(elapsed)
            );
            // 2回目は backfill 対象ゼロ。冪等性と定常コストの確認。
            drop(conn);
            let (_, elapsed) = timed(|| open_database(&copy).expect("reopen real copy"));
            println!("  2回目（backfill 対象なし）: {:.1} ms", ms(elapsed));
        } else {
            println!(
                "本番DBが見つからないため migration 計測はスキップ: {}",
                source.display()
            );
        }
    }

    // 空の一時DBを起点にした通しの初回実行。本番DBには一切触れない。
    let db = work.join("bench.sqlite3");
    let conn = open_database(&db).expect("open bench database");
    let project_id = seed_project(&conn, folder);

    let thumbnails = work.join("thumbnails");
    fs::create_dir_all(&thumbnails).expect("create thumbnail cache");

    let (cold_times, cold_stats) = run_pipeline(&conn, &project_id, folder, &thumbnails);
    report("実データ 初回（キャッシュなし）", &cold_times, &cold_stats);

    let (warm_times, warm_stats) = run_pipeline(&conn, &project_id, folder, &thumbnails);
    report("実データ 2回目（キャッシュあり）", &warm_times, &warm_stats);

    // ファイル単位の内訳。EXIF / デコード / resize+hash を別々に測る。
    println!("\nファイル単位の内訳を計測中…");
    let entries = collect_photos(folder);
    let mut file_rows = Vec::with_capacity(entries.len());
    let mut decode_total = 0.0f64;
    let mut resize_total = 0.0f64;
    let mut exif_total = 0.0f64;
    // Step 4 後の実経路。フルデコード版と同じループで並べて測る。
    let mut fast_total = 0.0f64;
    let mut fast_sources: BTreeMap<&'static str, usize> = BTreeMap::new();
    for path in &entries {
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let format = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (captured, exif_elapsed) = timed(|| capture_time(path));
        let (fast_hash, fast_elapsed) = timed(|| d_hash(path));
        fast_total += ms(fast_elapsed);
        *fast_sources
            .entry(photo_curator_lib::bench_api::decode_source(path).unwrap_or("failed"))
            .or_default() += 1;
        let (decoded, decode_elapsed) = timed(|| image::open(path).ok());
        let (dimensions, resize_elapsed) = match &decoded {
            Some(image) => {
                let dimensions = image.dimensions();
                let (_, elapsed) = timed(|| hash_bits(image));
                (Some(dimensions), elapsed)
            }
            None => (None, Duration::ZERO),
        };
        exif_total += ms(exif_elapsed);
        decode_total += ms(decode_elapsed);
        resize_total += ms(resize_elapsed);
        let mut row = String::new();
        let _ = write!(
            row,
            "{},{},{},{},{},{:.2},{:.2},{:.2},{}",
            cell(&path.to_string_lossy()),
            format,
            dimensions.map(|d| d.0).unwrap_or(0),
            dimensions.map(|d| d.1).unwrap_or(0),
            size,
            ms(exif_elapsed),
            ms(decode_elapsed),
            ms(resize_elapsed),
            if decoded.is_some() { "ok" } else { "failed" },
        );
        let _ = write!(
            row,
            ",{},{:.2},{},{}",
            captured.map_or("none", |(_, source)| source),
            ms(fast_elapsed),
            photo_curator_lib::bench_api::decode_source(path).unwrap_or("failed"),
            if fast_hash.is_some() { "ok" } else { "failed" },
        );
        file_rows.push(row);
    }
    let count = entries.len().max(1) as f64;
    println!(
        "  EXIF {:.2} ms/枚 / 旧フルデコード {:.2} ms/枚 / 旧 resize+hash {:.2} ms/枚",
        exif_total / count,
        decode_total / count,
        resize_total / count
    );
    println!(
        "  新デコード+hash {:.2} ms/枚（旧 {:.2} ms/枚 の {:.1}倍速）",
        fast_total / count,
        (decode_total + resize_total) / count,
        (decode_total + resize_total) / fast_total.max(f64::MIN_POSITIVE)
    );
    println!("  デコード経路の内訳: {fast_sources:?}");

    write_csv(
        &out.join("real-per-file.csv"),
        "path,format,width,height,bytes,exif_ms,full_decode_ms,full_resize_hash_ms,full_decode_result,captured_at_source,fast_hash_ms,decode_source,fast_hash_result",
        &file_rows,
    )
    .expect("write per-file csv");
    write_csv(
        &out.join("real-summary.csv"),
        SUMMARY_HEADER,
        &[
            summary_row("real-cold", &cold_times, &cold_stats),
            summary_row("real-warm", &warm_times, &warm_stats),
        ],
    )
    .expect("write summary csv");
    let mut phases = phase_rows("real-cold", &cold_times, &cold_stats);
    phases.extend(phase_rows("real-warm", &warm_times, &warm_stats));
    write_csv(&out.join("real-phases.csv"), PHASE_HEADER, &phases).expect("write phase csv");

    let _ = fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// synth: 合成 N 枚
// ---------------------------------------------------------------------------

/// 小さな JPEG を N 枚作る。scan・DB書き込み・grouping のスケーリングを見るのが
/// 目的で、デコード負荷は実データ側の ms/枚 から外挿するため、ここでは
/// 意図的に小さい画像を使う（5000枚を実サイズで置くと 14GB になる）。
fn generate_photos(folder: &Path, count: usize) -> Duration {
    fs::create_dir_all(folder).expect("create synthetic folder");
    let start = Instant::now();
    for index in 0..count {
        // 連番で少しずつ変化する模様。全部同一だと dHash が完全一致して
        // グルーピングが非現実的に単純になる。
        let mut image = image::RgbImage::new(64, 64);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let seed = (index as u32).wrapping_mul(37);
            *pixel = image::Rgb([
                ((x * 4 + seed) % 256) as u8,
                ((y * 4 + seed / 3) % 256) as u8,
                ((x + y + seed / 7) % 256) as u8,
            ]);
        }
        let path = folder.join(format!("synthetic-{index:06}.jpg"));
        image.save(&path).expect("write synthetic photo");
    }
    start.elapsed()
}

fn command_synth(counts: &[usize], out: &Path) {
    let mut summaries = Vec::new();
    let mut phases = Vec::new();
    for &count in counts {
        println!("\n合成 {count} 枚を生成中…");
        let work = scratch(&format!("synth{count}"));
        let photos = work.join("photos");
        let elapsed = generate_photos(&photos, count);
        println!("  生成 {:.0} ms", ms(elapsed));

        let db = work.join("bench.sqlite3");
        let conn = open_database(&db).expect("open synthetic database");
        let project_id = seed_project(&conn, &photos);

        let thumbnails = work.join("thumbnails");
        fs::create_dir_all(&thumbnails).expect("create thumbnail cache");

        let label_cold = format!("synth{count}-cold");
        let (cold_times, cold_stats) = run_pipeline(&conn, &project_id, &photos, &thumbnails);
        report(&label_cold, &cold_times, &cold_stats);
        let label_warm = format!("synth{count}-warm");
        let (warm_times, warm_stats) = run_pipeline(&conn, &project_id, &photos, &thumbnails);
        report(&label_warm, &warm_times, &warm_stats);

        // WAL モードなので本体だけ見ても実容量が分からない。-wal / -shm も足す。
        let db_bytes: u64 = ["", "-wal", "-shm"]
            .iter()
            .map(|suffix| {
                let mut path = db.clone().into_os_string();
                path.push(suffix);
                fs::metadata(PathBuf::from(path))
                    .map(|m| m.len())
                    .unwrap_or(0)
            })
            .sum();
        println!(
            "  DBサイズ {:.2} MB ({:.0} bytes/枚)",
            db_bytes as f64 / 1024.0 / 1024.0,
            db_bytes as f64 / count as f64
        );

        summaries.push(summary_row(&label_cold, &cold_times, &cold_stats));
        summaries.push(summary_row(&label_warm, &warm_times, &warm_stats));
        phases.extend(phase_rows(&label_cold, &cold_times, &cold_stats));
        phases.extend(phase_rows(&label_warm, &warm_times, &warm_stats));

        drop(conn);
        let _ = fs::remove_dir_all(&work);
    }
    write_csv(
        &out.join("synthetic-summary.csv"),
        SUMMARY_HEADER,
        &summaries,
    )
    .expect("write synthetic summary");
    write_csv(&out.join("synthetic-phases.csv"), PHASE_HEADER, &phases)
        .expect("write synthetic phases");
}

// ---------------------------------------------------------------------------
// decode: デコード方式の比較
// ---------------------------------------------------------------------------

/// 同じ1枚を各方式で hash した結果。デコードに失敗した方式は None。
struct HashTriple {
    full: String,
    thumbnail: Option<String>,
    scaled: Option<String>,
    fast_resize: Option<String>,
}

fn command_decode(folder: &Path, out: &Path, count: usize) {
    let entries: Vec<PathBuf> = collect_photos(folder).into_iter().take(count).collect();
    println!("デコード方式比較: {} 枚", entries.len());

    let mut rows = Vec::with_capacity(entries.len());
    // 撮影順（= ファイル名順）に並んだ3方式のハッシュ。あとで連続ペアの
    // 判定が変わるかを見るのに使う。
    let mut hashes: Vec<HashTriple> = Vec::with_capacity(entries.len());
    let (mut full_ms, mut thumb_ms, mut scaled_ms, mut fast_ms) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut thumb_ok, mut scaled_ok, mut fast_ok) = (0usize, 0usize, 0usize);
    let mut thumb_distances: Vec<u32> = Vec::new();
    let mut scaled_distances: Vec<u32> = Vec::new();
    let mut fast_distances: Vec<u32> = Vec::new();
    let mut mismatched_reference = 0usize;

    for path in &entries {
        // ① Routine 1 の基準（Step 4 以前の d_hash と同じ、フルデコード +
        // resize_exact）。Step 4 で本体はここから離れたので、比較の基準として
        // ローカルに保持する。
        let (reference, elapsed) = timed(|| image::open(path).ok().map(|image| hash_bits(&image)));
        let full_elapsed = ms(elapsed);
        full_ms += full_elapsed;
        let Some(reference) = reference else {
            rows.push(format!("{},failed,,,,,,,", cell(&path.to_string_lossy())));
            continue;
        };
        // 本番の d_hash() が②相当の経路に乗っていることを毎回確かめる。
        // ずれていたら、この比較表と実際に動くコードが別物になっている。
        let production = d_hash(path);
        if production.is_none() {
            mismatched_reference += 1;
        }

        // ② EXIF 埋め込みサムネイル
        let (thumb_hash, elapsed) = timed(|| {
            exif_thumbnail(path)
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|image| hash_bits(&image))
        });
        let thumb_elapsed = ms(elapsed);
        thumb_ms += thumb_elapsed;

        // ③ JPEG scaled decode（1/8 IDCT）
        let (scaled_hash, elapsed) = timed(|| scaled_decode(path).map(|image| hash_bits(&image)));
        let scaled_elapsed = ms(elapsed);
        scaled_ms += scaled_elapsed;

        // ④ フルデコードのまま、縮小だけ thumbnail_exact に替えた版。
        // デコードは現行と同じなので、resize が占めている分だけが浮く。
        let (fast_hash, elapsed) = timed(|| {
            image::open(path)
                .ok()
                .map(|image| hash_bits_thumbnail(&image))
        });
        let fast_elapsed = ms(elapsed);
        fast_ms += fast_elapsed;

        let thumb_distance = thumb_hash.as_ref().map(|hash| hamming(&reference, hash));
        let scaled_distance = scaled_hash.as_ref().map(|hash| hamming(&reference, hash));
        let fast_distance = fast_hash.as_ref().map(|hash| hamming(&reference, hash));
        if let Some(distance) = fast_distance {
            fast_ok += 1;
            fast_distances.push(distance);
        }
        if let Some(distance) = thumb_distance {
            thumb_ok += 1;
            thumb_distances.push(distance);
        }
        if let Some(distance) = scaled_distance {
            scaled_ok += 1;
            scaled_distances.push(distance);
        }

        hashes.push(HashTriple {
            full: reference.clone(),
            thumbnail: thumb_hash.clone(),
            scaled: scaled_hash.clone(),
            fast_resize: fast_hash.clone(),
        });

        let distance = |value: Option<u32>| {
            value
                .map(|d| d.to_string())
                .unwrap_or_else(|| "na".to_owned())
        };
        rows.push(format!(
            "{},ok,{},{},{},{},{:.2},{:.2},{:.2},{:.2},{},{},{}",
            cell(&path.to_string_lossy()),
            reference,
            thumb_hash.clone().unwrap_or_default(),
            scaled_hash.clone().unwrap_or_default(),
            fast_hash.clone().unwrap_or_default(),
            full_elapsed,
            thumb_elapsed,
            scaled_elapsed,
            fast_elapsed,
            distance(thumb_distance),
            distance(scaled_distance),
            distance(fast_distance),
        ));
    }

    write_csv(
        &out.join("decode-per-file.csv"),
        "path,result,full_hash,thumb_hash,scaled_hash,fast_resize_hash,full_ms,thumb_ms,scaled_ms,fast_resize_ms,thumb_hamming,scaled_hamming,fast_resize_hamming",
        &rows,
    )
    .expect("write decode csv");

    let count = entries.len().max(1) as f64;
    println!("\n=== デコード方式比較 ===");
    if mismatched_reference > 0 {
        println!("  ⚠ 本番の d_hash() が {mismatched_reference} 件で値を返せなかった。");
    } else {
        println!("  本番の d_hash() は全件で値を返した（①は Step 4 以前の基準値）");
    }
    println!(
        "  ① フルデコード     {:7.2} ms/枚  成功 {}/{}",
        full_ms / count,
        entries.len(),
        entries.len()
    );
    println!(
        "  ② EXIFサムネイル   {:7.2} ms/枚  成功 {}/{}  ({:.1}x)",
        thumb_ms / count,
        thumb_ok,
        entries.len(),
        full_ms / thumb_ms.max(f64::MIN_POSITIVE)
    );
    println!(
        "  ③ scaled decode    {:7.2} ms/枚  成功 {}/{}  ({:.1}x)",
        scaled_ms / count,
        scaled_ok,
        entries.len(),
        full_ms / scaled_ms.max(f64::MIN_POSITIVE)
    );
    println!(
        "  ④ フル+高速縮小   {:7.2} ms/枚  成功 {}/{}  ({:.1}x)",
        fast_ms / count,
        fast_ok,
        entries.len(),
        full_ms / fast_ms.max(f64::MIN_POSITIVE)
    );

    let mut distribution_rows = Vec::new();
    for (label, distances) in [
        ("thumbnail", &thumb_distances),
        ("scaled", &scaled_distances),
        ("fast_resize", &fast_distances),
    ] {
        if distances.is_empty() {
            println!("\n  {label}: 成功件数ゼロ");
            continue;
        }
        let mut sorted = distances.clone();
        sorted.sort_unstable();
        let mean = sorted.iter().map(|d| *d as f64).sum::<f64>() / sorted.len() as f64;
        let percentile = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
        let within = |limit: u32| {
            sorted.iter().filter(|d| **d <= limit).count() as f64 / sorted.len() as f64 * 100.0
        };
        println!(
            "\n  {label} のフルデコード版との dHash ハミング距離（n={}）",
            sorted.len()
        );
        println!(
            "    平均 {mean:.2} / 中央 {} / p90 {} / p95 {} / 最大 {}",
            percentile(0.5),
            percentile(0.9),
            percentile(0.95),
            sorted[sorted.len() - 1]
        );
        println!(
            "    距離0 {:.1}%  ≤2 {:.1}%  ≤4 {:.1}%  ≤14(判定閾値) {:.1}%",
            within(0),
            within(2),
            within(4),
            within(14)
        );
        // 距離ごとのヒストグラム
        let mut histogram = [0usize; 65];
        for distance in &sorted {
            histogram[*distance as usize] += 1;
        }
        for (distance, hits) in histogram.iter().enumerate() {
            if *hits > 0 {
                distribution_rows.push(format!("{label},{distance},{hits}"));
            }
        }
    }
    write_csv(
        &out.join("decode-hamming-histogram.csv"),
        "method,hamming_distance,count",
        &distribution_rows,
    )
    .expect("write histogram csv");

    // ここが本題。単体ハッシュがどれだけ似ているかではなく、
    // 「隣り合う2枚を同じ連写とみなすか」という *判定* が変わるかどうか。
    // dHash は最終的に hash_distance(a,b) <= HASH_DISTANCE_LIMIT でしか使われない。
    // 撮影順 = ファイル名順（ファイル名がタイムスタンプ）である前提。
    let mut pair_rows = Vec::new();
    type Pick = fn(&HashTriple) -> Option<String>;
    let pickers: [(&str, Pick); 3] = [
        ("thumbnail", |entry| entry.thumbnail.clone()),
        ("scaled", |entry| entry.scaled.clone()),
        ("fast_resize", |entry| entry.fast_resize.clone()),
    ];
    for (label, alternatives) in pickers {
        let mut evaluated = 0usize;
        let mut flips = 0usize;
        let mut deltas: Vec<u32> = Vec::new();
        let mut boundary = 0usize;
        for pair in hashes.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            let (Some(la), Some(ra)) = (alternatives(left), alternatives(right)) else {
                continue;
            };
            evaluated += 1;
            let reference = hamming(&left.full, &right.full);
            let alternative = hamming(&la, &ra);
            deltas.push(reference.abs_diff(alternative));
            if (reference <= HASH_LIMIT) != (alternative <= HASH_LIMIT) {
                flips += 1;
            }
            if reference.abs_diff(HASH_LIMIT) <= 4 {
                boundary += 1;
            }
            pair_rows.push(format!("{label},{reference},{alternative}"));
        }
        let count = evaluated.max(1);
        println!(
            "\n  {label}: 連続ペアの連写判定（距離 <= {HASH_LIMIT}）",
            HASH_LIMIT = HASH_LIMIT
        );
        println!(
            "    評価 {evaluated} ペア / 判定が変わった {flips} ペア ({:.1}%)",
            flips as f64 / count as f64 * 100.0
        );
        println!(
            "    ペア距離のずれ 平均 {:.2} / 最大 {}   閾値±4 の境界ペア {boundary}",
            deltas.iter().map(|d| *d as f64).sum::<f64>() / count as f64,
            deltas.iter().max().copied().unwrap_or(0)
        );
    }
    write_csv(
        &out.join("decode-pair-decisions.csv"),
        "method,full_pair_distance,method_pair_distance",
        &pair_rows,
    )
    .expect("write pair csv");
}

const HASH_LIMIT: u32 = photo_curator_lib::bench_api::HASH_DISTANCE_LIMIT;

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Step 6: worker 数ごとの実測
//
// 呼ぶのはアプリ本体と同じ `run_in_parallel` + `hash_one`。ここで別実装を
// 書くと「計測したコード」と「動いているコード」が別物になる。
// ---------------------------------------------------------------------------

fn command_parallel(folder: &Path, out: &Path, worker_counts: &[usize]) {
    let entries = collect_photos(folder);
    let photos: Vec<(String, String)> = entries
        .iter()
        .enumerate()
        .map(|(index, path)| (format!("bench-{index}"), path.to_string_lossy().to_string()))
        .collect();
    println!("並列計測: {} / {} 枚", folder.display(), photos.len());
    println!(
        "既定の worker 数: {}",
        photo_curator_lib::bench_api::analysis_worker_count()
    );

    let mut rows = Vec::new();
    let mut baseline: Option<f64> = None;
    for &workers in worker_counts {
        // サムネイルキャッシュに当たると読み取りが消えて並列度の意味が無くなる。
        // worker 数ごとに空のキャッシュから始める。
        let thumbnails = scratch(&format!("parallel-{workers}"));
        let started = Instant::now();
        let (completed, failed, timed_out) =
            photo_curator_lib::bench_api::analyse_in_parallel(&thumbnails, &photos, workers);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let speedup = baseline.map_or(1.0, |first: f64| first / elapsed);
        if baseline.is_none() {
            baseline = Some(elapsed);
        }
        println!(
            "  workers {workers}: {elapsed:8.1} ms  ({:.2} ms/枚, {speedup:.2}x)  \
             完了 {completed} 失敗 {failed} timeout {timed_out}  peak {:.1} MB",
            elapsed / photos.len().max(1) as f64,
            peak_memory_mb()
        );
        rows.push(format!(
            "{workers},{elapsed:.1},{:.3},{speedup:.3},{completed},{failed},{timed_out},{:.1}",
            elapsed / photos.len().max(1) as f64,
            peak_memory_mb()
        ));
        let _ = fs::remove_dir_all(&thumbnails);
    }
    write_csv(
        &out.join("parallel-workers.csv"),
        "workers,total_ms,ms_per_photo,speedup_vs_1,completed,failed,timed_out,peak_working_set_mb",
        &rows,
    )
    .expect("write parallel csv");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: bench real <photo-dir> <out-dir> [real-db]\n       bench synth <counts,comma-separated> <out-dir>\n       bench decode <photo-dir> <out-dir> [count]\n       bench parallel <photo-dir> <out-dir> [worker-counts,comma-separated]";
    match args.first().map(String::as_str) {
        Some("real") if args.len() >= 3 => command_real(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.get(3).map(Path::new),
        ),
        Some("synth") if args.len() >= 3 => {
            let counts: Vec<usize> = args[1]
                .split(',')
                .filter_map(|value| value.trim().parse().ok())
                .collect();
            command_synth(&counts, Path::new(&args[2]));
        }
        Some("decode") if args.len() >= 3 => command_decode(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.get(3)
                .and_then(|value| value.parse().ok())
                .unwrap_or(100),
        ),
        Some("parallel") if args.len() >= 3 => {
            let counts: Vec<usize> = args
                .get(3)
                .map(|value| {
                    value
                        .split(',')
                        .filter_map(|value| value.trim().parse().ok())
                        .collect()
                })
                .unwrap_or_else(|| vec![1, 2, 3, 4]);
            command_parallel(Path::new(&args[1]), Path::new(&args[2]), &counts);
        }
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    }
}
