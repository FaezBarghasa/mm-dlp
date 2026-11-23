use clap::Parser;
use mm_dlp_cli::Cli;

#[test]
fn test_cli_phase1_simulation_flags() {
    let args = vec![
        "mm-dlp",
        "-g",
        "-e",
        "--get-id",
        "--get-thumbnail",
        "--get-duration",
        "--get-filename",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse simulation flags");
    assert!(cli.get_url);
    assert!(cli.get_title);
    assert!(cli.get_id);
    assert!(cli.get_thumbnail);
    assert!(cli.get_duration);
    assert!(cli.get_filename);
    assert_eq!(
        cli.urls,
        vec!["https://www.youtube.com/watch?v=dQw4w9WgXcQ"]
    );
}

#[test]
fn test_cli_phase1_compatibility_flags() {
    let args = vec![
        "mm-dlp",
        "--newline",
        "--skip-download",
        "--no-check-certificates",
        "--no-mtime",
        "--prefer-free-formats",
        "--abort-on-error",
        "--add-header",
        "Authorization: Bearer test_token",
        "--add-header",
        "X-Custom-Client: mm-dlp-test",
        "https://example.com/video.mp4",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse compatibility flags");
    assert!(cli.newline);
    assert!(cli.skip_download);
    assert!(cli.no_check_certificates);
    assert!(cli.no_mtime);
    assert!(cli.prefer_free_formats);
    assert!(cli.abort_on_error);
    assert_eq!(
        cli.add_header,
        vec![
            "Authorization: Bearer test_token".to_string(),
            "X-Custom-Client: mm-dlp-test".to_string(),
        ]
    );
    assert_eq!(cli.urls, vec!["https://example.com/video.mp4"]);
}

#[test]
fn test_cli_phase1_aliases() {
    let args = vec![
        "mm-dlp",
        "--no-download",
        "--no-check-certificate",
        "https://example.com/video.mp4",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse flag aliases");
    assert!(cli.skip_download);
    assert!(cli.no_check_certificates);
}

#[test]
fn test_cli_phase2_fragment_resilience_flags() {
    let args = vec![
        "mm-dlp",
        "--fragment-retries",
        "15",
        "--skip-unavailable-fragments",
        "https://example.com/stream.m3u8",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse fragment resilience flags");
    assert_eq!(cli.fragment_retries, "15");
    assert!(cli.skip_unavailable_fragments);
    assert!(!cli.no_skip_unavailable_fragments);
}

#[test]
fn test_cli_phase2_fragment_resilience_aliases() {
    let args = vec![
        "mm-dlp",
        "--fragment-retries",
        "infinite",
        "--abort-on-unavailable-fragments",
        "https://example.com/stream.m3u8",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse fragment resilience aliases");
    assert_eq!(cli.fragment_retries, "infinite");
    assert!(cli.no_skip_unavailable_fragments);
}

#[test]
fn test_cli_phase4_drop_in_parity_flags() {
    let args = vec![
        "mm-dlp",
        "--write-info-json",
        "--socket-timeout",
        "45",
        "https://example.com/video.mp4",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse drop-in parity flags");
    assert!(cli.write_info_json);
    assert_eq!(cli.socket_timeout, Some(45));
}

#[test]
fn test_cli_phase5_ultimate_parity_flags() {
    let args = vec![
        "mm-dlp",
        "--limit-rate",
        "1.5M",
        "--no-part",
        "--playlist-items",
        "1,2,5-8",
        "--playlist-start",
        "3",
        "--playlist-end",
        "9",
        "https://example.com/playlist",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse ultimate parity flags");
    assert_eq!(cli.limit_rate.as_deref(), Some("1.5M"));
    assert!(cli.no_part);
    assert_eq!(cli.playlist_items.as_deref(), Some("1,2,5-8"));
    assert_eq!(cli.playlist_start, Some(3));
    assert_eq!(cli.playlist_end, Some(9));
}
