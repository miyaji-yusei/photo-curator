//! デコード・向き・サムネイル・表示用画像・dHash と 1 枚の解析（analyse_photo）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

// ---------------------------------------------------------------------------
// デコードとサムネイル
// ---------------------------------------------------------------------------

/// dHash のもとになる画素をどこから取ったか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeSource {
    ExifThumbnail,
    JpegScaled,
    FullDecode,
}

impl DecodeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExifThumbnail => "exif_thumbnail",
            Self::JpegScaled => "jpeg_scaled",
            Self::FullDecode => "full_decode",
        }
    }
}

/// 既に読み込んだ EXIF のフィールド列から、指定した IFD の Orientation を取る。
pub(crate) fn orientation_field(fields: &[exif::Field], ifd: In) -> Option<u16> {
    fields
        .iter()
        .find(|field| field.tag == Tag::Orientation && field.ifd_num == ifd)
        .and_then(|field| match &field.value {
            Value::Short(values) => values.first().copied(),
            _ => None,
        })
}

/// 原本の EXIF Orientation。読めなければ 1（無変換）を返す。
///
/// IFD0 は TIFF ブロックの先頭近くにあるので、先頭だけ読めていれば足りる。
pub(crate) fn exif_orientation_bytes(bytes: &[u8]) -> u16 {
    let Ok(exif) = Reader::new().read_from_container(&mut std::io::Cursor::new(bytes)) else {
        return 1;
    };
    match exif.get_field(Tag::Orientation, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Short(values)) => values.first().copied().unwrap_or(1),
        _ => 1,
    }
}

/// EXIF Orientation を画素に焼き込んで、見たままの向きにする。
///
/// **これをしないと一覧だけが横倒しになる。** 一覧はここで作ったサムネイルを
/// 表示し、選別・拡大は原本を `<img>` に渡す。WebView は原本の Orientation を
/// 自動で適用するので、焼き込まないほうだけが回転しない状態になる。
///
/// 値の意味は EXIF 規格のとおり。`rotate90` は時計回り。
pub(crate) fn apply_orientation(image: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        // 1（無変換）と、規格外の値。壊れた EXIF で画像を回さない。
        _ => image,
    }
}

/// EXIF の APP1 に埋め込まれたサムネイル JPEG を取り出してデコードする。
/// IFD1 の JPEGInterchangeFormat（オフセット）と同 Length が実体を指す。
/// 読むのは APP1 セグメントまでで、本体の画素には一切触れない。
///
/// Orientation も一緒に返す。**IFD1 のものを優先する。** 埋め込みサムネイルを
/// 既に正立させて保存するカメラがあり、そこで IFD0 の値を当てると二重に回る。
/// IFD1 に無ければ本体（IFD0）の値に従う。
pub(crate) fn exif_thumbnail_image_bytes(bytes: &[u8]) -> Option<(DynamicImage, u16)> {
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
    // 極端に小さいサムネイルは dHash も表示も成立しない。次の経路へ落とす。
    (image.width().max(image.height()) >= MIN_EXIF_THUMBNAIL_EDGE)
        .then_some((image, orientation))
}

/// jpeg-decoder の IDCT スケーリングで 1/8 相当まで小さくデコードする。
/// `scale()` は 1/8・1/4・1/2・1/1 のうち要求以上で最小のものを選ぶ。
/// image 0.25 のバックエンド zune-jpeg は 1/8 デコードを提供しないため、
/// この経路のためだけに jpeg-decoder を併用している。
pub(crate) fn scaled_jpeg_decode_bytes(bytes: &[u8]) -> Option<DynamicImage> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    decoder
        .scale(info.width.div_ceil(8), info.height.div_ceil(8))
        .ok()?;
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

/// dHash とサムネイルのもとになる画像を、**必要な画素数だけ**取り出す。
///
/// dHash が要るのは 64bit だけなのに、以前は 24Mpx を丸ごと展開したうえで
/// `resize_exact` が全画素を舐めていた。Routine 1 の実測（実画像100枚）:
///
/// | 方式 | ms/枚 | フル版とのペア判定の反転 |
/// |---|---:|---:|
/// | ① フルデコード + resize_exact | 132.27 | ―（基準）|
/// | ② EXIF サムネイル | **1.72** | 4/99 |
/// | ③ JPEG scaled decode (1/8) | 39.80 | 4/99 |
///
/// ②は③より 23 倍速く、判定の壊れ方は同じだったので②を主経路に置く。
/// ③は JPEG 専用なので、PNG/WebP とサムネイル非搭載 JPEG は①に落ちる。
///
/// どの経路を通っても、返す時点で **Orientation は焼き込み済み**。
///
/// **①は先頭 64KB しか読まない。**②③に落ちたときだけ全体を取る。
/// 実データでは 97.4% が①なので、読む量は 1 枚あたり 26KB で収まる。
pub(crate) fn decode_hash_source_from(source: &dyn PhotoSource) -> Option<(DynamicImage, DecodeSource)> {
    decode_hash_source_with(source, true)
}

/// `pixel_fallback` が false のときは、EXIF のサムネイルが取れなければここで諦める
/// （生の画素の読み込みへ進まない）。RAW 用: `image` は RAW を開けないので、
/// 25MB 前後の本体を丸ごと読んでから失敗するのは、ネットワークでは時間の無駄でしかない。
pub(crate) fn decode_hash_source_with(
    source: &dyn PhotoSource,
    pixel_fallback: bool,
) -> Option<(DynamicImage, DecodeSource)> {
    let head = source.head(EXIF_HEAD_PROBE)?;
    // 先頭が上限いっぱいなら、APP1 がまだ続いている可能性がある。
    let maybe_truncated = head.len() >= EXIF_HEAD_PROBE;
    let mut full: Option<Vec<u8>> = None;

    if let Some((image, orientation)) = exif_thumbnail_image_bytes(&head) {
        return Some((
            apply_orientation(image, orientation),
            DecodeSource::ExifThumbnail,
        ));
    }
    if maybe_truncated && pixel_fallback {
        // APP1 が 64KB に収まらないカメラ。全体を読み直して一度だけ試す。
        full = source.all();
        if let Some((image, orientation)) = full.as_deref().and_then(exif_thumbnail_image_bytes) {
            return Some((
                apply_orientation(image, orientation),
                DecodeSource::ExifThumbnail,
            ));
        }
    }

    if !pixel_fallback {
        return None;
    }
    // ここから先は生の画素なので、本体（IFD0）の Orientation をそのまま当てる。
    // IFD0 は TIFF ブロックの先頭近くなので、先頭だけで読める。
    let orientation = exif_orientation_bytes(&head);
    let bytes = match full {
        Some(bytes) => bytes,
        None => source.all()?,
    };
    if let Some(image) = scaled_jpeg_decode_bytes(&bytes) {
        return Some((
            apply_orientation(image, orientation),
            DecodeSource::JpegScaled,
        ));
    }
    image::load_from_memory(&bytes)
        .ok()
        .map(|image| (apply_orientation(image, orientation), DecodeSource::FullDecode))
}

pub(crate) fn decode_hash_source(path: &Path) -> Option<(DynamicImage, DecodeSource)> {
    let is_raw = extension_lower(path).is_some_and(|ext| RAW_EXTENSIONS.contains(&ext.as_str()));
    decode_hash_source_with(&LocalPhoto(path), !is_raw)
}

/// 選べる長辺に丸める。設定ファイルや古いセッションから変な値が来ても、
/// 生成する画像の大きさが暴れないようにする。
pub(crate) fn normalize_display_edge(edge: i64) -> u32 {
    let edge = edge.clamp(0, u32::MAX as i64) as u32;
    DISPLAY_EDGES
        .into_iter()
        .find(|candidate| *candidate == edge)
        .unwrap_or(DISPLAY_EDGE_DEFAULT)
}

pub(crate) fn display_file(dir: &Path, photo_id: &str) -> PathBuf {
    dir.join(format!("{photo_id}.jpg"))
}

/// 表示用画像を作る。長辺を `edge` に収め、原本より大きくはしない。
pub(crate) fn encode_display(image: &DynamicImage, edge: u32) -> Option<Vec<u8>> {
    let scaled = if image.width().max(image.height()) <= edge {
        image.clone()
    } else {
        image.thumbnail(edge, edge)
    };
    let rgb = scaled.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, DISPLAY_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(bytes)
}

/// 表示用画像を 1 枚ぶん用意する。
///
/// **要点は「下げるときは原本に戻らない」こと。** 既に保存してある表示用画像が
/// 要求より大きければ、それを縮めれば足りる。1536 → 1024 の切り替えで
/// 2,000 枚ぶん 13.4GB を読み直すのは無駄でしかない。
/// 逆に上げるときは、小さい画像から大きい画像は作れないので原本へ戻る。
pub(crate) fn build_display(
    source: &dyn PhotoSource,
    edge: u32,
    existing: Option<(&Path, u32)>,
) -> Option<Vec<u8>> {
    if let Some((path, stored_edge)) = existing {
        if stored_edge >= edge && path.is_file() {
            if let Some(image) = fs::read(path)
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
            {
                return encode_display(&image, edge);
            }
        }
    }
    // 原本から作る。**ここだけが全体を読む。**
    // Orientation は decode_hash_source_from と同じ規則で焼き込む。
    let bytes = source.all()?;
    let orientation = source
        .head(EXIF_HEAD_PROBE)
        .map(|head| exif_orientation_bytes(&head))
        .unwrap_or(1);
    let image = image::load_from_memory(&bytes).ok()?;
    encode_display(&apply_orientation(image, orientation), edge)
}

pub(crate) fn scale_for_thumbnail(image: &DynamicImage) -> DynamicImage {
    if image.width().max(image.height()) <= THUMBNAIL_MAX_EDGE {
        // EXIF サムネイルは 160x120 前後。引き伸ばしても情報は増えない。
        return image.clone();
    }
    image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
}

pub(crate) fn encode_thumbnail(image: &DynamicImage) -> Option<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(bytes)
}

/// ハッシュ値。**必ず core を通す**（Android・Web と同じ計算式にするため）。
/// 大きい画像のまま平均を取ると遅いので、先に軽く縮めてから渡す。
/// 作れないとき（極端に小さい画像など）は None で、0 を入れない。
pub(crate) fn d_hash_of(image: &DynamicImage) -> Option<String> {
    let scaled = if image.width().max(image.height()) > 128 {
        image.thumbnail(128, 128)
    } else {
        image.clone()
    };
    let gray = scaled.to_luma8();
    let (width, height) = (gray.width(), gray.height());
    photo_curator_core::d_hash_from_gray(gray.into_raw(), width, height)
}

/// dHash は**必ず保存するサムネイルのバイト列から**計算する。
/// 生成直後とキャッシュヒット時で必ず同じ値になることを保証するため、
/// JPEG 符号化の往復を挟んだあとの画素を使う。
pub(crate) fn hash_thumbnail_bytes(bytes: &[u8]) -> Option<String> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .ok()
        .and_then(|image| d_hash_of(&image))
}

/// キャッシュを使わずに1枚ぶんのハッシュを出す。計測ハーネス用。
/// キャッシュ経路（`analyse_photo`）と同じ値を返す。
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
pub(crate) fn d_hash(path: &Path) -> Option<String> {
    let (image, _) = decode_hash_source(path)?;
    hash_thumbnail_bytes(&encode_thumbnail(&scale_for_thumbnail(&image))?)
}

/// DB に保存済みの解析結果。キャッシュの有効性判定に使う。
#[derive(Default, Clone, Debug)]
pub struct CachedAnalysis {
    pub d_hash: Option<String>,
    pub d_hash_version: Option<i64>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_mtime: Option<i64>,
    pub thumbnail_size: Option<i64>,
    pub thumbnail_version: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailState {
    /// 生成済みのサムネイルをそのまま使えた。
    Hit,
    /// 作り直した。どの経路でデコードしたかを持つ。
    Generated(DecodeSource),
    /// 壊れた画像・非対応形式・権限エラーなど。1枚失敗しても解析は続く。
    Failed,
}

impl ThumbnailState {
    /// 作り直したときだけ、使ったデコード経路の名前を返す。
    /// キャッシュヒットでは経路が分からないので、DB 側の値を据え置く。
    pub(crate) fn decode_source(self) -> Option<&'static str> {
        match self {
            Self::Generated(source) => Some(source.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisOutcome {
    pub d_hash: Option<String>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_state: ThumbnailState,
    /// DB に書き戻す必要すらなかった（ハッシュもサムネイルも据え置き）。
    pub hash_reused: bool,
}

pub(crate) fn thumbnail_file(dir: &Path, photo_id: &str) -> PathBuf {
    dir.join(format!("{photo_id}.jpg"))
}

/// 1枚ぶんのサムネイル生成と dHash。
///
/// サムネイルは一度だけ作って使い回す。無効化は Session 0 で直した
/// fingerprint（mtime/size）で判定する。ファイルが読めず fingerprint が
/// 取れない場合はキャッシュを信用しない（読めないファイル同士が同じ
/// fingerprint に見えて誤ヒットするのを避けるため）。
pub(crate) fn analyse_photo(
    thumbnail_dir: &Path,
    photo_id: &str,
    source: &Path,
    current: Option<(i64, i64)>,
    cached: &CachedAnalysis,
) -> AnalysisOutcome {
    let file = thumbnail_file(thumbnail_dir, photo_id);
    let stored = file.to_string_lossy().to_string();
    let failed = || AnalysisOutcome {
        d_hash: None,
        thumbnail_path: None,
        thumbnail_state: ThumbnailState::Failed,
        hash_reused: false,
    };

    let usable = match current {
        Some((mtime, size)) => {
            cached.thumbnail_mtime == Some(mtime)
                && cached.thumbnail_size == Some(size)
                && cached.thumbnail_path.as_deref() == Some(stored.as_str())
                // 生成方式が変わっていたら、ファイルが残っていても使わない。
                // fingerprint は「原本が変わっていない」ことしか見ておらず、
                // こちら側の作り方の変更を検知できない。
                && cached.thumbnail_version == Some(THUMBNAIL_VERSION)
                && file.is_file()
        }
        None => false,
    };

    if usable {
        // 版が合う d_hash があればデコードすら不要。
        if cached.d_hash.is_some() && cached.d_hash_version == Some(D_HASH_VERSION) {
            return AnalysisOutcome {
                d_hash: cached.d_hash.clone(),
                thumbnail_path: Some(stored),
                thumbnail_state: ThumbnailState::Hit,
                hash_reused: true,
            };
        }
        // サムネイルは使えるがハッシュが旧方式。原本には戻らず作り直す。
        if let Some(hash) = fs::read(&file)
            .ok()
            .as_deref()
            .and_then(hash_thumbnail_bytes)
        {
            return AnalysisOutcome {
                d_hash: Some(hash),
                thumbnail_path: Some(stored),
                thumbnail_state: ThumbnailState::Hit,
                hash_reused: false,
            };
        }
    }

    let Some((image, decode_source)) = decode_hash_source(source) else {
        return failed();
    };
    let Some(bytes) = encode_thumbnail(&scale_for_thumbnail(&image)) else {
        return failed();
    };
    // サムネイルを保存できなくてもハッシュは出せる。次回また作り直すだけで、
    // 解析全体を止める理由にはならない。
    if fs::create_dir_all(thumbnail_dir).is_err() || fs::write(&file, &bytes).is_err() {
        return AnalysisOutcome {
            d_hash: hash_thumbnail_bytes(&bytes),
            thumbnail_path: None,
            thumbnail_state: ThumbnailState::Failed,
            hash_reused: false,
        };
    }
    AnalysisOutcome {
        d_hash: hash_thumbnail_bytes(&bytes),
        thumbnail_path: Some(stored),
        thumbnail_state: ThumbnailState::Generated(decode_source),
        hash_reused: false,
    }
}

#[cfg_attr(not(feature = "bench"), allow(dead_code))]
pub(crate) fn hash_distance(left: &str, right: &str) -> u32 {
    u64::from_str_radix(left, 16)
        .ok()
        .zip(u64::from_str_radix(right, 16).ok())
        .map(|(a, b)| (a ^ b).count_ones())
        .unwrap_or(64)
}
