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
