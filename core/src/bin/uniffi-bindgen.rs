//! Kotlin / Swift / Python のバインディングを生成する道具。
//!
//! UniFFI 0.28 は「同じクレートに置いた小さな bin」から呼ぶ形を推奨している。
//! 別途 uniffi-bindgen を入れると**クレートと版がずれて**、生成物と
//! 実行時ライブラリが噛み合わなくなる。
fn main() {
    uniffi::uniffi_bindgen_main()
}
