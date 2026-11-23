use mm_dlp_core::downloader::flusher::SequentialFlusher;
use mm_dlp_core::downloader::manifest::parse_m3u8;
use mm_dlp_core::downloader::parallel::download_segments;
use reqwest::Client;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[tokio::test]
async fn test_segment_downloader_and_flusher() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // Generate an asynchronous mock network layer artificially pushing segment drops and variable latency delays
    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0; 1024];
                    let _ = socket.read(&mut buf).await;
                    let req = String::from_utf8_lossy(&buf);

                    let (status, body) = if req.contains("GET /seg0.ts") {
                        ("200 OK", "chunk0")
                    } else if req.contains("GET /seg1.ts") {
                        tokio::time::sleep(Duration::from_millis(100)).await; // Simulates dropped payload/latency 
                        ("200 OK", "chunk1")
                    } else if req.contains("GET /seg2.ts") {
                        ("200 OK", "chunk2")
                    } else {
                        ("404 Not Found", "")
                    };

                    let response = format!(
                        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        }
    });

    let manifest = r#"
#EXTM3U
#EXTINF:10.0,
seg0.ts
#EXTINF:10.0,
seg1.ts
#EXTINF:10.0,
seg2.ts
    "#;

    let base_url = format!("http://127.0.0.1:{}", port);
    let segments = parse_m3u8(manifest, &base_url).expect("Failed to parse M3U8");
    assert_eq!(segments.len(), 3);

    let (tx, rx) = mpsc::channel(10);
    let client = Client::new();

    let segments_clone = segments.clone();
    tokio::spawn(async move {
        download_segments(client, segments_clone, 2, tx).await;
    });

    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let temp_path = std::env::temp_dir().join(format!("mm_dlp_test_{}.ts", id));

    let flusher = SequentialFlusher::new();
    flusher
        .flush_to_disk(&temp_path, rx, 3)
        .await
        .expect("Failed to flush to disk");

    let contents = fs::read_to_string(&temp_path).expect("Failed to read flushed file");
    assert_eq!(contents, "chunk0chunk1chunk2"); // Sequential assurance

    std::fs::remove_file(&temp_path).unwrap();
}

#[tokio::test]
async fn test_resumable_download_with_range_support() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0; 2048];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.contains("Range: bytes=5-") {
                        let response = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 5-9/10\r\nContent-Length: 5\r\nConnection: close\r\n\r\n56789";
                        let _ = socket.write_all(response.as_bytes()).await;
                    } else {
                        let response = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789";
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                });
            }
        }
    });

    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let target_path = std::env::temp_dir().join(format!("mm_dlp_resume_test_{}.bin", id));
    let part_path = std::path::PathBuf::from(format!("{}.part", target_path.display()));

    // Create partial file with first 5 bytes
    tokio::fs::write(&part_path, b"01234").await.unwrap();

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/media.bin", port);
    let cancel = tokio_util::sync::CancellationToken::new();

    let total = mm_dlp_core::downloader::download_resumable_to_file(
        &client,
        &url,
        &target_path,
        cancel,
        false,
        None,
    )
    .await
    .expect("Resume download failed");

    assert_eq!(total, 10);
    assert!(!part_path.exists(), ".part file should be renamed");
    assert!(target_path.exists(), "Target file should exist");

    let content = tokio::fs::read(&target_path).await.unwrap();
    assert_eq!(content, b"0123456789");

    let _ = tokio::fs::remove_file(&target_path).await;
}

#[tokio::test]
async fn test_direct_download_with_no_part_flag() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0; 2048];
                    let _ = socket.read(&mut buf).await;
                    let response =
                        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        }
    });

    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let target_path = std::env::temp_dir().join(format!("mm_dlp_nopart_test_{}.bin", id));
    let part_path = std::path::PathBuf::from(format!("{}.part", target_path.display()));

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/direct.bin", port);
    let cancel = tokio_util::sync::CancellationToken::new();

    let total = mm_dlp_core::downloader::download_resumable_to_file(
        &client,
        &url,
        &target_path,
        cancel,
        true, // no_part = true
        None,
    )
    .await
    .expect("Direct download failed");

    assert_eq!(total, 5);
    assert!(
        !part_path.exists(),
        ".part file should never exist when no_part=true"
    );
    assert!(target_path.exists());

    let content = tokio::fs::read(&target_path).await.unwrap();
    assert_eq!(content, b"hello");

    let _ = tokio::fs::remove_file(&target_path).await;
}
