use clap::Parser;
use mm_dlp_core::config::EngineConfig;
use mm_dlp_core::MmDlpEngine;

/// Command line arguments for mm-dlp-cli tool.
#[derive(Parser, Debug)]
#[command(author, version, about = "High performance media downloader CLI", long_about = None)]
struct Args {
    /// The target URL to extract metadata or download media from.
    #[arg(short, long)]
    url: String,

    /// Target audio output format (e.g. mp3, flac, opus, wav).
    #[arg(short, long, default_value = "mp3")]
    format: String,

    /// Optional media source identifier (e.g. youtube, spotify, soundcloud).
    #[arg(short, long, default_value = "auto")]
    source: String,
}

fn main() {
    let args = Args::parse();
    println!("Initializing mm-dlp engine for URL: {}", args.url);
    println!("Format: {}, Source: {}", args.format, args.source);

    let config = EngineConfig::default();
    let _engine = MmDlpEngine::new();
    println!(
        "Engine initialized successfully with max concurrent downloads: {}",
        config.max_concurrent_downloads
    );
}
