//! 端末の写真（MediaStore）への口。**Android でだけコンパイルされる。**
//!
//! Kotlin の `PhotoAccess` を JNI で呼ぶ。境界は 3 本だけ:
//!
//! | Kotlin | 何を返すか |
//! |---|---|
//! | `listAlbums()` | アルバム（bucket）の一覧。JSON |
//! | `listPhotos(bucketId)` | その中の写真。撮影順。JSON |
//! | `readBytes(uri, offset, length)` | 写真のバイト列。**先頭だけ読める** |
//!
//! 先頭だけ読めることが要点で、EXIF 埋め込みサムネイル経路は 26KB 程度で
//! 済む（実測）。ここを常に全体にすると 1 枚 6.7MB を毎回運ぶことになる。

use jni::objects::{JByteArray, JObject, JString, JValue};
use jni::JavaVM;
use serde::{Deserialize, Serialize};

const CLASS: &str = "app/photocurator/desktop/PhotoAccess";

/// JavaVM を取る。**Android のランタイムが `ndk-context` に登録している**ので、
/// Tauri の AppHandle を経由する必要は無い。
fn java_vm() -> Result<JavaVM, String> {
    let context = ndk_context::android_context();
    if context.vm().is_null() {
        return Err("Android の実行環境に接続していません。".into());
    }
    // Safety: ndk-context が登録した、プロセスに 1 つの JavaVM のポインタ。
    unsafe { JavaVM::from_raw(context.vm().cast()) }
        .map_err(|error| format!("JavaVM を取得できません: {error}"))
}

/// JNI を触るときの共通の型。**失敗は全部これに畳む。**
/// 1 枚読めなくても解析全体は続けるので、詳細より「駄目だった」ことが大事。
pub type Bridge<T> = Result<T, String>;

fn with_env<T>(body: impl FnOnce(&mut jni::AttachGuard<'_>) -> Bridge<T>) -> Bridge<T> {
    let vm = java_vm()?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("JNI に接続できません: {error}"))?;
    let result = body(&mut env);
    // 例外が残っていると次の JNI 呼び出しが必ず失敗する。ここで必ず払う。
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

/// 文字列引数だけを取る静的メソッドを呼び、文字列を受け取る。
/// **戻り値が void でも呼べる**（その場合は空文字が返る）。
/// android_smb からも使うので、ここが JNI の共通の入口になる。
pub fn jni_string(class: &str, method: &str, signature: &str, args: &[&str]) -> Bridge<String> {
    with_env(|env| {
        // JString は env から作るので、参照を保ったまま JValue に詰める。
        let mut objects = Vec::with_capacity(args.len());
        for argument in args {
            objects.push(
                env.new_string(argument)
                    .map_err(|error| format!("引数を渡せません: {error}"))?,
            );
        }
        let values: Vec<JValue<'_, '_>> =
            objects.iter().map(|o| JValue::Object(o)).collect();
        let value = env
            .call_static_method(class, method, signature, &values)
            .map_err(|error| format!("{method} を呼べません: {error}"))?;
        // void のときは l() が失敗する。呼べたこと自体は成功なので空文字を返す。
        let Ok(object) = value.l() else {
            return Ok(String::new());
        };
        if object.is_null() {
            return Ok(String::new());
        }
        let text: String = env
            .get_string(&JString::from(object))
            .map_err(|error| format!("{method} の文字列を読めません: {error}"))?
            .into();
        Ok(text)
    })
}

/// (String, long, int) から byte[] を受け取る形の静的メソッドを呼ぶ。
/// **部分読みの入口。** 全体を読むときは length に 0 を渡す。
pub fn jni_bytes(
    class: &str,
    method: &str,
    url: &str,
    offset: i64,
    length: i32,
) -> Option<Vec<u8>> {
    with_env(|env| {
        let argument = env.new_string(url).map_err(|e| e.to_string())?;
        let value = env
            .call_static_method(
                class,
                method,
                "(Ljava/lang/String;JI)[B",
                &[
                    JValue::Object(&argument),
                    JValue::Long(offset),
                    JValue::Int(length),
                ],
            )
            .map_err(|e| e.to_string())?;
        let object: JObject<'_> = value.l().map_err(|e| e.to_string())?;
        if object.is_null() {
            return Err("読めませんでした。".into());
        }
        let array = JByteArray::from(object);
        env.convert_byte_array(&array).map_err(|e| e.to_string())
    })
    .ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPhoto {
    pub uri: String,
    pub name: String,
    pub relative_path: String,
    pub size: i64,
    pub modified_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stat {
    size: i64,
    modified_at: i64,
}

pub fn list_albums() -> Bridge<Vec<Album>> {
    let json = jni_string(CLASS, "listAlbums", "()Ljava/lang/String;", &[])?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("アルバムの一覧を読めません: {error}"))
}

pub fn list_photos(bucket_id: &str) -> Bridge<Vec<MediaPhoto>> {
    let json = jni_string(
        CLASS,
        "listPhotos",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[bucket_id],
    )?;
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|error| format!("写真の一覧を読めません: {error}"))
}

/// 1 枚ぶんの mtime と size。**既存のキャッシュ無効化がそのまま効く。**
pub fn stat(uri: &str) -> Option<(i64, i64)> {
    let json = jni_string(
        CLASS,
        "statPhoto",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[uri],
    )
    .ok()?;
    let parsed: Stat = serde_json::from_str(&json).ok()?;
    Some((parsed.modified_at, parsed.size))
}

/// 写真のバイト列。`length` が 0 なら最後まで。
pub fn read_bytes(uri: &str, offset: i64, length: i32) -> Option<Vec<u8>> {
    jni_bytes(CLASS, "readBytes", uri, offset, length)
}
