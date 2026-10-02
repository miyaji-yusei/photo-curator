//! 星の XMP（JPEG の中の XMP パケット・組の RAW の隣の .xmp）と、書いた画像の確かめ。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

/// JPEG の XMP パケットを組み立てる。`xmp:Rating` は 0〜5 をそのまま持てる。
pub(crate) fn xmp_body(rating: i64) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

pub(crate) fn xmp_packet(rating: i64) -> Vec<u8> {
    let body = xmp_body(rating);
    let mut packet = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    packet.extend_from_slice(body.as_bytes());
    packet
}

/// JPEG のセグメント列を組み直し、XMP の APP1 を差し替える（無ければ挿入）。
///
/// 画像本体（SOS 以降）には一切触れない。既存の EXIF セグメントも
/// そのまま残すので、撮影情報は失われない。
pub(crate) fn jpeg_with_rating(original: &[u8], rating: i64) -> Result<Vec<u8>, String> {
    if original.len() < 4 || original[0] != 0xFF || original[1] != 0xD8 {
        return Err("JPEG ではありません".into());
    }
    let packet = xmp_packet(rating);
    if packet.len() + 2 > 0xFFFF {
        return Err("XMP が大きすぎます".into());
    }

    // 走査と組み立てを分ける。1パスで「既存を置換」と「無ければ挿入」を両方
    // やろうとすると、XMP が既にある JPEG で置換と挿入が二重に走り、
    // 書くたびに APP1 が増えていく。
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut insert_after = 0usize; // 何番目のセグメントの後ろに XMP を置くか
    let mut index = 2usize;
    while index + 4 <= original.len() {
        if original[index] != 0xFF {
            break;
        }
        let marker = original[index + 1];
        // SOS 以降は画像本体。ここから先はそのまま写す。
        if marker == 0xDA {
            break;
        }
        let length = u16::from_be_bytes([original[index + 2], original[index + 3]]) as usize;
        if length < 2 || index + 2 + length > original.len() {
            return Err("JPEG の構造が壊れています".into());
        }
        let payload = &original[index + 4..index + 2 + length];
        let is_xmp = marker == 0xE1 && payload.starts_with(b"http://ns.adobe.com/xap/1.0/\0");
        if !is_xmp {
            // 既存の XMP だけを落とす。EXIF などは順番ごと残す。
            kept.push(&original[index..index + 2 + length]);
            if marker == 0xE1 {
                insert_after = kept.len();
            }
        }
        index += 2 + length;
    }

    let mut out = Vec::with_capacity(original.len() + packet.len() + 4);
    out.extend_from_slice(&original[0..2]); // SOI
    let push_packet = |out: &mut Vec<u8>| {
        out.push(0xFF);
        out.push(0xE1);
        out.extend_from_slice(&((packet.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&packet);
    };
    for (position, segment) in kept.iter().enumerate() {
        if position == insert_after {
            push_packet(&mut out);
        }
        out.extend_from_slice(segment);
    }
    // EXIF が無い、またはすべてのセグメントの後ろに置く場合。
    if insert_after >= kept.len() {
        push_packet(&mut out);
    }
    out.extend_from_slice(&original[index..]);

    if out.len() < 4 || out[out.len() - 2] != 0xFF || out[out.len() - 1] != 0xD9 {
        return Err("書き出した JPEG の終端が不正です".into());
    }
    Ok(out)
}
/// 書いたファイルが画像として開けるか確かめる。
/// **形式は拡張子ではなく中身から判定する。** 一時ファイルの拡張子（`.photocurator-tmp`）からは
/// 形式が分からず、確かめが毎回失敗して書き込みを取り消していた（新版の 6e56524 と同じ不具合）。
pub(crate) fn verify_image_file(path: &Path) -> Result<(), String> {
    image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
// ---- 組の RAW の .xmp（U47）----------------------------------------------

/// XML のタグ 1 つの位置（`text` 内のバイト位置。`end` は `>` の次）。
pub(crate) struct XmlTag {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) name: String,
    pub(crate) is_end: bool,
    pub(crate) self_closing: bool,
}

/// XML のタグを前から拾う。コメント・処理命令・CDATA・DOCTYPE は読み飛ばす。
/// タグの入れ子が合っていなければ（`.xmp` が壊れている）Err。**XML として読めるかの検証を兼ねる。**
pub(crate) fn scan_xml_tags(text: &str) -> Result<Vec<XmlTag>, String> {
    let bytes = text.as_bytes();
    let mut tags = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0usize;
    let skip_to = |from: usize, needle: &str| -> Result<usize, String> {
        text[from..]
            .find(needle)
            .map(|offset| from + offset + needle.len())
            .ok_or_else(|| "XML が途中で終わっています".to_string())
    };
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let rest = &text[index..];
        if rest.starts_with("<!--") {
            index = skip_to(index + 4, "-->")?;
        } else if rest.starts_with("<?") {
            index = skip_to(index + 2, "?>")?;
        } else if rest.starts_with("<![CDATA[") {
            index = skip_to(index + 9, "]]>")?;
        } else if rest.starts_with("<!") {
            index = skip_to(index + 2, ">")?;
        } else {
            // 開始タグ・終了タグ。属性の値の中の `>` は無視する。
            let mut cursor = index + 1;
            let mut quote: Option<u8> = None;
            let mut close = None;
            while cursor < bytes.len() {
                let byte = bytes[cursor];
                match quote {
                    Some(q) => {
                        if byte == q {
                            quote = None;
                        }
                    }
                    None => {
                        if byte == b'"' || byte == b'\'' {
                            quote = Some(byte);
                        } else if byte == b'>' {
                            close = Some(cursor);
                            break;
                        }
                    }
                }
                cursor += 1;
            }
            let close = close.ok_or_else(|| "タグが閉じていません".to_string())?;
            let inner = &text[index + 1..close];
            let is_end = inner.starts_with('/');
            let self_closing = !is_end && inner.ends_with('/');
            let name_source = inner.trim_start_matches('/');
            let name: String = name_source
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
                .collect();
            if name.is_empty() {
                return Err("タグの名前がありません".into());
            }
            if is_end {
                match stack.pop() {
                    Some(open) if open == name => {}
                    _ => return Err(format!("タグの対応が合っていません: {name}")),
                }
            } else if !self_closing {
                stack.push(name.clone());
            }
            tags.push(XmlTag { start: index, end: close + 1, name, is_end, self_closing });
            index = close + 1;
        }
    }
    if !stack.is_empty() {
        return Err("閉じていないタグがあります".into());
    }
    if tags.is_empty() {
        return Err("XML のタグがありません".into());
    }
    Ok(tags)
}

/// 開始タグの属性（名前・値の範囲。範囲は `text` 内のバイト位置で、引用符の内側）。
pub(crate) fn tag_attributes(text: &str, tag: &XmlTag) -> Vec<(String, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let limit = tag.end - 1;
    let mut cursor = tag.start + 1;
    // タグの名前を飛ばす。
    while cursor < limit && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    loop {
        while cursor < limit && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/') {
            cursor += 1;
        }
        if cursor >= limit {
            break;
        }
        let name_start = cursor;
        while cursor < limit && bytes[cursor] != b'=' && !bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let name = text[name_start..cursor].to_string();
        while cursor < limit && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= limit || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < limit && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= limit || (bytes[cursor] != b'"' && bytes[cursor] != b'\'') {
            break;
        }
        let quote = bytes[cursor];
        let value_start = cursor + 1;
        let Some(length) = bytes[value_start..limit].iter().position(|b| *b == quote) else {
            break;
        };
        out.push((name, value_start..value_start + length));
        cursor = value_start + length + 1;
    }
    out
}

/// `.xmp` の中の `xmp:Rating`（属性・要素）の値を、見つけた順に返す。読めなければ Err。
pub(crate) fn xmp_ratings_of_document(text: &str) -> Result<Vec<String>, String> {
    let tags = scan_xml_tags(text)?;
    let mut found = Vec::new();
    for (position, tag) in tags.iter().enumerate() {
        if tag.is_end {
            continue;
        }
        for (name, range) in tag_attributes(text, tag) {
            if name == "xmp:Rating" {
                found.push(text[range].trim().to_string());
            }
        }
        if tag.name == "xmp:Rating" && !tag.self_closing {
            if let Some(next) = tags.get(position + 1) {
                if next.is_end && next.name == "xmp:Rating" {
                    found.push(text[tag.end..next.start].trim().to_string());
                }
            }
        }
    }
    Ok(found)
}

/// 既にある `.xmp` の星（`xmp:Rating`）だけを書き換える。ほかの内容は 1 バイトも変えない。
/// `xmp:Rating` が無ければ、最初の `rdf:Description` に属性として足す。
/// 読めない・`rdf:Description` が無いときは Err（呼び出し側は何も書かない）。
pub(crate) fn xmp_with_rating(existing: &str, rating: i64) -> Result<String, String> {
    let tags = scan_xml_tags(existing)?;
    let value = rating.to_string();
    // (差し替える範囲, 入れる文字列)
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (position, tag) in tags.iter().enumerate() {
        if tag.is_end {
            continue;
        }
        for (name, range) in tag_attributes(existing, tag) {
            if name == "xmp:Rating" {
                edits.push((range, value.clone()));
            }
        }
        if tag.name == "xmp:Rating" && !tag.self_closing {
            if let Some(next) = tags.get(position + 1) {
                if next.is_end && next.name == "xmp:Rating" {
                    edits.push((tag.end..next.start, value.clone()));
                }
            }
        }
    }
    if edits.is_empty() {
        let description = tags
            .iter()
            .find(|tag| !tag.is_end && tag.name == "rdf:Description")
            .ok_or_else(|| "rdf:Description が見つかりません".to_string())?;
        let has_namespace = tag_attributes(existing, description)
            .iter()
            .any(|(name, _)| name == "xmlns:xmp");
        let insert_at = description.end - if description.self_closing { 2 } else { 1 };
        let mut addition = String::new();
        if !has_namespace {
            addition.push_str(" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"");
        }
        addition.push_str(&format!(" xmp:Rating=\"{value}\""));
        edits.push((insert_at..insert_at, addition));
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = existing.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    Ok(out)
}

/// 新しく作る `.xmp` の中身（最小の XMP パケット）。
pub(crate) fn new_sidecar_xmp(rating: i64) -> String {
    xmp_body(rating)
}

/// JPEG（`.jpg`・`.jpeg`）の組の RAW を、同じフォルダの一覧から選ぶ純関数。
/// 拡張子を除いた名前が大文字小文字を無視して一致し、拡張子が RAW の一覧にあるもの。
/// フォルダが違うものは組ではない。並びはパス順。
pub(crate) fn paired_raw_files(jpeg: &Path, siblings: &[PathBuf]) -> Vec<PathBuf> {
    let Some(stem) = jpeg.file_stem().map(|v| v.to_string_lossy().to_lowercase()) else {
        return Vec::new();
    };
    let folder = jpeg.parent();
    let mut found: Vec<PathBuf> = siblings
        .iter()
        .filter(|candidate| {
            candidate.parent() == folder
                && candidate
                    .file_stem()
                    .is_some_and(|v| v.to_string_lossy().to_lowercase() == stem)
                && extension_lower(candidate).is_some_and(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
        })
        .cloned()
        .collect();
    found.sort();
    found
}

/// JPEG と同じフォルダのファイルを列挙して、組の RAW を返す（読めなければ空）。
pub(crate) fn find_paired_raws(jpeg: &Path) -> Vec<PathBuf> {
    let Some(folder) = jpeg.parent() else {
        return Vec::new();
    };
    let siblings: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => return Vec::new(),
    };
    paired_raw_files(jpeg, &siblings)
}

/// RAW の隣の `.xmp` に星を書く。**RAW 本体は読みも書きもしない。**
/// 既にあれば（名前の大文字小文字は問わない）`xmp:Rating` だけを更新し、無ければ新しく作る。
/// 同じフォルダの一時ファイルへ書き、XML として読めて星が期待どおりか確かめてから置き換える。
/// 読み取り専用の `.xmp`・UTF-8 でない `.xmp`・構造が読めない `.xmp` には触らず Err。
pub(crate) fn write_sidecar_xmp_for_raw(raw: &Path, rating: i64) -> Result<PathBuf, String> {
    let folder = raw.parent().ok_or("RAW のフォルダが分かりません")?;
    let stem = raw.file_stem().ok_or("RAW の名前が分かりません")?;
    let wanted = format!("{}.xmp", stem.to_string_lossy().to_lowercase());
    let existing: Option<PathBuf> = fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase() == wanted)
        });

    let (target, content) = match &existing {
        Some(path) => {
            let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
            if metadata.permissions().readonly() {
                return Err("既存の .xmp が読み取り専用のため書きませんでした".into());
            }
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let text = String::from_utf8(bytes)
                .map_err(|_| "既存の .xmp が UTF-8 ではないため書きませんでした".to_string())?;
            let updated = xmp_with_rating(&text, rating)
                .map_err(|reason| format!("既存の .xmp を読めないため書きませんでした: {reason}"))?;
            (path.clone(), updated)
        }
        None => {
            let mut name = stem.to_os_string();
            name.push(".xmp");
            (folder.join(name), new_sidecar_xmp(rating))
        }
    };

    let mut temporary_name = target.file_name().unwrap_or_default().to_os_string();
    temporary_name.push(".photocurator-tmp");
    let temporary = folder.join(temporary_name);
    if let Err(error) = fs::write(&temporary, content.as_bytes()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    let expected = rating.to_string();
    let verified = fs::read_to_string(&temporary)
        .map_err(|error| error.to_string())
        .and_then(|text| xmp_ratings_of_document(&text))
        .and_then(|ratings| {
            if !ratings.is_empty() && ratings.iter().all(|value| *value == expected) {
                Ok(())
            } else {
                Err("星が期待どおりに書けていません".to_string())
            }
        });
    if let Err(reason) = verified {
        let _ = fs::remove_file(&temporary);
        return Err(format!("検証に失敗したため .xmp は変更していません: {reason}"));
    }
    match fs::rename(&temporary, &target) {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error.to_string())
        }
    }
}
