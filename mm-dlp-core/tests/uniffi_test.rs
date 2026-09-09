use std::fs;
use std::path::PathBuf;

#[test]
fn test_uniffi_scaffolding_and_bindings() {
    let udl_path = PathBuf::from("src/mm-dlp.udl");
    assert!(udl_path.exists(), "Target UDL file is missing from src/uniffi/");

    let out_dir = std::env::temp_dir().join("mm_dlp_uniffi_test_out");
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir_all(&out_dir).unwrap();
    
    let source_path = camino::Utf8PathBuf::from(udl_path.to_string_lossy().into_owned());
    let out_path = camino::Utf8PathBuf::from("../bindings");

    // Ensure directory exists
    std::fs::create_dir_all("../bindings/kotlin").unwrap();
    std::fs::create_dir_all("../bindings/swift").unwrap();

    let options = uniffi_bindgen::bindings::GenerateOptions {
        languages: vec![
            uniffi_bindgen::bindings::TargetLanguage::Swift,
            uniffi_bindgen::bindings::TargetLanguage::Kotlin,
        ],
        source: source_path,
        out_dir: out_path.clone(),
        config_override: None,
        format: false,
        crate_filter: None,
        metadata_no_deps: true,
    };

    uniffi_bindgen::bindings::generate(options)
        .expect("Failed to invoke uniffi_bindgen for Swift and Kotlin exports");

    // Copy generated files to correct folders if needed, or let them stay in output folder
    // Let's verify files are generated in the bindings directory
    let kt_file = std::path::Path::new("../bindings/mmdlp.kt");
    let swift_file = std::path::Path::new("../bindings/mmdlp.swift");
    assert!(kt_file.exists() || std::path::Path::new("../bindings/uniffi/mmdlp/mmdlp.kt").exists(), "Kotlin JNI bindings were not generated");
    assert!(swift_file.exists(), "Swift bindings were not generated");
}