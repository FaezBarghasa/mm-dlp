use criterion::{criterion_group, criterion_main, Criterion};
use mm_dlp_core::domain::playlist::{Playlist, Track};
use mm_dlp_core::extractor::traits::AudioSource;
use mm_dlp_core::data::playlist::{json_handler, xml_handler};

fn create_large_playlist(num_tracks: usize) -> Playlist {
    let tracks = (0..num_tracks)
        .map(|i| Track {
            id: format!("track-{}", i),
            title: format!("Track {}", i),
            artist: format!("Artist {}", i),
            album: Some(format!("Album {}", i)),
            source_url: format!("http://example.com/track{}", i),
            duration: 180,
        })
        .collect();

    Playlist {
        id: "large-playlist".to_string(),
        name: "Large Test Playlist".to_string(),
        description: None,
        tracks,
        source: AudioSource::Spotify,
    }
}

fn benchmark_playlist_parsing(c: &mut Criterion) {
    let playlist = create_large_playlist(10_000);
    let json = json_handler::export_to_json(&playlist).unwrap();
    let xml = xml_handler::export_to_xml(&playlist).unwrap();

    let mut group = c.benchmark_group("Playlist Parsing");

    group.bench_function("JSON Deserialization (10,000 tracks)", |b| {
        b.iter(|| json_handler::import_from_json(&json).unwrap());
    });

    group.bench_function("XML Deserialization (10,000 tracks)", |b| {
        b.iter(|| xml_handler::import_from_xml(&xml).unwrap());
    });

    group.finish();
}

criterion_group!(benches, benchmark_playlist_parsing);
criterion_main!(benches);
