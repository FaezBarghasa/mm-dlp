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
    let out_path = camino::Utf8PathBuf::from(out_dir.to_string_lossy().into_owned());

    let options = uniffi_bindgen::bindings::GenerateOptions {
        languages: vec![
            uniffi_bindgen::bindings::TargetLanguage::Swift,
            uniffi_bindgen::bindings::TargetLanguage::Kotlin,
        ],
        source: source_path,
        out_dir: out_path,
        config_override: None,
        format: false,
        crate_filter: None,
        metadata_no_deps: true,
    };

    uniffi_bindgen::bindings::generate(options)
        .expect("Failed to invoke uniffi_bindgen for Swift and Kotlin exports");

    let kt_file = out_dir.join("mmdlp.kt");
    let fallback_kt_file = out_dir.join("uniffi").join("mmdlp").join("mmdlp.kt");
    let mut entries = Vec::new();
    fn collect_files(dir: &std::path::Path, entries: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_dir() {
                        collect_files(&path, entries);
                    } else {
                        entries.push(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    collect_files(&out_dir, &mut entries);
    assert!(
        kt_file.exists() || fallback_kt_file.exists(),
        "Kotlin JNI bindings were not generated. Found files: {:?}", entries
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