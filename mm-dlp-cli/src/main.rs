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

    /// Optional target output directory.
    #[arg(short, long, default_value = ".")]
    output: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    println!("Initializing mm-dlp engine for target: {}", args.url);
    println!("Selected format: {}, output directory: {}", args.format, args.output);

    let config = EngineConfig::default();
    let engine = MmDlpEngine::new();
    println!("Engine initialized successfully with max concurrent downloads: {}", config.max_concurrent_downloads);

    match engine.extract_metadata(args.url.clone()) {
        Ok(info) => {
            println!("Extracted media info for title: {}", info.title);
        }
        Err(e) => {
            println!("Extraction note / status: {}", e);
        }
    }
}
