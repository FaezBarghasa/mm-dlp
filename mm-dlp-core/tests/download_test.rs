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
    flusher.flush_to_disk(&temp_path, rx, 3).await.expect("Failed to flush to disk");

    let contents = fs::read_to_string(&temp_path).expect("Failed to read flushed file");
    assert_eq!(contents, "chunk0chunk1chunk2"); // Sequential assurance

    std::fs::remove_file(&temp_path).unwrap();
}