//! NAS のパスワードを端末に預ける口。**Android でだけコンパイルされる。**
//!
//! 中身は Kotlin の `SecretStore` に任せる。鍵は Android Keystore が持ち、
//! アプリの外へは出ない。ここは JNI の境界だけを引く。
//!
//! **利用者が「保存する」を選んだときだけ使う。** 既定では預からない。

use crate::android_photos::{jni_string, Bridge};

const CLASS: &str = "app/photocurator/desktop/SecretStore";

/// 預けてあるパスワード。無ければ空文字。
pub fn load_password() -> Option<String> {
    jni_string(CLASS, "loadPassword", "()Ljava/lang/String;", &[]).ok()
}

/// 預ける。**空文字を渡すと消す。**
pub fn save_password(password: &str) -> Result<(), String> {
    let failure: Bridge<String> = jni_string(
        CLASS,
        "savePassword",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[password],
    );
    match failure? {
        // Kotlin 側は「駄目だった理由」を返す。空なら成功。
        reason if reason.is_empty() => Ok(()),
        reason => Err(reason),
    }
}
