fn main() {
    uniffi::generate_scaffolding("src/core.udl").expect("uniffi の足場を作れません");
}
