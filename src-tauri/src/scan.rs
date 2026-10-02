//! 走査の規則（目印 fingerprint・拡張子・中身の判定・RAW＋JPEG の組の RAW を除く・フォルダの列挙）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

pub(crate) fn fingerprint(path: &Path) -> Option<(i64, i64)> {
    fingerprint_of(&fs::metadata(path).ok()?)
}

/// 「変わったか」の目印の定義: (更新時刻ミリ秒, 大きさ)。走査の列挙で得た情報と
/// `fingerprint(path)` が同じ値になるよう、作り方をここ 1 か所にする。
pub(crate) fn fingerprint_of(metadata: &fs::Metadata) -> Option<(i64, i64)> {
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((mtime, metadata.len() as i64))
}

/// 画像・RAW の名前の拡張子。拡張子は「候補に入れるか」だけに使い、
/// 画像かどうかは中身で決める（`content_is_not_image`）。
pub(crate) const IMAGE_EXTENSIONS: [&str; 13] = [
    "jpg", "jpeg", "png", "webp", "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];
/// 拡張子が動画のものは、中身に関わらず常に除く。
pub(crate) const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "mov", "m4v", "avi", "mts", "m2ts", "3gp"];
/// 中身が JPEG・PNG・WebP でなくても「画像だが今は読めない」に数える拡張子。
pub(crate) const OTHER_IMAGE_EXTENSIONS: [&str; 9] = [
    "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];

pub(crate) fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

/// RAW の拡張子（`IMAGE_EXTENSIONS` のうち HEIC・HEIF 以外。rw2・pef・srw は今は走査の
/// 候補に入らないが、入れる日のために並べておく）。
pub(crate) const RAW_EXTENSIONS: [&str; 10] = [
    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
];

/// RAW＋JPEG 同時撮影の「組」の RAW を除く（U46）。同じフォルダに、拡張子を除いた名前が
/// 大文字小文字を無視して一致する JPEG（.jpg・.jpeg）がある RAW は、写真に数えない。
/// Android は jpg/png/webp だけを走査するので、これで 2 台の顔ぶれが揃う。
/// 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組は除かない。
/// `enabled` はプロジェクトの設定（`pair_raw_jpeg`）。false なら何も除かない。
pub(crate) fn skip_paired_raw<T>(items: Vec<T>, enabled: bool, path_of: impl Fn(&T) -> &Path) -> Vec<T> {
    if !enabled {
        return items;
    }
    let key_of = |path: &Path| -> Option<(PathBuf, String)> {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        Some((path.parent().map(Path::to_path_buf).unwrap_or_default(), stem))
    };
    let jpeg_keys: HashSet<(PathBuf, String)> = items
        .iter()
        .filter_map(|item| {
            let path = path_of(item);
            let ext = extension_lower(path)?;
            if ext == "jpg" || ext == "jpeg" {
                key_of(path)
            } else {
                None
            }
        })
        .collect();
    items
        .into_iter()
        .filter(|item| {
            let path = path_of(item);
            let is_raw = extension_lower(path)
                .map(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
                .unwrap_or(false);
            !(is_raw && key_of(path).is_some_and(|key| jpeg_keys.contains(&key)))
        })
        .collect()
}

/// 拡張子が画像・RAW の名前か、拡張子が無いもの。動画の拡張子は常に除く。
pub(crate) fn is_supported(path: &Path) -> bool {
    match extension_lower(path) {
        Some(ext) => {
            !VIDEO_EXTENSIONS.contains(&ext.as_str()) && IMAGE_EXTENSIONS.contains(&ext.as_str())
        }
        None => true,
    }
}

/// 中身が画像ではない（動画・テキスト・壊れたファイルなど）。先頭バイトで決める。
/// JPEG・PNG・WebP 以外の画像（HEIC・RAW）は先頭バイトでは見分けられないので、
/// 拡張子がその種類なら画像として通し、「読めなかった」に数える（解析で失敗する）。
/// HEIC・CR3 は `ftyp` で始まり、`sniff` は動画と判定するため、この扱いが要る。
pub(crate) fn content_is_not_image(head: &[u8], path: &Path) -> bool {
    let kind = format::sniff(head);
    if format::is_image(kind) {
        return false;
    }
    let other_image = extension_lower(path)
        .map(|ext| OTHER_IMAGE_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false);
    !other_image
}

/// 隠しフォルダ・隠しファイル（名前が `.` で始まる）。NAS の `.webaxs` のような
/// サムネイルのキャッシュを原本として数えないために、走査の入口で除く。
pub(crate) fn is_hidden_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.')
}

/// 列挙で見つかった 1 枚。ハッシュ値ではなく「変わったか」の目印（更新時刻・大きさ）を、
/// 列挙で得た情報から作って持つ。
pub(crate) struct ListedFile {
    pub(crate) path: PathBuf,
    /// `fingerprint(path)` と同じ定義の (更新時刻ミリ秒, 大きさ)。読めなければ None。
    pub(crate) fingerprint: Option<(i64, i64)>,
}

/// フォルダの列挙の結果。`unreadable` は、列挙の途中で読めなかった場所の数
/// （権限・切断など）。1 件でもあれば、列挙は「全部は見えていない」。
pub(crate) struct FolderListing {
    pub(crate) files: Vec<ListedFile>,
    pub(crate) unreadable: usize,
}

/// 走査の入口。フォルダ自体が開けなければ（NAS の切断・共有の取り外し・移動）、
/// 空のフォルダとして通さずエラーにする。
///
/// 更新時刻・大きさは、ディレクトリの列挙で得た情報（Windows では列挙の結果に
/// 入っている）から取る。ファイルごとに `stat` をやり直すと、ネットワークの
/// フォルダでは 1 枚 1 往復になる。
pub(crate) fn list_photo_files(folder: &str, pair_raw: bool) -> Result<FolderListing, String> {
    fs::read_dir(folder)
        .map_err(|error| format!("フォルダに接続できませんでした（{folder}）: {error}"))?;
    let mut files = Vec::new();
    let mut unreadable = 0;
    for entry in WalkDir::new(folder)
        .into_iter()
        .filter_entry(|entry| !is_hidden_entry(entry))
    {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() && is_supported(entry.path()) {
                    let fingerprint = entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| fingerprint_of(&metadata));
                    files.push(ListedFile {
                        path: entry.into_path(),
                        fingerprint,
                    });
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    // RAW＋JPEG 同時撮影の組の RAW は写真に数えない（U46）。
    let files = skip_paired_raw(files, pair_raw, |file| file.path.as_path());
    Ok(FolderListing { files, unreadable })
}
