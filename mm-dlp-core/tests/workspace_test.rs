use mm_dlp_core::config::EngineConfig;
use mm_dlp_core::engine::MmDlpEngine;

#[test]
fn test_workspace_compiles_and_initializes_scaffolding() {
    let config = EngineConfig::default();
    assert_eq!(config.max_concurrent_downloads, 4);
    assert_eq!(config.timeout_seconds, 30);

    let engine = MmDlpEngine::new(config).unwrap();
    assert_eq!(engine.config().max_concurrent_downloads, 4);
}
