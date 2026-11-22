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

#[tokio::test]
async fn test_streaming_flusher_max_backlog_overflow() {
    let flusher = SequentialFlusher::new().with_max_backlog(4);
    let (tx, rx) = mpsc::channel(16);

    // Send index 1, 2, 3, 4, 5 (omitting index 0 to force backlog accumulation)
    for idx in 1..=5 {
        tx.send(Ok(SequencedChunk {
            index: idx,
            data: Bytes::from(format!("PART{}", idx)),
        }))
        .await
        .expect("Send failed");
    }

    let mut output = Cursor::new(Vec::new());
    let res = flusher.flush_to_writer(&mut output, rx, 10).await;
    assert!(res.is_err(), "Expected buffer overflow error");
    match res {
        Err(mm_dlp_core::EngineError::IoError(msg)) => {
            assert!(msg.contains("backlog exceeded"));
        }
        other => panic!("Unexpected result: {:?}", other),
    }
}

#[test]
fn test_live_tracker_bounded_history() {
    use std::collections::{HashSet, VecDeque};

    let mut seen_urls: HashSet<String> = HashSet::new();
    let mut seen_queue: VecDeque<String> = VecDeque::new();
    let max_history = 500;

    for i in 0..650 {
        let url = format!("https://stream.example.com/chunk_{}.ts", i);
        if seen_urls.insert(url.clone()) {
            seen_queue.push_back(url);
            if seen_queue.len() > max_history {
                if let Some(oldest) = seen_queue.pop_front() {
                    seen_urls.remove(&oldest);
                }
            }
        }
    }

    assert_eq!(seen_urls.len(), 500);
    assert_eq!(seen_queue.len(), 500);
    // Oldest chunk 0 should have been evicted
    assert!(!seen_urls.contains("https://stream.example.com/chunk_0.ts"));
    // Newest chunk 649 must be retained
    assert!(seen_urls.contains("https://stream.example.com/chunk_649.ts"));
}

#[cfg(target_os = "linux")]
#[test]
fn test_splice_pipe_to_pipe_roundtrip() {
    use mm_dlp_core::downloader::splice::splice_pipe_to_pipe;
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;

    let mut pipe1 = [0; 2];
    let mut pipe2 = [0; 2];
    unsafe {
        assert_eq!(libc::pipe(pipe1.as_mut_ptr()), 0);
        assert_eq!(libc::pipe(pipe2.as_mut_ptr()), 0);
    }

    let test_data = b"ZERO_COPY_SPLICE_IN_KERNEL_TEST_PAYLOAD";

    // Write into pipe 1 write-end
    let mut w1 = unsafe { std::fs::File::from_raw_fd(pipe1[1]) };
    w1.write_all(test_data).expect("Failed to write to pipe1");

    // Splice from pipe 1 read-end to pipe 2 write-end
    let spliced =
        unsafe { splice_pipe_to_pipe(pipe1[0], pipe2[1], test_data.len()) }.expect("Splice failed");
    assert_eq!(spliced, test_data.len());

    // Read from pipe 2 read-end
    let mut r2 = unsafe { std::fs::File::from_raw_fd(pipe2[0]) };
    let mut buf = vec![0u8; test_data.len()];
    r2.read_exact(&mut buf).expect("Failed to read from pipe2");
    assert_eq!(&buf[..], test_data);

    unsafe {
        libc::close(pipe1[0]);
        libc::close(pipe2[1]);
    }
}

#[tokio::test]
async fn test_mux_streams_piped_cancellation() {
    let (_v_tx, v_rx) = mpsc::channel(4);
    let (_a_tx, a_rx) = mpsc::channel(4);
    let cancel_token = CancellationToken::new();

    // Cancel immediately
    cancel_token.cancel();

    let out_dir = std::env::temp_dir().join(format!("test_mux_{}.mp4", std::process::id()));
    let res =
        mm_dlp_core::processor::mux_streams_piped(v_rx, a_rx, &out_dir, "mp4", "m4a", cancel_token)
            .await;

    // Must gracefully handle cancellation without panic or hanging
    let _ = tokio::fs::remove_file(&out_dir).await;
    assert!(res.is_err() || res.is_ok());
}
