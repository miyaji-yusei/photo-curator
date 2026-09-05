//! SAF（フォルダ選択）から写真を読む口。**Android でだけコンパイルされる。**
//!
//! NAS へは smbj で直接繋ぐのが第一の道（`android_smb`）だが、実機で繋がらない
//! ことがありうる。そのときの代替がこれ。
//!
//! **NAS のベンダー製アプリが DocumentsProvider として登録されていれば、
//! SAF のフォルダ選択にその NAS が現れる。** そこを選べば、SMB を自分で
//! 話さずに NAS の写真へ届く。端末内のフォルダも同じ仕組みで扱える。
//!
//! 読み出しは `content://` なので、`PhotoRef::Content` がそのまま使える。
//! 増えるのは「フォルダを選ぶ」「フォルダの中を数え上げる」の 2 つだけ。

use crate::android_photos::{jni_string, Bridge, MediaPhoto};
use serde::Deserialize;

const CLASS: &str = "app/photocurator/desktop/TreeAccess";
const ACTIVITY: &str = "app/photocurator/desktop/MainActivity";

/// 権限を持っているフォルダ。**再起動しても残る**（永続化済みの URI 権限）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub path: String,
    pub name: String,
}

/// フォルダ選択を開く。**結果はここでは返らない。**
/// Activity の結果を待つ仕掛けを Rust 側に作らずに済ませるため、
/// 選ばれたものは `take_picked` で後から取りに行く。
pub fn open_picker() -> Bridge<()> {
    jni_string(ACTIVITY, "openTreePicker", "()V", &[]).map(|_| ())
}

/// 直近に選ばれたフォルダ。まだ選ばれていなければ空文字。
pub fn take_picked() -> Bridge<String> {
    jni_string(CLASS, "takePickedTree", "()Ljava/lang/String;", &[])
}

pub fn list_granted() -> Bridge<Vec<Tree>> {
    let json = jni_string(CLASS, "listGrantedTrees", "()Ljava/lang/String;", &[])?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("フォルダの一覧を読めません: {error}"))
}

pub fn list_photos(tree_uri: &str) -> Bridge<Vec<MediaPhoto>> {
    let json = jni_string(
        CLASS,
        "listPhotos",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[tree_uri],
    )?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("写真の一覧を読めません: {error}"))
}
