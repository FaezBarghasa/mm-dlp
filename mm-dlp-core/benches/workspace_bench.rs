use criterion::{criterion_group, criterion_main, Criterion};
use mm_dlp_core::config::EngineConfig;

fn bench_engine_config_init(c: &mut Criterion) {
    c.bench_function("engine_config_init", |b| {
        b.iter(|| {
            let _config = EngineConfig::default();
        })
    });
}

criterion_group!(benches, bench_engine_config_init);
criterion_main!(benches);
