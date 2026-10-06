//! 撮影時刻（EXIF・ファイル名）。U54 で lib.rs から移した（中身は変えていない）。

use super::*;

// ---------------------------------------------------------------------------
// 撮影時刻
// ---------------------------------------------------------------------------

/// `captured_at` をどの経路で得たか。連写判定でどれだけ信用してよいかが変わる。
///
/// 特に `FilesystemMtime` は撮影時刻ではない。コピー・ダウンロード・展開で
/// 大量のファイルが同一 mtime を持つため、これを連写の根拠にすると
/// フォルダ全体が候補になってしまう。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampSource {
    ExifOriginal,
    ExifDateTime,
    FilenameInferred,
    FilesystemMtime,
    Unknown,
}

impl TimestampSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExifOriginal => "exif_original",
            Self::ExifDateTime => "exif_datetime",
            Self::FilenameInferred => "filename_inferred",
            Self::FilesystemMtime => "filesystem_mtime",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("exif_original") => Self::ExifOriginal,
            Some("exif_datetime") => Self::ExifDateTime,
            Some("filename_inferred") => Self::FilenameInferred,
            Some("filesystem_mtime") => Self::FilesystemMtime,
            _ => Self::Unknown,
        }
    }

    /// 連写の根拠としては弱い経路かどうか。
    pub(crate) fn is_weak(self) -> bool {
        matches!(self, Self::FilesystemMtime | Self::Unknown)
    }
}

pub struct CaptureTime {
    pub(crate) at: i64,
    pub(crate) source: TimestampSource,
}

/// 写真の実体をどこから取るか。**解析コードはこれ以外を知らない。**
///
/// 要点は `head` があること。EXIF 埋め込みサムネイル経路は**先頭 26KB 程度で
/// 用が済む**（実データで計測）。ここを `all` に一本化すると、Android から
/// NAS 越しに読むときに 1 枚 6.7MB を落とすことになり、転送量が 260 倍になる。
/// デスクトップでは OS の SMB クライアントが同じことを黙ってやってくれていた。
pub trait PhotoSource {
    /// 先頭 `want` バイト。ファイルがそれより短ければあるだけ返す。
    fn head(&self, want: usize) -> Option<Vec<u8>>;
    /// 全体。EXIF サムネイルが無い写真だけがここへ落ちる。
    fn all(&self) -> Option<Vec<u8>>;
    /// mtime と size。**既存のキャッシュ無効化がそのまま効く。**
    fn fingerprint(&self) -> Option<(i64, i64)>;
    /// ファイル名。EXIF が無いときの撮影時刻の手がかりになる。
    fn name(&self) -> Option<String>;
}

/// EXIF を読むために先に取る量。実データでは APP1 が先頭 26KB で終わるので
/// 64KB あれば 1 往復で足りる。足りなかったときだけ全体を取り直す。
pub(crate) const EXIF_HEAD_PROBE: usize = 64 * 1024;

/// ローカルのファイル。デスクトップはこれだけを使う。
pub struct LocalPhoto<'a>(pub &'a Path);

impl PhotoSource for LocalPhoto<'_> {
    fn head(&self, want: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let file = File::open(self.0).ok()?;
        let mut buffer = Vec::new();
        // `take` で読む量を区切る。**ここを fs::read にすると、EXIF だけ見たい
        // ときにも全体を読んでしまう。**
        BufReader::new(file)
            .take(want as u64)
            .read_to_end(&mut buffer)
            .ok()?;
        Some(buffer)
    }

    fn all(&self) -> Option<Vec<u8>> {
        fs::read(self.0).ok()
    }

    fn fingerprint(&self) -> Option<(i64, i64)> {
        fingerprint(self.0)
    }

    fn name(&self) -> Option<String> {
        self.0.file_name()?.to_str().map(str::to_owned)
    }
}

/// 撮影時刻を、根拠の強い順に探す。
/// EXIF → ファイル名 → mtime。ファイル名を mtime より優先するのは、
/// 書き出しや転送で EXIF が落ちても `20260630_181932` の類は残ることが多く、
/// mtime よりはるかに撮影時刻に近いため。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read_capture_time_from(source: &dyn PhotoSource) -> Option<CaptureTime> {
    capture_time_from_head(source, source.head(EXIF_HEAD_PROBE).as_deref())
}

/// 先頭 64KB を読み済みのときの撮影時刻。先頭を 2 回読まないために分けてある。
pub(crate) fn capture_time_from_head(source: &dyn PhotoSource, head: Option<&[u8]>) -> Option<CaptureTime> {
    head.and_then(exif_capture_time_bytes)
        .or_else(|| {
            let name = source.name()?;
            let stem = Path::new(&name).file_stem()?.to_str()?.to_owned();
            filename_capture_time_of(&stem).map(|at| CaptureTime {
                at,
                source: TimestampSource::FilenameInferred,
            })
        })
        .or_else(|| {
            source.fingerprint().map(|(mtime, _)| CaptureTime {
                at: mtime,
                source: TimestampSource::FilesystemMtime,
            })
        })
}

#[cfg_attr(not(feature = "bench"), allow(dead_code))]
pub(crate) fn read_capture_time(path: &Path) -> Option<CaptureTime> {
    read_capture_time_from(&LocalPhoto(path))
}

pub(crate) fn ascii_field(exif: &exif::Exif, tag: Tag) -> Option<Vec<u8>> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(values) => values.first().cloned(),
        _ => None,
    }
}

// 以前は `display_value()` が整形した文字列から数字を拾っていた。表示用の
// 文字列に依存していたうえ timezone を無視し、月や日の範囲も検証していなかった
// ため、壊れた EXIF が「それらしい値」に化けるか、失敗して mtime fallback に
// 落ちて候補爆発を誘発していた。ここでは生の ASCII を規格どおりに解釈する。
pub(crate) fn exif_capture_time_bytes(bytes: &[u8]) -> Option<CaptureTime> {
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

/// `YYYY:MM:DD HH:MM:SS` と `+09:00` 形式のオフセットを UTC のミリ秒にする。
/// `DateTime::from_ascii` は範囲を検証しない（13月を返しうる）ので、
/// 妥当性の確認は `civil_timestamp_ms` 側で必ず行う。
pub(crate) fn exif_timestamp_ms(datetime: &[u8], offset: Option<&[u8]>) -> Option<i64> {
    let mut parsed = exif::DateTime::from_ascii(datetime).ok()?;
    if let Some(offset) = offset {
        // オフセットが壊れていても日時そのものは使う。
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
    // オフセットが無い EXIF は「現地時刻だが地域は不明」。連写判定は差分しか
    // 見ないため、UTC とみなしても同一フォルダ内の相対関係は壊れない。
    Some(local - i64::from(parsed.offset.unwrap_or(0)) * 60_000)
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// 暦日から Unix epoch までの日数（Howard Hinnant の days_from_civil）。
/// 以前の自前計算はうるう年の加算が 1〜2月でずれていた。
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(crate) fn civil_timestamp_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<i64> {
    if !(1900..=2999).contains(&year) {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // うるう秒で 60 を書く機材があるため秒だけ 60 を許す。
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
}

pub(crate) fn digit_groups(value: &str) -> Vec<&str> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect()
}

pub(crate) fn split_fixed(text: &str, widths: &[usize]) -> Option<Vec<i64>> {
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

pub(crate) fn timestamp_from_groups(groups: &[&str]) -> Option<i64> {
    let lengths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
    let parts = match lengths.as_slice() {
        // 20260630181932
        [14, ..] => split_fixed(groups[0], &[4, 2, 2, 2, 2, 2])?,
        // IMG_20260630_181932
        [8, 6, ..] => {
            let mut parts = split_fixed(groups[0], &[4, 2, 2])?;
            parts.extend(split_fixed(groups[1], &[2, 2, 2])?);
            parts
        }
        // 2026-06-30_18-19-32
        [4, 2, 2, 2, 2, 2, ..] => groups[..6]
            .iter()
            .map(|group| group.parse().ok())
            .collect::<Option<Vec<i64>>>()?,
        _ => return None,
    };
    civil_timestamp_ms(parts[0], parts[1], parts[2], parts[3], parts[4], parts[5])
}

/// ファイル名から撮影時刻らしい並びを読む。`2026-06-30_18-19-32` /
/// `20260630_181932` / `IMG_20260630_181932` に対応する。
/// 数字の並びとして成立していても暦として不正なら採らない。
pub(crate) fn filename_capture_time_of(stem: &str) -> Option<i64> {
    let groups = digit_groups(stem);
    (0..groups.len()).find_map(|start| timestamp_from_groups(&groups[start..]))
}

/// パスから拡張子を落として上に渡すだけ。本番の経路は `PhotoSource::name()`
/// から名前を受け取るので、こちらはテストの読みやすさのために残している。
#[cfg(test)]
pub(crate) fn filename_capture_time(path: &Path) -> Option<i64> {
    filename_capture_time_of(path.file_stem()?.to_str()?)
}
