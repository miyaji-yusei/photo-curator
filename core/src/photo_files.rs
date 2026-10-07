//! 写真のファイル名の規則のうち、3 台（PC・Web・Android）で同じでなければ困るもの（R10）。
//!
//! - RAW の判定（`is_raw_name`）
//! - RAW＋JPEG 同時撮影の「組」の RAW を除く（`skip_paired_raw`・`paired_raw_mask`。U46・U49）
//!
//! **どの名前を写真の候補にするか（拡張子の一覧）はここに無い。** 3 実装で差が残っている
//! （B8: rw2・pef・srw・HEIC）ので、揃えるまでは各環境が持つ。
//!
//! 名前は道筋の文字列（区切りは `/` でも `\` でもよい）。フォルダは最後の区切りまで、
//! ファイル名はその後ろ。拡張子は Rust の `Path::extension` と同じく、**先頭の点は拡張子にしない**
//! （`.cr2` は拡張子なし）。

/// RAW の拡張子（小文字）。HEIC・HEIF は RAW ではない。rw2・pef・srw は PC の走査の候補には
/// 入らないが（B8）、RAW としては数える。
pub const RAW_EXTENSIONS: [&str; 10] = [
    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
];

/// 組になる JPEG の拡張子。**HEIC・PNG・WebP は組にしない。**
const JPEG_EXTENSIONS: [&str; 2] = ["jpg", "jpeg"];

/// (フォルダ, ファイル名)。フォルダの区切りは `/` にそろえる。
fn split_folder(path: &str) -> (String, &str) {
    match path.rfind(['/', '\\']) {
        Some(cut) => (path[..cut].replace('\\', "/"), &path[cut + 1..]),
        None => (String::new(), path),
    }
}

/// (拡張子を除いた名前, 拡張子)。先頭の点は拡張子にしない（`Path::file_stem`・`extension` と同じ）。
fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], Some(&name[dot + 1..])),
        _ => (name, None),
    }
}

/// 拡張子（ASCII の小文字）。無ければ None。
fn extension_lower(path: &str) -> Option<String> {
    let (_, name) = split_folder(path);
    split_extension(name).1.map(|ext| ext.to_ascii_lowercase())
}

/// 道筋（またはファイル名）の拡張子が RAW か。大文字小文字は問わない。
pub fn is_raw_name(name: &str) -> bool {
    extension_lower(name).is_some_and(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
}

/// RAW の拡張子の一覧（小文字）。各環境が「候補に入れる拡張子」を組むときに使う。
pub fn raw_extensions() -> Vec<String> {
    RAW_EXTENSIONS.iter().map(|ext| ext.to_string()).collect()
}

/// 組を見分ける鍵: 同じフォルダで、拡張子を除いた名前（大文字小文字を無視）。
fn pair_key(path: &str) -> (String, String) {
    let (folder, name) = split_folder(path);
    (folder, split_extension(name).0.to_lowercase())
}

/// 道筋ごとに「組の RAW なので除く」なら true（並びは入力と同じ）。
///
/// 同じフォルダに、拡張子を除いた名前が大文字小文字を無視して一致する JPEG（.jpg・.jpeg）がある
/// RAW が true。組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組、RAW 同士は false。
/// 呼ぶ側が自分の型の一覧を、並びを保ったまま絞るための形。
pub fn paired_raw_mask(paths: Vec<String>) -> Vec<bool> {
    let jpeg_keys: std::collections::HashSet<(String, String)> = paths
        .iter()
        .filter(|path| {
            extension_lower(path).is_some_and(|ext| JPEG_EXTENSIONS.contains(&ext.as_str()))
        })
        .map(|path| pair_key(path))
        .collect();
    paths
        .iter()
        .map(|path| is_raw_name(path) && jpeg_keys.contains(&pair_key(path)))
        .collect()
}

/// RAW＋JPEG 同時撮影の「組」の RAW を除いた道筋の一覧（並びは保つ）。
/// `enabled` はプロジェクトの設定（`pair_raw_jpeg`）。false なら何も除かない。
pub fn skip_paired_raw(paths: Vec<String>, enabled: bool) -> Vec<String> {
    if !enabled {
        return paths;
    }
    let mask = paired_raw_mask(paths.clone());
    paths
        .into_iter()
        .zip(mask)
        .filter_map(|(path, skip)| (!skip).then_some(path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn is_raw_name_follows_the_extension() {
        assert!(is_raw_name("IMG_0001.CR2"));
        assert!(is_raw_name("dir/x.dng"));
        assert!(is_raw_name("dir\\x.Srw"));
        assert!(!is_raw_name("x.jpg"));
        assert!(!is_raw_name("x.heic"));
        assert!(!is_raw_name(".cr2"));
        assert!(!is_raw_name("cr2"));
        assert!(!is_raw_name("a.cr2/b"));
    }

    #[test]
    fn skip_paired_raw_keeps_order_and_only_drops_paired_raw() {
        let kept = skip_paired_raw(strings(&["b.cr2", "a.JPG", "a.cr3", "c.cr2"]), true);
        assert_eq!(kept, strings(&["b.cr2", "a.JPG", "c.cr2"]));
        let all = strings(&["a.jpg", "a.cr2"]);
        assert_eq!(skip_paired_raw(all.clone(), false), all);
    }

    #[test]
    fn folders_compare_with_either_separator() {
        let mask = paired_raw_mask(strings(&["d\\a.jpg", "d/a.cr2", "e/a.cr2"]));
        assert_eq!(mask, vec![false, true, false]);
    }

    #[test]
    fn leading_dot_is_not_an_extension() {
        // `.jpg` は拡張子の無い名前なので、`.cr2` の組にならない（`.cr2` も RAW ではない）。
        let mask = paired_raw_mask(strings(&["d/.jpg", "d/.cr2"]));
        assert_eq!(mask, vec![false, false]);
    }
}
