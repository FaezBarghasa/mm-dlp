fn main() {
    uniffi::generate_scaffolding("./src/mm-dlp.udl").expect("Build script failed");
}