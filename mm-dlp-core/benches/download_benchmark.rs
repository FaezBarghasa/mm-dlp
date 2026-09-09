use criterion::{criterion_group, criterion_main, Criterion, AsyncBencher};
use mm_dlp_core::network::quic_client::QuicHttpClient;
use reqwest::Client;
use tokio::runtime::Runtime;
use url::Url;

async fn benchmark_quic_download(client: &QuicHttpClient, url: &Url) {
    let _ = client.get(url).await.unwrap();
}

async fn benchmark_http2_download(client: &Client, url: &Url) {
    let _ = client.get(url.clone()).send().await.unwrap();
}

fn benchmark_downloads(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let quic_client = QuicHttpClient::new().unwrap();
    let http2_client = Client::new();
    let url = Url::parse("https://www.google.com").unwrap(); // Using a known high-availability server

    let mut group = c.benchmark_group("Download Comparison");

    group.bench_function("QUIC Download", |b| {
        b.to_async(&rt).iter(|| benchmark_quic_download(&quic_client, &url));
    });

    group.bench_function("HTTP/2 Download", |b| {
        b.to_async(&rt).iter(|| benchmark_http2_download(&http2_client, &url));
    });

    group.finish();
}

criterion_group!(benches, benchmark_downloads);
criterion_main!(benches);
