//! デコード・サムネイル・表示用画像・指紋（設計 07 章 段4-2）。
//!
//! **指紋は `photo_curator_core::d_hash_from_gray` で作る**（2026-09-23 決定）。
//! 画素を集めるところまでがここの仕事で、64bit に畳む計算そのものは core に置く
//! （Android・Web と答えを揃えるため）。

use std::fs;
use std::path::Path;

use image::DynamicImage;

use crate::exif_util::{
    apply_orientation, exif_orientation_bytes, exif_thumbnail_image_bytes, scaled_jpeg_decode_bytes,
    EXIF_HEAD_PROBE,
};

pub const THUMBNAIL_MAX_EDGE: u32 = 256;
const THUMBNAIL_QUALITY: u8 = 82;
const DISPLAY_QUALITY: u8 = 82;
/// 選べる長辺。ここに無い値は既定に丸める（設計 02 章の設定画面）。
pub const DISPLAY_EDGES: [u32; 5] = [768, 1024, 1280, 1536, 1920];
pub const DISPLAY_EDGE_DEFAULT: u32 = 1024;

pub fn normalize_display_edge(edge: u32) -> u32 {
    DISPLAY_EDGES
        .into_iter()
        .find(|candidate| *candidate == edge)
        .unwrap_or(DISPLAY_EDGE_DEFAULT)
}

/// dHash・サムネイルのもとになる画像を、**必要な画素数だけ**取り出す。
/// ① EXIF 埋め込みサムネイル → ② JPEG の 1/8 スケールデコード → ③ フルデコード。
/// Orientation は必ず焼き込み済みで返す。
pub fn decode_hash_source(path: &Path, head: &[u8]) -> Option<DynamicImage> {
    if let Some((image, orientation)) = exif_thumbnail_image_bytes(head) {
        return Some(apply_orientation(image, orientation));
    }
    let maybe_truncated = head.len() >= EXIF_HEAD_PROBE;
    let mut full: Option<Vec<u8>> = None;
    if maybe_truncated {
        full = fs::read(path).ok();
        if let Some((image, orientation)) = full.as_deref().and_then(exif_thumbnail_image_bytes) {
            return Some(apply_orientation(image, orientation));
        }
    }
    let orientation = exif_orientation_bytes(head);
    let bytes = match full {
        Some(bytes) => bytes,
        None => fs::read(path).ok()?,
    };
    if let Some(image) = scaled_jpeg_decode_bytes(&bytes) {
        return Some(apply_orientation(image, orientation));
    }
    image::load_from_memory(&bytes)
        .ok()
        .map(|image| apply_orientation(image, orientation))
}

/// 原本をフルで読み、Orientation を焼き込んで返す（表示用画像づくり専用）。
pub fn decode_full(path: &Path, head: &[u8]) -> Option<DynamicImage> {
    let bytes = fs::read(path).ok()?;
    let orientation = exif_orientation_bytes(head);
    image::load_from_memory(&bytes)
        .ok()
        .map(|image| apply_orientation(image, orientation))
}

pub fn scale_for_thumbnail(image: &DynamicImage) -> DynamicImage {
    if image.width().max(image.height()) <= THUMBNAIL_MAX_EDGE {
        image.clone()
    } else {
        image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
    }
}

pub fn encode_thumbnail(image: &DynamicImage) -> Option<Vec<u8>> {
    encode_jpeg(image, THUMBNAIL_QUALITY)
}

/// 表示用画像。長辺を `edge` に収め、原本より大きくはしない。
pub fn encode_display(image: &DynamicImage, edge: u32) -> Option<Vec<u8>> {
    let scaled = if image.width().max(image.height()) <= edge {
        image.clone()
    } else {
        image.thumbnail(edge, edge)
    };
    encode_jpeg(&scaled, DISPLAY_QUALITY)
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(bytes)
}

/// 指紋。**必ず core を通す**（Android・Web と同じ計算式にするため）。
/// 大きい画像のまま平均を取ると遅いので、先に軽く縮めてから渡す
/// （core 側でさらに 9x8 へ平均される。段の縮小は結果を変えない範囲に留める）。
pub fn d_hash_of(image: &DynamicImage) -> Option<String> {
    let scaled = if image.width().max(image.height()) > 128 {
        image.thumbnail(128, 128)
    } else {
        image.clone()
    };
    let gray = scaled.to_luma8();
    let (width, height) = (gray.width(), gray.height());
    photo_curator_core::d_hash_from_gray(gray.into_raw(), width, height)
}
