//! 星を JPEG の XMP に書き込む。**原本を書き換える**（設計 07 章 段4-4）。
//! 旧 `src-tauri/src/lib.rs`（段3 以前）の同名関数をそのまま移した。

/// JPEG の XMP パケットを組み立てる。`xmp:Rating` は 0〜5 をそのまま持てる。
pub fn xmp_packet(rating: i64) -> Vec<u8> {
    let body = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    );
    let mut packet = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    packet.extend_from_slice(body.as_bytes());
    packet
}

/// JPEG のセグメント列を組み直し、XMP の APP1 を差し替える（無ければ挿入）。
/// 画像本体（SOS 以降）には一切触れない。既存の EXIF セグメントもそのまま残す。
pub fn jpeg_with_rating(original: &[u8], rating: i64) -> Result<Vec<u8>, String> {
    if original.len() < 4 || original[0] != 0xFF || original[1] != 0xD8 {
        return Err("JPEG ではありません".into());
    }
    let packet = xmp_packet(rating);
    if packet.len() + 2 > 0xFFFF {
        return Err("XMP が大きすぎます".into());
    }

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut insert_after = 0usize;
    let mut index = 2usize;
    while index + 4 <= original.len() {
        if original[index] != 0xFF {
            break;
        }
        let marker = original[index + 1];
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
            kept.push(&original[index..index + 2 + length]);
            if marker == 0xE1 {
                insert_after = kept.len();
            }
        }
        index += 2 + length;
    }

    let mut out = Vec::with_capacity(original.len() + packet.len() + 4);
    out.extend_from_slice(&original[0..2]);
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
    if insert_after >= kept.len() {
        push_packet(&mut out);
    }
    out.extend_from_slice(&original[index..]);

    if out.len() < 4 || out[out.len() - 2] != 0xFF || out[out.len() - 1] != 0xD9 {
        return Err("書き出した JPEG の終端が不正です".into());
    }
    Ok(out)
}
