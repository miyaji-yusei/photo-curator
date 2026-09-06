fn main() {
    uniffi::generate_scaffolding("src/photo_curator_core.udl").expect("uniffi の足場を作れません");
}
