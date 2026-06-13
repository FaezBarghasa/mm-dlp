use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mm_dlp_core::downloader::mmap::MmapWriter;
use std::env::temp_dir;
use std::fs;

fn mmap_allocation_benchmark(c: &mut Criterion) {
    c.bench_function("mmap_write_zero_copy_10MB", |b| {
        b.iter(|| {
            let path = temp_dir().join("bench_mmap_fast.bin");
            
            // Pre-allocate a simulated 10MB DASH segment block
            let size = 10 * 1024 * 1024;
            let mut writer = MmapWriter::new(&path, size).expect("Failed to create MmapWriter");
            
            let mock_network_chunk = vec![0x41; 1024]; // 1KB frame chunk
            
            // Write chunks into scattered offsets to simulate out-of-order segment arrivals
            for i in 0..10 {
                writer.write_at_offset(black_box(i * 1024), black_box(&mock_network_chunk)).unwrap();
            }
            
            fs::remove_file(path).unwrap();
        });
    });
}

criterion_group!(benches, mmap_allocation_benchmark);
criterion_main!(benches);