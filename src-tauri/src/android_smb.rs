//! NAS（SMB）への口。**Android でだけコンパイルされる。**
//!
//! `android_photos` と同じ形で、Kotlin の `SmbAccess` を JNI で呼ぶ。
//! 境界は「繋ぐ」「一覧を返す」「バイト列を返す」だけ。
//!
//! **`read_bytes` に offset と length があることが要点。**
//! EXIF 埋め込みサムネイル経路は先頭 26KB 程度で済むので、
//! 1 枚 6.7MB を毎回 Wi-Fi で運ばずに済む（2,000 枚で 13.4GB → 200MB）。

use crate::android_photos::{jni_bytes, jni_string, MediaPhoto};
use serde::Deserialize;

const CLASS: &str = "app/photocurator/desktop/SmbAccess";

/// 共有フォルダの中の 1 フォルダ。選ぶ画面に並べる。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmbFolder {
    pub name: String,
    /// `smb://host/share/...`。そのまま `create_project` に渡せる。
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stat {
    size: i64,
    modified_at: i64,
}

/// 繋ぐ。空文字が返れば成功、そうでなければ理由。
pub fn connect(host: &str, share: &str, user: &str, password: &str) -> Result<(), String> {
    let message = jni_string(
        CLASS,
        "connect",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[host, share, user, password],
    )?;
    if message.is_empty() {
        Ok(())
    } else {
        Err(message)
    }
}

pub fn disconnect() {
    let _ = jni_string(CLASS, "disconnect", "()V", &[]);
}

pub fn list_folders(path: &str) -> Result<Vec<SmbFolder>, String> {
    let json = jni_string(
        CLASS,
        "listFolders",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[path],
    )?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("フォルダの一覧を読めません: {error}"))
}

pub fn list_photos(path: &str) -> Result<Vec<MediaPhoto>, String> {
    let json = jni_string(
        CLASS,
        "listPhotos",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[path],
    )?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("写真の一覧を読めません: {error}"))
}

pub fn stat(url: &str) -> Option<(i64, i64)> {
    let json = jni_string(CLASS, "stat", "(Ljava/lang/String;)Ljava/lang/String;", &[url]).ok()?;
    let parsed: Stat = serde_json::from_str(&json).ok()?;
    Some((parsed.modified_at, parsed.size))
}

pub fn read_bytes(url: &str, offset: i64, length: i32) -> Option<Vec<u8>> {
    jni_bytes(CLASS, "readBytes", url, offset, length)
}
