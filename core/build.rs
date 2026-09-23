fn main() {
    // uniffi 無し（`--no-default-features`）のときは足場を作らない。
    // cargo はこのクレート自身が有効にした feature を CARGO_FEATURE_* で渡す。
    if std::env::var_os("CARGO_FEATURE_UNIFFI").is_some() {
        uniffi::generate_scaffolding("src/photo_curator_core.udl").expect("uniffi の足場を作れません");
    }
}
