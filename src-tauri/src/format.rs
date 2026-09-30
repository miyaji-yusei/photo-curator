//! 形式は中身の先頭バイトで判定する（拡張子で決めない。設計 05 章 2026-09-23）。
//! Amazon の `.cr2` が中身 JPEG だった教訓（07 章）と同じ理由。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Jpeg,
    Png,
    WebP,
    Video,
    Other,
}

/// `head` は先頭 16 バイト以上あれば足りる。
pub fn sniff(head: &[u8]) -> FileKind {
    if head.len() >= 3 && head[0] == 0xFF && head[1] == 0xD8 && head[2] == 0xFF {
        return FileKind::Jpeg;
    }
    if head.len() >= 8 && head[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return FileKind::Png;
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" {
        if &head[8..12] == b"WEBP" {
            return FileKind::WebP;
        }
        if &head[8..12] == b"AVI " {
            return FileKind::Video;
        }
    }
    // MP4 / MOV / M4V / 3GP などの ISO base media 系は 4〜8 バイト目が "ftyp"。
    if head.len() >= 8 && &head[4..8] == b"ftyp" {
        return FileKind::Video;
    }
    // WebM / Matroska
    if head.len() >= 4 && head[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return FileKind::Video;
    }
    FileKind::Other
}

pub fn is_image(kind: FileKind) -> bool {
    matches!(kind, FileKind::Jpeg | FileKind::Png | FileKind::WebP)
}
