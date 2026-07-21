fn main() {
    let udl_path = "./src/mm-dlp.udl";
    if std::path::Path::new(udl_path).exists() {
        uniffi::generate_scaffolding(udl_path).expect("Build script failed to generate UDL scaffolding");
    }
}

