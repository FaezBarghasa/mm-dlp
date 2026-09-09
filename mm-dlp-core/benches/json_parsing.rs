use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use uniffi_mmdlp::data::playlist::json_handler;
use uniffi_mmdlp::domain::playlist::{Playlist, Track};
use uniffi_mmdlp::extractor::traits::AudioSource;

// ─── YouTube player response parse ───────────────────────────────────────────

fn benchmark_yt_json_parsing(c: &mut Criterion) {
    let json_data = r#"{
        "videoDetails": {
            "title": "Never Gonna Give You Up",
            "author": "Rick Astley",
            "lengthSeconds": "213",
            "thumbnail": {
                "thumbnails": [
                    { "url": "http://example.com/thumb1.jpg" },
                    { "url": "http://example.com/thumb2.jpg" }
                ]
            }
        },
        "streamingData": {
            "adaptiveFormats": [
                {
                    "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                    "bitrate": 128000,
                    "url": "http://example.com/audio.mp4"
                },
                {
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": 160000,
                    "url": "http://example.com/audio.webm"
                }
            ]
        }
    }"#;

    c.bench_function("parse youtube player response", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(json_data)).unwrap();
        })
    });
}

// ─── Playlist JSON round-trip ─────────────────────────────────────────────────

fn make_playlist(track_count: usize) -> Playlist {
    Playlist {
        id: "bench-pl".to_string(),
        name: "Benchmark Playlist".to_string(),
        description: None,
        tracks: (0..track_count)
            .map(|i| Track {
                id: format!("t-{}", i),
                title: format!("Track {}", i),
                artist: format!("Artist {}", i % 20),
                album: Some(format!("Album {}", i / 20)),
                source_url: format!("https://example.com/{}", i),
                duration: 200,
            })
            .collect(),
        source: AudioSource::SoundCloud,
    }
}

fn benchmark_playlist_json_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("playlist_json_roundtrip");

    for size in [100usize, 1_000, 10_000] {
        let playlist = make_playlist(size);
        let json = json_handler::export_to_json(&playlist).expect("export failed");

        group.bench_with_input(
            BenchmarkId::new("import_from_json", size),
            &json,
            |b, json_str| {
                b.iter(|| {
                    let _ = json_handler::import_from_json(black_box(json_str)).unwrap();
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("export_to_json", size),
            &playlist,
            |b, pl| {
                b.iter(|| {
                    let _ = json_handler::export_to_json(black_box(pl)).unwrap();
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_yt_json_parsing, benchmark_playlist_json_parse);
criterion_main!(benches);
