use mm_dlp_core::downloader::dash::DashDownloader;
use mm_dlp_core::downloader::hls::HlsDownloader;
use mm_dlp_core::extractor::fallback_bridge::FallbackBridgeExtractor;
use mm_dlp_core::extractor::format_selector::{FormatSelector, SelectedFormat};
use mm_dlp_core::extractor::generic::GenericExtractor;
use mm_dlp_core::extractor::traits::{AsyncExtractor, MediaFormat, MediaInfo};
use mm_dlp_core::utils::{expand_output_template, sanitize_url};
use serde_json::json;

fn create_mock_media_info() -> MediaInfo {
    MediaInfo {
        id: "dQw4w9WgXcQ".into(),
        title: "Never Gonna Give You Up".into(),
        description: Some("Official Music Video".into()),
        uploader: Some("Rick Astley".into()),
        duration: Some(213),
        thumbnail: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg".into()),
        webpage_url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".into()),
        formats: vec![
            MediaFormat {
                format_id: "140".into(),
                url: "https://example.com/140.m4a".into(),
                ext: "m4a".into(),
                width: None,
                height: None,
                fps: None,
                vcodec: Some("none".into()),
                acodec: Some("mp4a.40.2".into()),
                filesize: Some(3500000),
                tbr: Some(128.0),
                protocol: Some("https".into()),
                http_headers: None,
            },
            MediaFormat {
                format_id: "251".into(),
                url: "https://example.com/251.webm".into(),
                ext: "webm".into(),
                width: None,
                height: None,
                fps: None,
                vcodec: Some("none".into()),
                acodec: Some("opus".into()),
                filesize: Some(4000000),
                tbr: Some(160.0),
                protocol: Some("https".into()),
                http_headers: None,
            },
            MediaFormat {
                format_id: "18".into(),
                url: "https://example.com/18.mp4".into(),
                ext: "mp4".into(),
                width: Some(640),
                height: Some(360),
                fps: Some(30.0),
                vcodec: Some("avc1.42001E".into()),
                acodec: Some("mp4a.40.2".into()),
                filesize: Some(15000000),
                tbr: Some(500.0),
                protocol: Some("https".into()),
                http_headers: None,
            },
            MediaFormat {
                format_id: "137".into(),
                url: "https://example.com/137.mp4".into(),
                ext: "mp4".into(),
                width: Some(1920),
                height: Some(1080),
                fps: Some(30.0),
                vcodec: Some("avc1.640028".into()),
                acodec: Some("none".into()),
                filesize: Some(65000000),
                tbr: Some(4000.0),
                protocol: Some("https".into()),
                http_headers: None,
            },
            MediaFormat {
                format_id: "248".into(),
                url: "https://example.com/248.webm".into(),
                ext: "webm".into(),
                width: Some(1920),
                height: Some(1080),
                fps: Some(30.0),
                vcodec: Some("vp9".into()),
                acodec: Some("none".into()),
                filesize: Some(55000000),
                tbr: Some(3500.0),
                protocol: Some("https".into()),
                http_headers: None,
            },
        ],
        subtitles: None,
    }
}

#[test]
fn test_drop_in_format_selection_bestvideo_plus_bestaudio() {
    let info = create_mock_media_info();
    let selected = FormatSelector::select(&info, "bestvideo+bestaudio/best").unwrap();
    match selected {
        SelectedFormat::Merged { video, audio } => {
            assert_eq!(video.format_id, "137");
            assert_eq!(audio.format_id, "251");
        }
        _ => panic!("Expected Merged video and audio formats"),
    }
}

#[test]
fn test_drop_in_format_selection_shorthand_ba() {
    let info = create_mock_media_info();
    let selected = FormatSelector::select(&info, "ba").unwrap();
    match selected {
        SelectedFormat::Single(f) => {
            assert_eq!(f.format_id, "251");
            assert_eq!(f.ext, "webm");
        }
        _ => panic!("Expected Single audio format"),
    }
}

#[test]
fn test_drop_in_format_selection_explicit_ids() {
    let info = create_mock_media_info();
    let selected = FormatSelector::select(&info, "137+140").unwrap();
    match selected {
        SelectedFormat::Merged { video, audio } => {
            assert_eq!(video.format_id, "137");
            assert_eq!(audio.format_id, "140");
            assert_eq!(audio.ext, "m4a");
        }
        _ => panic!("Expected Merged format"),
    }
}

#[test]
fn test_drop_in_format_selection_height_filter() {
    let info = create_mock_media_info();
    let selected = FormatSelector::select(&info, "bestvideo[height<=360]+bestaudio/best").unwrap();
    match selected {
        SelectedFormat::Merged { video, audio } => {
            assert_eq!(video.format_id, "18");
            assert_eq!(audio.format_id, "251");
        }
        _ => panic!("Expected Merged format with 360p video"),
    }
}

#[test]
fn test_drop_in_hls_playlist_parser() {
    let manifest = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:6.0,
segment_0.ts
#EXTINF:6.0,
segment_1.ts
#EXTINF:4.2,
segment_2.ts
#EXT-X-ENDLIST"#;

    let playlist =
        HlsDownloader::parse_manifest(manifest, "https://live.example.com/hls/stream.m3u8")
            .unwrap();

    assert!(!playlist.is_master);
    assert_eq!(playlist.segments.len(), 3);
    assert_eq!(
        playlist.segments[0].url,
        "https://live.example.com/hls/segment_0.ts"
    );
    assert_eq!(playlist.segments[0].duration, 6.0);
    assert_eq!(
        playlist.segments[2].url,
        "https://live.example.com/hls/segment_2.ts"
    );
    assert_eq!(playlist.segments[2].duration, 4.2);
}

#[test]
fn test_drop_in_dash_manifest_parser() {
    let manifest = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
        <BaseURL>https://cdn.example.com/dash/1080p/</BaseURL>
        <SegmentTemplate initialization="init-1080p.mp4" media="segment-$Number%05d$.m4s" startNumber="1">
            <SegmentTimeline>
                <S t="0" d="2000"/>
                <S t="2000" d="2000"/>
            </SegmentTimeline>
        </SegmentTemplate>
    </MPD>"#;

    let dash =
        DashDownloader::parse_manifest(manifest, "https://example.com/video/manifest.mpd").unwrap();

    assert_eq!(
        dash.init_url,
        Some("https://cdn.example.com/dash/1080p/init-1080p.mp4".into())
    );
    assert_eq!(dash.media_segments.len(), 2);
    assert_eq!(
        dash.media_segments[0].url,
        "https://cdn.example.com/dash/1080p/segment-00001.m4s"
    );
    assert_eq!(
        dash.media_segments[1].url,
        "https://cdn.example.com/dash/1080p/segment-00002.m4s"
    );
}

#[test]
fn test_drop_in_fallback_bridge_json_parity() {
    let fixture = json!({
        "id": "vimeo_123456",
        "title": "Cinematic Drone Reel 4K",
        "uploader": "Aero Studios",
        "duration": 142,
        "thumbnail": "https://i.vimeocdn.com/video/123456.jpg",
        "webpage_url": "https://vimeo.com/123456",
        "formats": [
            {
                "format_id": "hls-1080p",
                "url": "https://player.vimeo.com/hls/1080p.m3u8",
                "ext": "mp4",
                "width": 1920,
                "height": 1080,
                "fps": 60.0,
                "vcodec": "avc1.640028",
                "acodec": "mp4a.40.2",
                "protocol": "m3u8_native",
                "tbr": 5000.0
            }
        ]
    });

    let info = FallbackBridgeExtractor::parse_yt_dlp_json(&fixture)
        .expect("yt-dlp JSON schema should deserialize into MediaInfo");

    assert_eq!(info.id, "vimeo_123456");
    assert_eq!(info.title, "Cinematic Drone Reel 4K");
    assert_eq!(info.uploader.as_deref(), Some("Aero Studios"));
    assert_eq!(info.duration, Some(142));
    assert_eq!(info.formats.len(), 1);
    assert_eq!(info.formats[0].protocol.as_deref(), Some("m3u8_native"));
    assert_eq!(info.formats[0].height, Some(1080));
}

#[test]
fn test_drop_in_generic_media_detection() {
    let extractor = GenericExtractor::new();
    assert!(extractor.matches_url("https://example.com/assets/trailer.mp4"));
    assert!(extractor.matches_url("https://example.com/stream.m3u8?token=xyz"));
    assert!(extractor.matches_url("https://example.com/manifest.mpd"));
}

#[test]
fn test_output_template_expansion_parity() {
    let template = "%(uploader)s - %(title)s [%(id)s].%(ext)s";
    let filename = expand_output_template(
        template,
        "dQw4w9WgXcQ",
        "Never Gonna Give You Up",
        Some("Rick Astley"),
        "mp4",
    );
    assert_eq!(
        filename,
        "Rick Astley - Never Gonna Give You Up [dQw4w9WgXcQ].mp4"
    );

    let rich_template =
        "%(uploader)s/%(height)sp/%(title)s [%(format_id)s] (%(resolution)s).%(ext)s";
    let ctx = mm_dlp_core::utils::TemplateContext {
        id: "dQw4w9WgXcQ",
        title: "Never Gonna Give You Up",
        uploader: Some("Rick Astley"),
        ext: "mp4",
        height: Some(1080),
        width: Some(1920),
        fps: Some(60.0),
        format_id: Some("137+140"),
    };
    let rich_filename = mm_dlp_core::utils::expand_output_template_with_meta(rich_template, &ctx);
    assert_eq!(
        rich_filename,
        "Rick Astley/1080p/Never Gonna Give You Up [137+140] (1920x1080).mp4"
    );
}

#[test]
fn test_sanitize_url_preserves_youtube_id() {
    let raw = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=share#t=20";
    let clean = sanitize_url(raw).unwrap();
    assert_eq!(clean, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
}

#[test]
fn test_gecko_firefox_cookie_extraction_parity() {
    use mm_dlp_core::client::cookies::{BrowserCookieExtractor, FirefoxCookieExtractor};
    let temp_root = std::env::temp_dir().join(format!("test_ff_profile_{}", std::process::id()));
    let profile_dir = temp_root.join("Profiles/default-release");
    std::fs::create_dir_all(&profile_dir).unwrap();

    let ini_content = "\
[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/default-release
Default=1
";
    std::fs::write(temp_root.join("profiles.ini"), ini_content).unwrap();

    let db_path = profile_dir.join("cookies.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE moz_cookies (
            id INTEGER PRIMARY KEY,
            originAttributes TEXT,
            name TEXT,
            value TEXT,
            host TEXT,
            path TEXT,
            expiry INTEGER,
            lastAccessed INTEGER,
            creationTime INTEGER,
            isSecure INTEGER,
            isHttpOnly INTEGER,
            inBrowserElement INTEGER,
            sameSite INTEGER,
            rawSameSite INTEGER,
            schemeMap INTEGER
        );
        INSERT INTO moz_cookies (name, value, host, path, expiry, isSecure, isHttpOnly)
        VALUES ('LOGIN_INFO', 'gecko_session_test', '.youtube.com', '/', 1900000000, 1, 1);
        ",
    )
    .unwrap();
    drop(conn);

    let extracted = FirefoxCookieExtractor::extract_from_sqlite(&db_path).unwrap();
    assert_eq!(extracted.len(), 1);
    assert_eq!(extracted[0].name, "LOGIN_INFO");
    assert_eq!(extracted[0].value, "gecko_session_test");
    assert!(extracted[0].secure);
    assert!(extracted[0].http_only);

    let netscape =
        FirefoxCookieExtractor::extract_to_netscape(Some(profile_dir.to_str().unwrap())).unwrap();
    assert!(
        netscape
            .contains(".youtube.com\tTRUE\t/\tTRUE\t1900000000\tLOGIN_INFO\tgecko_session_test")
    );

    let custom_extracted =
        BrowserCookieExtractor::extract(&format!("firefox:{}", profile_dir.display())).unwrap();
    assert!(custom_extracted.contains("LOGIN_INFO\tgecko_session_test"));

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_gecko_impersonation_user_agent_and_headers() {
    use mm_dlp_core::client::h2::{BrowserProfile, H2Impersonator};
    use mm_dlp_core::config::{EngineConfig, FIREFOX_GECKO_USER_AGENT};
    use mm_dlp_core::downloader::build_http_client_with_config;

    let mut config = EngineConfig::default();
    config.impersonate_target = Some("firefox".to_string());
    assert!(config.is_gecko_or_firefox());

    let client = build_http_client_with_config(&config).unwrap();
    drop(client);

    // Verify Firefox/Gecko HTTP/2 pseudo-header order: :method, :path, :authority, :scheme
    let gecko_headers = H2Impersonator::format_pseudo_headers_for(
        BrowserProfile::Firefox,
        "GET",
        "manifest.googlevideo.com",
        "https",
        "/api/playback",
    );
    assert_eq!(gecko_headers[0].0, ":method");
    assert_eq!(gecko_headers[1].0, ":path");
    assert_eq!(gecko_headers[2].0, ":authority");
    assert_eq!(gecko_headers[3].0, ":scheme");

    // Verify User-Agent
    assert!(FIREFOX_GECKO_USER_AGENT.contains("Gecko/20100101 Firefox/"));
}

#[tokio::test]
async fn test_custom_dns_configuration_and_fallback() {
    use mm_dlp_core::client::dns::{CustomDnsResolver, default_dns_servers, parse_dns_servers};
    use mm_dlp_core::config::EngineConfig;
    use mm_dlp_core::downloader::build_http_client_with_config;

    let defaults = default_dns_servers();
    assert_eq!(
        defaults[0],
        "1.1.1.1:53".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(
        defaults[1],
        "9.9.9.9:53".parse::<std::net::SocketAddr>().unwrap()
    );

    let custom = parse_dns_servers("8.8.8.8, 9.9.9.9:53; 1.1.1.1").unwrap();
    assert_eq!(custom.len(), 3);
    assert_eq!(
        custom[0],
        "8.8.8.8:53".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(
        custom[1],
        "9.9.9.9:53".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(
        custom[2],
        "1.1.1.1:53".parse::<std::net::SocketAddr>().unwrap()
    );

    let resolver = CustomDnsResolver::new(custom);
    let resolved = resolver.resolve_host("127.0.0.1", 443).await.unwrap();
    assert_eq!(
        resolved[0],
        "127.0.0.1:443".parse::<std::net::SocketAddr>().unwrap()
    );

    // Verify engine builds successfully with custom DNS
    let mut config = EngineConfig::default();
    config.dns_servers = Some("1.1.1.1, 9.9.9.9".to_string());
    let client = build_http_client_with_config(&config).unwrap();
    drop(client);
}

#[test]
fn test_safari_impersonation_user_agent_and_headers() {
    use mm_dlp_core::client::h2::{BrowserProfile, H2Impersonator};
    use mm_dlp_core::client::h3::H3Impersonator;
    use mm_dlp_core::config::{EngineConfig, SAFARI_IOS_USER_AGENT, SAFARI_MACOS_USER_AGENT};
    use mm_dlp_core::downloader::build_http_client_with_config;
    use quiche::h3::NameValue;

    // Test macOS Safari impersonation
    let mut mac_config = EngineConfig::default();
    mac_config.impersonate_target = Some("safari".to_string());
    assert!(mac_config.is_safari_or_webkit());
    assert!(!mac_config.is_ios_safari());

    let client = build_http_client_with_config(&mac_config).unwrap();
    drop(client);

    // Test iOS Safari impersonation
    let mut ios_config = EngineConfig::default();
    ios_config.impersonate_target = Some("ios".to_string());
    assert!(ios_config.is_safari_or_webkit());
    assert!(ios_config.is_ios_safari());

    let ios_client = build_http_client_with_config(&ios_config).unwrap();
    drop(ios_client);

    // Verify Safari / WebKit HTTP/2 pseudo-header order: :method, :scheme, :path, :authority
    let safari_h2 = H2Impersonator::format_pseudo_headers_for(
        BrowserProfile::Safari,
        "GET",
        "safari.apple.com",
        "https",
        "/media/stream.m3u8",
    );
    assert_eq!(safari_h2[0].0, ":method");
    assert_eq!(safari_h2[1].0, ":scheme");
    assert_eq!(safari_h2[2].0, ":path");
    assert_eq!(safari_h2[3].0, ":authority");

    // Verify Safari / WebKit HTTP/3 request headers order: :method, :scheme, :path, :authority
    let safari_h3 = H3Impersonator::format_request_headers_for(
        BrowserProfile::Safari,
        "GET",
        "safari.apple.com",
        "https",
        "/media/stream.m3u8",
        None,
    );
    assert_eq!(safari_h3[0].name(), b":method");
    assert_eq!(safari_h3[1].name(), b":scheme");
    assert_eq!(safari_h3[2].name(), b":path");
    assert_eq!(safari_h3[3].name(), b":authority");

    // Verify User-Agents
    assert!(SAFARI_MACOS_USER_AGENT.contains("Macintosh"));
    assert!(SAFARI_MACOS_USER_AGENT.contains("Version/18.3 Safari/"));
    assert!(SAFARI_IOS_USER_AGENT.contains("iPhone"));
    assert!(SAFARI_IOS_USER_AGENT.contains("Mobile/15E148 Safari/"));
}

#[test]
fn test_safari_binarycookies_extraction_via_browser_extractor() {
    use mm_dlp_core::client::cookies::BrowserCookieExtractor;

    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(format!(
        "test_safari_cookies_{}.binarycookies",
        std::process::id()
    ));

    let domain = ".safari-test.org\0";
    let name = "APPLE_AUTH\0";
    let path = "/\0";
    let value = "safari_secure_token_12345\0";

    let url_offset = 56u32;
    let name_offset = url_offset + domain.len() as u32;
    let path_offset = name_offset + name.len() as u32;
    let value_offset = path_offset + path.len() as u32;
    let cookie_size = value_offset + value.len() as u32;

    let mut cookie_record = Vec::new();
    cookie_record.extend_from_slice(&cookie_size.to_le_bytes());
    cookie_record.extend_from_slice(&[0u8; 4]);
    cookie_record.extend_from_slice(&5u32.to_le_bytes()); // secure & http_only
    cookie_record.extend_from_slice(&[0u8; 4]);
    cookie_record.extend_from_slice(&url_offset.to_le_bytes());
    cookie_record.extend_from_slice(&name_offset.to_le_bytes());
    cookie_record.extend_from_slice(&path_offset.to_le_bytes());
    cookie_record.extend_from_slice(&value_offset.to_le_bytes());
    cookie_record.extend_from_slice(&[0u8; 8]);
    let mac_expiry: f64 = 821_692_800.0; // +978307200 = 1800000000
    cookie_record.extend_from_slice(&mac_expiry.to_le_bytes());
    let mac_creation: f64 = 800_000_000.0;
    cookie_record.extend_from_slice(&mac_creation.to_le_bytes());
    cookie_record.extend_from_slice(domain.as_bytes());
    cookie_record.extend_from_slice(name.as_bytes());
    cookie_record.extend_from_slice(path.as_bytes());
    cookie_record.extend_from_slice(value.as_bytes());

    let cookie_offset_in_page = 16u32;
    let page_size = cookie_offset_in_page + cookie_record.len() as u32;

    let mut page = Vec::new();
    page.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    page.extend_from_slice(&1u32.to_le_bytes());
    page.extend_from_slice(&cookie_offset_in_page.to_le_bytes());
    page.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    page.extend_from_slice(&cookie_record);

    let mut file_bytes = Vec::new();
    file_bytes.extend_from_slice(b"cook");
    file_bytes.extend_from_slice(&1u32.to_be_bytes());
    file_bytes.extend_from_slice(&page_size.to_be_bytes());
    file_bytes.extend_from_slice(&page);

    std::fs::write(&file_path, &file_bytes).expect("Failed to write mock binarycookies file");

    let extracted =
        BrowserCookieExtractor::extract(&format!("safari:{}", file_path.display())).unwrap();
    assert!(extracted.contains("# Extracted by mm-dlp (Safari/WebKit/CFNetwork)"));
    assert!(extracted.contains(
        ".safari-test.org\tTRUE\t/\tTRUE\t1800000000\tAPPLE_AUTH\tsafari_secure_token_12345"
    ));

    let _ = std::fs::remove_file(&file_path);
}

#[test]
fn test_phase1_engine_config_options() {
    use mm_dlp_core::config::EngineConfig;

    let mut config = EngineConfig::default();
    assert!(!config.dangerously_insecure);
    assert!(config.custom_headers.is_none());

    config.dangerously_insecure = true;
    config.custom_headers = Some(vec![
        "Authorization: Bearer test_secret".to_string(),
        "X-Custom-Client: test_val".to_string(),
    ]);

    assert!(config.dangerously_insecure);
    let headers = config.custom_headers.unwrap();
    assert_eq!(headers.len(), 2);
    assert_eq!(headers[0], "Authorization: Bearer test_secret");
}

#[tokio::test]
async fn test_phase1_processor_audio_formats() {
    use mm_dlp_core::processor::{
        extract_audio_from_file, resolve_ffmpeg_path, transcode_stream_to_writer,
    };
    use std::path::Path;

    if resolve_ffmpeg_path().is_err() {
        return;
    }

    // Test channel setup and execution for transcode_stream_to_writer
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    drop(tx);

    // Verify format mapping doesn't panic on valid phase 1 formats
    let _ = transcode_stream_to_writer(rx, tokio::io::sink(), "best", false).await;

    // Test extract_audio_from_file with mock non-existent file returning ffmpeg or IO error gracefully without panic
    let p_in = Path::new("/tmp/non_existent_input.mp4");
    let p_out = Path::new("/tmp/non_existent_output.mp3");
    let _ = extract_audio_from_file(p_in, p_out, "m4a", "192k", false).await;
    let _ = extract_audio_from_file(p_in, p_out, "opus", "best", false).await;
    let _ = extract_audio_from_file(p_in, p_out, "flac", "best", false).await;
    let _ = extract_audio_from_file(p_in, p_out, "wav", "best", false).await;
}

#[tokio::test]
async fn test_phase2_fragment_resilience_and_downloaders() {
    use mm_dlp_core::config::EngineConfig;
    use mm_dlp_core::downloader::dash::DashDownloader;
    use mm_dlp_core::downloader::fetch_bytes_with_retry;
    use mm_dlp_core::downloader::hls::HlsDownloader;
    use tokio_util::sync::CancellationToken;

    // Verify EngineConfig defaults for Phase 2
    let config = EngineConfig::default();
    assert_eq!(config.fragment_retries, 10);
    assert!(config.skip_unavailable_fragments);

    let client = reqwest::Client::new();
    let cancel = CancellationToken::new();

    // Verify fetch_bytes_with_retry fails fast when cancelled
    cancel.cancel();
    let err = fetch_bytes_with_retry(&client, "http://127.0.0.1:9999/dummy", &cancel, 2).await;
    assert!(err.is_err());

    // Verify HlsDownloader and DashDownloader builders
    let hls = HlsDownloader::new(client.clone(), 4)
        .with_retries(5)
        .with_skip_unavailable(false);
    drop(hls);

    let dash = DashDownloader::new(client, 4)
        .with_retries(3)
        .with_skip_unavailable(true);
    drop(dash);
}
