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

use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::jint;
use jni::JavaVM;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;

const CLASS: &str = "app/photocurator/desktop/PhotoAccess";

/// プロセスに 1 つだけある JavaVM。
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

/// JVM が `System.loadLibrary` の中で呼ぶ。**JavaVM の唯一の入手経路。**
///
/// 以前は `ndk_context::android_context()` から取れると考えていたが、
/// **あれを埋めるのは NativeActivity 系（`ndk-glue`）だけ**で、Tauri のように
/// Java の Activity を使う構成では最後まで空のままになる。実機では
/// 「android context was not initialized」で panic し、`Rust_ipc` は
/// `extern "C"` なので巻き戻せず abort していた。
///
/// ここは `.so` が読み込まれた時点で呼ばれるので、**Kotlin も Rust も
/// まだ何も動いていない。** 呼ぶ順番を気にせずに済む。
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: *mut jni::sys::JavaVM, _reserved: *mut c_void) -> jint {
    // Safety: JVM が渡してくる、プロセスに 1 つの JavaVM のポインタ。
    if let Ok(vm) = unsafe { JavaVM::from_raw(vm) } {
        // クラスは**ここでしか正しく引けない**（CLASSES の説明を参照）。
        if let Ok(mut env) = vm.get_env() {
            let mut resolved = HashMap::new();
            for name in APP_CLASSES {
                let Ok(class) = env.find_class(name) else { continue };
                // ローカル参照はこのフレームを出ると無効になる。持ち続けるため昇格する。
                if let Ok(global) = env.new_global_ref(class) {
                    resolved.insert(*name, global);
                }
            }
            let _ = CLASSES.set(resolved);
        }
        // 2 度呼ばれることは無いが、失敗しても最初の 1 つが残ればよい。
        let _ = JAVA_VM.set(vm);
    }
    jni::sys::JNI_VERSION_1_6
}

/// このアプリが JNI で呼ぶクラス。**`JNI_OnLoad` で全部引いておく。**
const APP_CLASSES: &[&str] = &[
    "app/photocurator/desktop/PhotoAccess",
    "app/photocurator/desktop/SmbAccess",
    "app/photocurator/desktop/SecretStore",
];

/// `JNI_OnLoad` で解決したクラス。
///
/// **`FindClass` は呼んだスレッド任せ**で、一番上にある Java フレームの
/// クラスローダを使う。走査や解析は `spawn_blocking` の上、つまり Java の
/// フレームが 1 つも無い Rust のスレッドで動くので、そこではシステムの
/// クラスローダに落ち、**アプリのクラスが見つからない。**
///
/// 実機ではこれが「listPhotos を呼べません: Java exception was thrown」
/// として出た。アルバム一覧（`Rust_ipc` から同期で呼ばれる＝Java フレームが
/// ある）は通るのに走査だけ落ちる、という分かりにくい形になる。
///
/// `JNI_OnLoad` は `System.loadLibrary` を呼んだアプリのクラスから呼ばれる
/// ので、そこでだけ正しく引ける。
static CLASSES: OnceLock<HashMap<&'static str, GlobalRef>> = OnceLock::new();

fn class_ref(class: &str) -> Bridge<&'static GlobalRef> {
    CLASSES
        .get()
        .and_then(|classes| classes.get(class))
        .ok_or_else(|| format!("{class} を読み込めていません。"))
}

fn java_vm() -> Result<&'static JavaVM, String> {
    JAVA_VM
        .get()
        .ok_or_else(|| "Android の実行環境に接続していません。".to_string())
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
    // **払う前に中身を取り出す。** 握り潰すと「Java exception was thrown」
    // としか分からず、実機で原因に辿り着けない。
    if env.exception_check().unwrap_or(false) {
        let detail = take_exception(&mut env);
        return Err(match result {
            Err(message) => format!("{message} / {detail}"),
            Ok(_) => detail,
        });
    }
    result
}

/// 残っている例外を払い、何だったかを文字列にする。
/// スタックトレースは logcat にも出す。
fn take_exception(env: &mut jni::AttachGuard<'_>) -> String {
    let Ok(throwable) = env.exception_occurred() else {
        return "Java の例外（内容を取れません）".into();
    };
    let _ = env.exception_describe();
    // 以降の JNI 呼び出しのために、先に払っておく必要がある。
    let _ = env.exception_clear();
    let unknown = || "Java の例外".to_string();
    let Ok(value) = env.call_method(&throwable, "toString", "()Ljava/lang/String;", &[]) else {
        return unknown();
    };
    let Ok(object) = value.l() else { return unknown() };
    // JString を束縛してから借りる。式の途中だと一時値になって借用できない。
    let text = JString::from(object);
    let described = match env.get_string(&text) {
        Ok(value) => value.into(),
        Err(_) => unknown(),
    };
    described
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
        let holder = env
            .new_local_ref(class_ref(class)?.as_obj())
            .map_err(|error| format!("{class} を参照できません: {error}"))?;
        let value = env
            .call_static_method(&JClass::from(holder), method, signature, &values)
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
        let holder = env
            .new_local_ref(class_ref(class)?.as_obj())
            .map_err(|error| format!("{class} を参照できません: {error}"))?;
        let value = env
            .call_static_method(
                &JClass::from(holder),
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
