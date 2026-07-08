use criterion::{criterion_group, criterion_main, Criterion};
use serde_json::Value;

fn benchmark_json_parsing(c: &mut Criterion) {
    let json_data = r#"
    {
        "videoDetails": {
            "title": "Test Title",
            "author": "Test Author",
            "lengthSeconds": "300",
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
    }
    "#;

    c.bench_function("parse youtube player response", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(json_data).unwrap();
        })
    });
}

criterion_group!(benches, benchmark_json_parsing);
criterion_main!(benches);
