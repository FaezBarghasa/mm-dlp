fn main() {
    // Instructs Cargo to rerun the build script if the UniFFI interface changes
    println!("cargo:rerun-if-changed=src/uniffi/mm-dlp.udl");
    
    uniffi::generate_scaffolding("src/uniffi/mm-dlp.udl")
        .expect("Failed to generate UniFFI scaffolding from mm-dlp.udl");
}