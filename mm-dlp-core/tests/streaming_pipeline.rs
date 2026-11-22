use bytes::Bytes;
use mm_dlp_core::downloader::dash::DashDownloader;
use mm_dlp_core::downloader::flusher::SequentialFlusher;
use mm_dlp_core::downloader::hls::HlsDownloader;
use mm_dlp_core::downloader::parallel::{EwmaBandwidthEstimator, SequencedChunk};
use std::io::Cursor;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_streaming_flusher_ordered_piping() {
    let flusher = SequentialFlusher::new();
    let (tx, rx) = mpsc::channel(16);

    // Feed chunks out of order: 3, 1, 0, 2
    let indices = [3, 1, 0, 2];
    for &idx in &indices {
        let chunk = SequencedChunk {
            index: idx,
            data: Bytes::from(format!("PART{}", idx)),
        };
        tx.send(Ok(chunk)).await.expect("Channel send failed");
    }

    let mut output = Cursor::new(Vec::new());
    let total_bytes = flusher
        .flush_to_writer(&mut output, rx, 4)
        .await
        .expect("Flush to writer failed");

    assert_eq!(total_bytes, 20); // 4 * 5 bytes
    assert_eq!(output.into_inner(), b"PART0PART1PART2PART3");
}

#[tokio::test]
async fn test_streaming_ewma_pacing() {
    let mut estimator = EwmaBandwidthEstimator::new(0.2);

    // 10 MiB in 1 second = 10,485,760 B/s
    estimator.update(10 * 1024 * 1024, 1.0);
    let estimate = estimator.current_estimate();
    assert!((estimate - 10485760.0).abs() < 1.0);

    // Small chunk size: delay should be within bounds
    let delay = estimator.calculate_jitter_delay(64 * 1024, 10.0);
    assert!(delay.as_millis() <= 5);
}

#[test]
fn test_hls_live_edge_manifest_detection() {
    let live_manifest = r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-TARGETDURATION:2
#EXT-X-MEDIA-SEQUENCE:98200
#EXTINF:2.000,
chunk_98200.ts
#EXTINF:2.000,
chunk_98201.ts
#EXTINF:2.000,
chunk_98202.ts"#;

    let parsed =
        HlsDownloader::parse_manifest(live_manifest, "https://live.example.com/stream.m3u8")
            .expect("Failed to parse live HLS manifest");

    assert!(parsed.is_live, "Playlist should be marked live");
    assert_eq!(parsed.media_sequence, 98200);
    assert_eq!(parsed.target_duration, Some(2.0));
    assert_eq!(parsed.segments.len(), 3);
}

#[test]
fn test_dash_dynamic_manifest_detection() {
    let dynamic_mpd = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="dynamic" minimumUpdatePeriod="PT1.5S">
        <BaseURL>https://live-dash.example.com/</BaseURL>
        <SegmentTemplate initialization="init.mp4" media="seg-$Number$.m4s" startNumber="50">
            <SegmentTimeline>
                <S t="0" d="1500"/>
                <S t="1500" d="1500"/>
            </SegmentTimeline>
        </SegmentTemplate>
    </MPD>"#;

    let parsed =
        DashDownloader::parse_manifest(dynamic_mpd, "https://live-dash.example.com/manifest.mpd")
            .expect("Failed to parse dynamic DASH manifest");

    assert!(parsed.is_dynamic, "MPD should be identified as dynamic");
    assert_eq!(parsed.min_update_period, Some(1.5));
    assert_eq!(parsed.media_segments.len(), 2);
}

#[tokio::test]
async fn test_streaming_flusher_cancellation() {
    let flusher = SequentialFlusher::new();
    let (tx, rx) = mpsc::channel(4);
    let cancel_token = CancellationToken::new();

    tx.send(Ok(SequencedChunk {
        index: 0,
        data: Bytes::from_static(b"HELLO"),
    }))
    .await
    .expect("Send failed");

    let cancel_clone = cancel_token.clone();
    let mut writer = Cursor::new(Vec::new());

    let handle = tokio::spawn(async move {
        flusher
            .flush_live_to_writer(&mut writer, rx, cancel_clone)
            .await
    });

    cancel_token.cancel();
    let res = handle.await.expect("Task failed");
    assert!(res.is_ok());
}
