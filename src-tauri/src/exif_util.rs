//! EXIF から撮影時刻・向きを読む（設計 05 章「EXIF は先頭 64KB」）。
//!
//! 旧 `src-tauri/src/lib.rs`（段3 以前）の同名関数を、DB・`PhotoSource` trait への
//! 依存を外してそのまま移した。計算そのもの（civil calendar の扱い等）は変えていない。

use exif::{In, Reader, Tag, Value};
use image::DynamicImage;

/// EXIF を読みに行く先頭バイト数。ほとんどの JPEG は APP1 がこの中に収まる
/// （設計 05 章の実測）。
pub const EXIF_HEAD_PROBE: usize = 64 * 1024;
/// これより小さい EXIF サムネイルは dHash にも表示にも使わない。
const MIN_EXIF_THUMBNAIL_EDGE: u32 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampSource {
    ExifOriginal,
    ExifDateTime,
    FilenameInferred,
    FilesystemMtime,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureTime {
    pub at: i64,
    #[allow(dead_code)]
    pub source: TimestampSource,
}

/// 撮影時刻を、根拠の強い順に探す。EXIF → ファイル名 → mtime。
pub fn read_capture_time(head: &[u8], file_name: &str, mtime_ms: i64) -> CaptureTime {
    if let Some(time) = exif_capture_time_bytes(head) {
        return time;
    }
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    if let Some(at) = filename_capture_time_of(stem) {
        return CaptureTime { at, source: TimestampSource::FilenameInferred };
    }
    CaptureTime { at: mtime_ms, source: TimestampSource::FilesystemMtime }
}

fn ascii_field(exif: &exif::Exif, tag: Tag) -> Option<Vec<u8>> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(values) => values.first().cloned(),
        _ => None,
    }
}

fn exif_capture_time_bytes(bytes: &[u8]) -> Option<CaptureTime> {
    let exif = Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let offset =
        ascii_field(&exif, Tag::OffsetTimeOriginal).or_else(|| ascii_field(&exif, Tag::OffsetTime));
    for (tag, source) in [
        (Tag::DateTimeOriginal, TimestampSource::ExifOriginal),
        (Tag::DateTime, TimestampSource::ExifDateTime),
    ] {
        let Some(raw) = ascii_field(&exif, tag) else {
            continue;
        };
        if let Some(at) = exif_timestamp_ms(&raw, offset.as_deref()) {
            return Some(CaptureTime { at, source });
        }
    }
    None
}

fn exif_timestamp_ms(datetime: &[u8], offset: Option<&[u8]>) -> Option<i64> {
    let mut parsed = exif::DateTime::from_ascii(datetime).ok()?;
    if let Some(offset) = offset {
        let _ = parsed.parse_offset(offset);
    }
    let local = civil_timestamp_ms(
        parsed.year as i64,
        parsed.month as i64,
        parsed.day as i64,
        parsed.hour as i64,
        parsed.minute as i64,
        parsed.second as i64,
    )?;
    Some(local - i64::from(parsed.offset.unwrap_or(0)) * 60_000)
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// その土地の時計の値（年月日時分秒）を、offset を考えずにそのままミリ秒へ。
/// EXIF に明示の offset が無いときはこれで済ませる（Amazon の `contentDate` の
/// 末尾 `Z` を無視して読む処理でも同じものを使う。設計 08 章「6. 撮影時刻の末尾の Z」）。
pub fn civil_timestamp_ms(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Option<i64> {
    if !(1900..=2999).contains(&year) {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
}

fn digit_groups(value: &str) -> Vec<&str> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_fixed(text: &str, widths: &[usize]) -> Option<Vec<i64>> {
    let mut rest = text;
    let mut parts = Vec::with_capacity(widths.len());
    for width in widths {
        if rest.len() < *width {
            return None;
        }
        let (head, tail) = rest.split_at(*width);
        parts.push(head.parse().ok()?);
        rest = tail;
    }
    Some(parts)
}

fn timestamp_from_groups(groups: &[&str]) -> Option<i64> {
    let lengths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
    let parts = match lengths.as_slice() {
        [14, ..] => split_fixed(groups[0], &[4, 2, 2, 2, 2, 2])?,
        [8, 6, ..] => {
            let mut parts = split_fixed(groups[0], &[4, 2, 2])?;
            parts.extend(split_fixed(groups[1], &[2, 2, 2])?);
            parts
        }
        [4, 2, 2, 2, 2, 2, ..] => groups[..6]
            .iter()
            .map(|group| group.parse().ok())
            .collect::<Option<Vec<i64>>>()?,
        _ => return None,
    };
    civil_timestamp_ms(parts[0], parts[1], parts[2], parts[3], parts[4], parts[5])
}

fn filename_capture_time_of(stem: &str) -> Option<i64> {
    let groups = digit_groups(stem);
    (0..groups.len()).find_map(|start| timestamp_from_groups(&groups[start..]))
}

fn orientation_field(fields: &[exif::Field], ifd: In) -> Option<u16> {
    fields
        .iter()
        .find(|field| field.tag == Tag::Orientation && field.ifd_num == ifd)
        .and_then(|field| match &field.value {
            Value::Short(values) => values.first().copied(),
            _ => None,
        })
}

/// 原本の EXIF Orientation。読めなければ 1（無変換）。
pub fn exif_orientation_bytes(bytes: &[u8]) -> u16 {
    let Ok(exif) = Reader::new().read_from_container(&mut std::io::Cursor::new(bytes)) else {
        return 1;
    };
    match exif.get_field(Tag::Orientation, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Short(values)) => values.first().copied().unwrap_or(1),
        _ => 1,
    }
}

/// EXIF Orientation を画素に焼き込む。**焼き込まないと一覧だけ横倒しになる。**
pub fn apply_orientation(image: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

/// EXIF の APP1 に埋め込まれたサムネイル JPEG を取り出してデコードする。
/// Orientation も一緒に返す（IFD1 優先、無ければ IFD0）。
pub fn exif_thumbnail_image_bytes(bytes: &[u8]) -> Option<(DynamicImage, u16)> {
    let tiff = exif::get_exif_attr_from_jpeg(&mut std::io::Cursor::new(bytes)).ok()?;
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
    if length == 0 {
        return None;
    }
    let end = offset.checked_add(length)?;
    if end > tiff.len() {
        return None;
    }
    let image =
        image::load_from_memory_with_format(&tiff[offset..end], image::ImageFormat::Jpeg).ok()?;
    let orientation = orientation_field(&fields, In::THUMBNAIL)
        .or_else(|| orientation_field(&fields, In::PRIMARY))
        .unwrap_or(1);
    (image.width().max(image.height()) >= MIN_EXIF_THUMBNAIL_EDGE).then_some((image, orientation))
}

/// jpeg-decoder の IDCT スケーリングで 1/8 相当まで小さくデコードする。
pub fn scaled_jpeg_decode_bytes(bytes: &[u8]) -> Option<DynamicImage> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    decoder.scale(info.width.div_ceil(8), info.height.div_ceil(8)).ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}
