//! RAW に埋め込まれたプレビュー JPEG を探して取り出す（U57）。
//!
//! `image` クレートは RAW を開けないので、カメラが RAW の中に入れておいた JPEG を絵として使う。
//! 規則は Android の `RawPreview.kt`（U49）と同じ。
//!
//! RAW は 25MB 級で、NAS では丸読みが致命的なので、**IFD をたどって要る範囲だけを読む**。
//! 読み方は [`RangeSource`] で受け取る（実ファイルは [`PhotoSource::read_range`]、テストはバイト列）。
//!
//! 対応する形:
//!  - TIFF 系（CR2・NEF・ARW・DNG・PEF・SRW・ORF・RW2）: IFD0・IFD1…の鎖、SubIFD、
//!    `JPEGInterchangeFormat`（0x0201/0x0202）、圧縮が JPEG（6・7）の 1 本だけの strip、
//!    RW2 の `JpgFromRaw`（0x002E）。
//!  - RAF: 先頭の見出しに書かれた JPEG の位置。
//!  - CR3（ISOBMFF）: 最上位の uuid 箱の中の `PRVW`。向きは moov の中の `CMT1`（TIFF の IFD0）。
//!
//! JPEG かどうかは**中身で決める。** 先頭が SOI で、SOF が基本（C0）・拡張（C1）・プログレッシブ（C2）の
//! ものだけ。CR2・DNG の RAW 本体は可逆 JPEG（C3）なので、ここで落ちる。
//! どこかが壊れていても panic せず、**分からなければ空**を返す。

use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

/// `offset` から最大 `length` バイトを読む。ファイルの外なら None、終わりにかかれば短く返す。
pub(crate) trait RangeSource {
    fn read(&self, offset: u64, length: usize) -> Option<Vec<u8>>;
}

/// バイト列そのもの。テスト用。
#[cfg(test)]
pub(crate) struct SliceSource<'a>(pub &'a [u8]);

#[cfg(test)]
impl RangeSource for SliceSource<'_> {
    fn read(&self, offset: u64, length: usize) -> Option<Vec<u8>> {
        let start = usize::try_from(offset).ok()?;
        if start >= self.0.len() {
            return None;
        }
        let end = start.saturating_add(length).min(self.0.len());
        Some(self.0[start..end].to_vec())
    }
}

/// 区画ごとに覚えて読む。**IFD をたどると小さな読みが何十回も起きる**ので、網越しでは
/// 1 回ごとに往復させない。大きな読み（プレビューの本体）は覚えずにそのまま通す。
const BLOCK: usize = EXIF_HEAD_PROBE;
const MAX_BLOCKS: usize = 16;

pub(crate) struct BlockCache<'a> {
    source: &'a dyn PhotoSource,
    blocks: RefCell<HashMap<u64, Vec<u8>>>,
}

impl<'a> BlockCache<'a> {
    /// `head` は既に読んである先頭（先頭の区画として使い、読み直さない）。
    pub(crate) fn new(source: &'a dyn PhotoSource, head: Option<&[u8]>) -> Self {
        let mut blocks = HashMap::new();
        if let Some(head) = head {
            // 区画より長い分は捨てる。短ければファイルの終わりまで。
            blocks.insert(0, head[..head.len().min(BLOCK)].to_vec());
        }
        Self {
            source,
            blocks: RefCell::new(blocks),
        }
    }

    fn block(&self, index: u64) -> Option<Vec<u8>> {
        if let Some(data) = self.blocks.borrow().get(&index) {
            return Some(data.clone());
        }
        let data = self.source.read_range(index.checked_mul(BLOCK as u64)?, BLOCK)?;
        let mut blocks = self.blocks.borrow_mut();
        if blocks.len() >= MAX_BLOCKS {
            blocks.retain(|key, _| *key == 0);
        }
        blocks.insert(index, data.clone());
        Some(data)
    }
}

impl RangeSource for BlockCache<'_> {
    fn read(&self, offset: u64, length: usize) -> Option<Vec<u8>> {
        if length > BLOCK * 4 {
            return self.source.read_range(offset, length);
        }
        let end = offset.checked_add(length as u64)?;
        let mut out = Vec::with_capacity(length);
        let mut at = offset;
        while at < end {
            let index = at / BLOCK as u64;
            let data = self.block(index)?;
            let inside = (at - index * BLOCK as u64) as usize;
            if inside >= data.len() {
                break;
            }
            let take = (data.len() - inside).min((end - at) as usize);
            out.extend_from_slice(&data[inside..inside + take]);
            at += take as u64;
            if data.len() < BLOCK {
                break;
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// 埋め込みの JPEG 1 つ。**大きさは JPEG の SOF から読んだもの。**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JpegRegion {
    pub offset: u64,
    pub length: usize,
    pub width: u32,
    pub height: u32,
}

impl JpegRegion {
    pub(crate) fn long_edge(&self) -> u32 {
        self.width.max(self.height)
    }
    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// RAW を見て分かったこと。`orientation` は EXIF の値（1〜8）、分からなければ None。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawInfo {
    pub previews: Vec<JpegRegion>,
    pub orientation: Option<u16>,
}

/// CR3 のプレビューの入った uuid 箱。
const PRVW_UUID: [u8; 16] = [
    0xea, 0xf4, 0x2b, 0x5e, 0x1c, 0x98, 0x4b, 0x88, 0xb9, 0xfb, 0xb7, 0xdc, 0x40, 0x6e, 0x4d, 0x16,
];
/// CR3 の moov の中の、CMT1〜（TIFF）の入った uuid 箱。
const CMT_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];

const MAX_IFDS: usize = 64;
const MAX_ENTRIES: usize = 1000;
const MAX_SEGMENTS: usize = 200;

/// 読めなくても panic しない。**分からなければ空。**（呼ぶ側は「表示できません」に倒す）
pub(crate) fn inspect(source: &dyn RangeSource) -> RawInfo {
    let Some(head) = source.read(0, 32) else {
        return RawInfo::default();
    };
    if head.len() < 16 {
        return RawInfo::default();
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        raf(source)
    } else if &head[4..8] == b"ftyp" {
        cr3(source)
    } else if head.starts_with(b"II") || head.starts_with(b"MM") {
        tiff(source, 0, true)
    } else {
        RawInfo::default()
    }
}

/// 一番大きいもの。**表示用画像の元。**
pub(crate) fn largest(info: &RawInfo) -> Option<JpegRegion> {
    info.previews
        .iter()
        .copied()
        .max_by_key(|region| (region.area(), region.length))
}

/// サムネイル用。長辺が `min_edge` 以上の中で一番小さいもの（網越しに読む量を減らす）。
/// 足りるものが無ければ一番大きいもの。
pub(crate) fn for_thumb(info: &RawInfo, min_edge: u32) -> Option<JpegRegion> {
    info.previews
        .iter()
        .copied()
        .filter(|region| region.long_edge() >= min_edge)
        .min_by_key(|region| (region.area(), region.length))
        .or_else(|| largest(info))
}

/// JPEG の本体を読む。**全部読めなければ None**（途中で切れた JPEG は渡さない）。
pub(crate) fn bytes_of(source: &dyn RangeSource, region: &JpegRegion) -> Option<Vec<u8>> {
    let bytes = source.read(region.offset, region.length)?;
    (bytes.len() == region.length).then_some(bytes)
}

// ---- TIFF ----

#[derive(Clone, Copy)]
struct Endian {
    little: bool,
}

impl Endian {
    fn u16(self, data: &[u8], at: usize) -> Option<u16> {
        let bytes: [u8; 2] = data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) })
    }
    fn u32(self, data: &[u8], at: usize) -> Option<u32> {
        let bytes: [u8; 4] = data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) })
    }
}

/// `base` からの相対で読む（CR3 の CMT1 は箱の中にあるので）。
struct TiffReader<'a> {
    source: &'a dyn RangeSource,
    endian: Endian,
    base: u64,
}

impl TiffReader<'_> {
    fn bytes(&self, at: u64, length: usize) -> Option<Vec<u8>> {
        let data = self.source.read(self.base.checked_add(at)?, length)?;
        (data.len() == length).then_some(data)
    }
    fn u16(&self, at: u64) -> Option<u16> {
        self.endian.u16(&self.bytes(at, 2)?, 0)
    }
    fn u32(&self, at: u64) -> Option<u32> {
        self.endian.u32(&self.bytes(at, 4)?, 0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Sub,
    Exif,
}

fn tiff(source: &dyn RangeSource, base: u64, previews_too: bool) -> RawInfo {
    tiff_inner(source, base, previews_too).unwrap_or_default()
}

fn tiff_inner(source: &dyn RangeSource, base: u64, previews_too: bool) -> Option<RawInfo> {
    let header = source.read(base, 8).filter(|header| header.len() == 8)?;
    let little = match (header[0], header[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return None,
    };
    let endian = Endian { little };
    let r = TiffReader { source, endian, base };
    let first = endian.u32(&header, 4)?;
    let mut queue: VecDeque<(u64, Kind)> = VecDeque::new();
    queue.push_back((u64::from(first), Kind::Chain));
    let mut visited: Vec<u64> = Vec::new();
    let mut found: Vec<(u64, u64)> = Vec::new();
    let mut orientation = None;
    let mut is_first = true;
    while let Some((at, kind)) = queue.pop_front() {
        if visited.len() >= MAX_IFDS {
            break;
        }
        if at == 0 || visited.contains(&at) {
            continue;
        }
        visited.push(at);
        let Some(count) = r.u16(at).map(usize::from) else { continue };
        if count == 0 || count > MAX_ENTRIES {
            continue;
        }
        let Some(table) = r.bytes(at + 2, count * 12) else { continue };
        let (mut compression, mut strip_offset, mut strip_length, mut strip_count) = (0u32, 0u64, 0u64, 0u64);
        let (mut jpeg_offset, mut jpeg_length) = (0u64, 0u64);
        for i in 0..count {
            let e = i * 12;
            let (Some(tag), Some(value_type), Some(n), Some(short), Some(long)) = (
                endian.u16(&table, e),
                endian.u16(&table, e + 2),
                endian.u32(&table, e + 4),
                endian.u16(&table, e + 8),
                endian.u32(&table, e + 8),
            ) else {
                continue;
            };
            let n = u64::from(n);
            let long = u64::from(long);
            let scalar = if value_type == 3 { u64::from(short) } else { long };
            match tag {
                0x0103 => compression = scalar as u32,
                0x0111 => {
                    strip_offset = scalar;
                    strip_count = n;
                }
                0x0117 => strip_length = scalar,
                0x0112 => {
                    if is_first && kind == Kind::Chain && (1..=8).contains(&short) {
                        orientation = Some(short);
                    }
                }
                0x0201 => jpeg_offset = scalar,
                0x0202 => jpeg_length = scalar,
                0x002E => {
                    if value_type == 7 && n > 4 {
                        found.push((long, n));
                    }
                }
                0x014A => {
                    if (1..=16).contains(&n) {
                        if n == 1 {
                            queue.push_back((long, Kind::Sub));
                        } else if let Some(list) = r.bytes(long, n as usize * 4) {
                            for k in 0..n as usize {
                                if let Some(value) = endian.u32(&list, k * 4) {
                                    queue.push_back((u64::from(value), Kind::Sub));
                                }
                            }
                        }
                    }
                }
                0x8769 => queue.push_back((long, Kind::Exif)),
                _ => {}
            }
        }
        if jpeg_offset > 0 && jpeg_length > 0 {
            found.push((jpeg_offset, jpeg_length));
        }
        if (compression == 6 || compression == 7) && strip_count == 1 && strip_offset > 0 && strip_length > 0 {
            found.push((strip_offset, strip_length));
        }
        if kind == Kind::Chain {
            if let Some(next) = r.u32(at + 2 + count as u64 * 12).filter(|next| *next > 0) {
                queue.push_back((u64::from(next), Kind::Chain));
            }
        }
        is_first = false;
    }
    let previews = if previews_too {
        regions(
            source,
            found
                .into_iter()
                .filter_map(|(offset, length)| Some((base.checked_add(offset)?, length)))
                .collect(),
        )
    } else {
        Vec::new()
    };
    Some(RawInfo { previews, orientation })
}

/// 候補を JPEG として確かめる。**同じ位置は 1 つにする。**
fn regions(source: &dyn RangeSource, candidates: Vec<(u64, u64)>) -> Vec<JpegRegion> {
    let mut seen: Vec<u64> = Vec::new();
    let mut out = Vec::new();
    for (offset, length) in candidates {
        if !(4..=i32::MAX as u64).contains(&length) || seen.contains(&offset) {
            continue;
        }
        seen.push(offset);
        let Some(last) = offset.checked_add(length - 1) else { continue };
        // **ファイルの外を指すものは捨てる。** 最後の 1 バイトが読めるか。
        if source.read(last, 1).is_none_or(|tail| tail.is_empty()) {
            continue;
        }
        let Some((width, height)) = jpeg_size(source, offset, length) else { continue };
        out.push(JpegRegion { offset, length: length as usize, width, height });
    }
    out
}

/// JPEG の大きさ（幅, 高さ）。**区切り（マーカー）を飛ばしながら SOF を探す。**
/// 基本・拡張・プログレッシブ以外（可逆・算術符号など）は None。
pub(crate) fn jpeg_size(source: &dyn RangeSource, offset: u64, length: u64) -> Option<(u32, u32)> {
    let soi = source.read(offset, 2)?;
    if soi.len() < 2 || soi[0] != 0xFF || soi[1] != 0xD8 {
        return None;
    }
    let mut at = offset.checked_add(2)?;
    let end = offset.checked_add(length)?;
    for _ in 0..MAX_SEGMENTS {
        if at.checked_add(4)? > end {
            return None;
        }
        let head = source.read(at, 4).filter(|head| head.len() == 4)?;
        if head[0] != 0xFF {
            return None;
        }
        let marker = head[1];
        match marker {
            0xFF => {
                at += 1;
                continue;
            }
            0xD0..=0xD7 | 0x01 => {
                at += 2;
                continue;
            }
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let segment = u64::from(u16::from_be_bytes([head[2], head[3]]));
        if segment < 2 {
            return None;
        }
        match marker {
            0xC0..=0xC2 => {
                let sof = source.read(at.checked_add(5)?, 4).filter(|sof| sof.len() == 4)?;
                let height = u32::from(u16::from_be_bytes([sof[0], sof[1]]));
                let width = u32::from(u16::from_be_bytes([sof[2], sof[3]]));
                return (width > 0 && height > 0).then_some((width, height));
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            _ => {}
        }
        at = at.checked_add(2 + segment)?;
    }
    None
}

// ---- RAF ----

fn raf(source: &dyn RangeSource) -> RawInfo {
    let Some(header) = source.read(84, 8).filter(|header| header.len() == 8) else {
        return RawInfo::default();
    };
    let offset = u64::from(u32::from_be_bytes([header[0], header[1], header[2], header[3]]));
    let length = u64::from(u32::from_be_bytes([header[4], header[5], header[6], header[7]]));
    RawInfo {
        previews: regions(source, vec![(offset, length)]),
        orientation: None,
    }
}

// ---- CR3（ISOBMFF） ----

struct Boxed {
    at: u64,
    kind: [u8; 4],
    header: u64,
    size: u64,
}

/// `from` から `until` までの箱を並べる。**数と大きさに上限を付ける。**
fn boxes(source: &dyn RangeSource, from: u64, until: Option<u64>) -> Vec<Boxed> {
    let mut out = Vec::new();
    let mut at = from;
    while out.len() < 64 && until.is_none_or(|until| at.saturating_add(8) <= until) {
        let Some(head) = source.read(at, 16) else { break };
        if head.len() < 8 {
            break;
        }
        let mut size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let kind = [head[4], head[5], head[6], head[7]];
        let mut header = 8;
        if size == 1 {
            if head.len() < 16 {
                break;
            }
            size = u64::from_be_bytes([
                head[8], head[9], head[10], head[11], head[12], head[13], head[14], head[15],
            ]);
            header = 16;
        }
        if size == 0 {
            out.push(Boxed { at, kind, header, size: u64::MAX / 4 });
            break;
        }
        if size < header {
            break;
        }
        out.push(Boxed { at, kind, header, size });
        let Some(next) = at.checked_add(size) else { break };
        at = next;
    }
    out
}

fn is_uuid(source: &dyn RangeSource, b: &Boxed, id: &[u8; 16]) -> bool {
    b.kind == *b"uuid"
        && b.at
            .checked_add(b.header)
            .and_then(|at| source.read(at, 16))
            .is_some_and(|read| read == id)
}

fn cr3(source: &dyn RangeSource) -> RawInfo {
    let mut found: Vec<(u64, u64)> = Vec::new();
    let mut orientation = None;
    for b in boxes(source, 0, None) {
        let Some(inner) = b.at.checked_add(b.header) else { continue };
        if is_uuid(source, &b, &PRVW_UUID) {
            // 中身の先頭に PRVW がある。間の詰め物の長さは機種で違いうるので、探す。
            let start = inner + 16;
            let Some(window) = source.read(start, 256) else { continue };
            let Some(index) = window.windows(4).position(|w| w == b"PRVW") else { continue };
            if index < 4 {
                continue;
            }
            let prvw = start + index as u64 - 4;
            let Some(length) = source
                .read(prvw + 20, 4)
                .filter(|v| v.len() == 4)
                .map(|v| u64::from(u32::from_be_bytes([v[0], v[1], v[2], v[3]])))
            else {
                continue;
            };
            found.push((prvw + 24, length));
        } else if &b.kind == b"moov" {
            let until = b.at.saturating_add(b.size);
            for child in boxes(source, inner, Some(until)) {
                if !is_uuid(source, &child, &CMT_UUID) {
                    continue;
                }
                let from = child.at + child.header + 16;
                for cmt in boxes(source, from, Some(child.at.saturating_add(child.size))) {
                    if &cmt.kind == b"CMT1" {
                        orientation = tiff(source, cmt.at + cmt.header, false).orientation;
                    }
                }
            }
        }
    }
    RawInfo { previews: regions(source, found), orientation }
}

// ---- 解析・表示への入口 ----

/// どのプレビューを使うか。
#[derive(Clone, Copy)]
pub(crate) enum Want {
    /// サムネイル・dHash 用。長辺が `THUMBNAIL_MAX_EDGE` 以上で一番小さいもの。
    Thumb,
    /// 表示用画像の元。一番大きいもの。
    Largest,
}

/// RAW のプレビューをデコードして返す。**向きは RAW 本体の Orientation を焼き込み済み**。
/// 先頭（`head`）は読み済みのものを渡す。取り出せない・デコードできない RAW は None。
pub(crate) fn decode_preview(
    source: &dyn PhotoSource,
    head: &[u8],
    want: Want,
) -> Option<DynamicImage> {
    let cache = BlockCache::new(source, Some(head));
    let info = inspect(&cache);
    let region = match want {
        Want::Thumb => for_thumb(&info, THUMBNAIL_MAX_EDGE),
        Want::Largest => largest(&info),
    }?;
    let bytes = bytes_of(&cache, &region)?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).ok()?;
    let orientation = info.orientation.unwrap_or_else(|| exif_orientation_bytes(head));
    Some(apply_orientation(image, orientation))
}
