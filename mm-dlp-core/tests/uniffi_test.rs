use std::fs;
use std::path::PathBuf;

#[test]
fn test_uniffi_scaffolding_and_bindings() {
    let udl_path = PathBuf::from("src/uniffi/mm-dlp.udl");
    assert!(udl_path.exists(), "Target UDL file is missing from src/uniffi/");

    let out_dir = std::env::temp_dir().join("mm_dlp_uniffi_test_out");
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir_all(&out_dir).unwrap();
    
    // Invoke uniffi_bindgen library programmatically to generate Kotlin and Swift files
    uniffi_bindgen::generate_bindings(
        &udl_path,
        None,
        vec!["swift", "kotlin"],
        Some(&out_dir),
        None,
        None,
        false,
    ).expect("Failed to invoke uniffi_bindgen for Swift and Kotlin exports");

    // Verify Kotlin JNI bindings are successfully generated
    let kt_file = out_dir.join("mmdlp.kt");
    let fallback_kt_file = out_dir.join("mmdlp").join("mmdlp.kt");
    assert!(
        kt_file.exists() || fallback_kt_file.exists(),
        "Kotlin JNI bindings were not generated"
    );

    // Verify Swift header exports align with defined types
    let swift_file = out_dir.join("mmdlp.swift");
    assert!(swift_file.exists(), "Swift bindings were not generated");

    let swift_content = fs::read_to_string(&swift_file).expect("Failed to read Swift bindings");
    assert!(swift_content.contains("DownloadProgressCallback"), "Swift file must declare the DownloadProgressCallback protocol");
    assert!(swift_content.contains("MediaInfo"), "Swift file must declare the MediaInfo struct");
    assert!(swift_content.contains("MmDlpEngine"), "Swift file must declare the MmDlpEngine class");

    // Clean up test artifacts
    fs::remove_dir_all(&out_dir).unwrap();
}