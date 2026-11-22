use clap::{Parser, Subcommand};
use mm_dlp_core::config::{DownloadConfig, DownloadResult, EngineConfig};
use mm_dlp_core::error::EngineError;
use mm_dlp_core::{DownloadProgressCallback, MmDlpEngine};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

struct CliProgressCallback {
    has_completed: AtomicBool,
}

impl CliProgressCallback {
    fn new() -> Self {
        Self {
            has_completed: AtomicBool::new(false),
        }
    }
}

impl DownloadProgressCallback for CliProgressCallback {
    fn on_status(&self, status: String) {
        println!("[mm-dlp] {}", status);
    }

    fn on_progress(&self, progress_percentage: f64) {
        print!("\r[download] {:>5.1}%", progress_percentage);
        let _ = std::io::stdout().flush();
    }

    fn on_error(&self, error: EngineError) {
        eprintln!("\n[error] Download failed: {}", error);
    }

    fn on_complete(&self, result: DownloadResult) {
        self.has_completed.store(true, Ordering::SeqCst);
        println!(
            "\n[complete] File saved to: {} ({} bytes, took {} ms)",
            result.file_path, result.total_bytes, result.elapsed_millis
        );
    }
}

/// High performance media downloader & extractor (Rust port of yt-dlp)
#[derive(Parser, Debug)]
#[command(
    name = "mm-dlp",
    author,
    version,
    about = "High performance media downloader CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Direct URL to download (shorthand when no subcommand is specified)
    #[arg(value_name = "URL")]
    url: Option<String>,

    /// Destination directory
    #[arg(short, long, default_value = ".")]
    output_dir: String,

    /// Audio output format (e.g. mp3, m4a, flac, opus)
    #[arg(short, long, default_value = "mp3")]
    format: String,

    /// Target quality (e.g. best, high, medium, low)
    #[arg(short, long, default_value = "best")]
    quality: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Extract and display all available formats and metadata for a given URL without downloading
    Info {
        /// Target URL (YouTube, Vimeo, SoundCloud, Spotify)
        url: String,
    },
    /// Download video or audio from the specified URL
    Download {
        /// Target URL
        url: String,
        /// Destination directory
        #[arg(short, long, default_value = ".")]
        output_dir: String,
        /// Audio format (mp3, m4a, flac, opus)
        #[arg(short, long, default_value = "mp3")]
        format: String,
        /// Quality preference
        #[arg(short, long, default_value = "best")]
        quality: String,
    },
    /// Search a supported platform for tracks/videos
    Search {
        /// Search query terms
        query: String,
        /// Target platform: youtube, soundcloud
        #[arg(short, long, default_value = "youtube")]
        platform: String,
    },
    /// Multi-threaded BitTorrent download engine for magnet links and .torrent files
    Torrent {
        /// Magnet URI, HTTP(S) torrent URL, or local .torrent file path
        source: String,
        /// Destination directory
        #[arg(short, long, default_value = ".")]
        output_dir: String,
        /// Peer connection concurrency limit
        #[arg(short, long, default_value_t = 8)]
        concurrency: usize,
        /// Inspect torrent metadata without starting download
        #[arg(long)]
        info_only: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = EngineConfig::default();
    let engine = MmDlpEngine::new(config)?;

    match cli.command {
        Some(Commands::Info { url }) => {
            handle_info(&engine, &url).await?;
        }
        Some(Commands::Download {
            url,
            output_dir,
            format,
            quality,
        }) => {
            handle_download(&engine, url, output_dir, format, quality).await?;
        }
        Some(Commands::Search { query, platform }) => {
            handle_search(&engine, &platform, &query).await?;
        }
        Some(Commands::Torrent {
            source,
            output_dir,
            concurrency,
            info_only,
        }) => {
            handle_torrent(&engine, source, output_dir, concurrency, info_only).await?;
        }
        None => {
            if let Some(url) = cli.url {
                if url.starts_with("magnet:") || url.ends_with(".torrent") {
                    handle_torrent(&engine, url, cli.output_dir, 8, false).await?;
                } else {
                    handle_download(&engine, url, cli.output_dir, cli.format, cli.quality).await?;
                }
            } else {
                eprintln!("Error: URL or subcommand required. Run `mm-dlp --help` for usage.");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn handle_info(engine: &MmDlpEngine, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    if url.starts_with("magnet:") || url.ends_with(".torrent") {
        return handle_torrent(engine, url.to_string(), ".".to_string(), 8, true).await;
    }

    println!("[mm-dlp] Inspecting media stream: {}", url);

    // Try full MediaInfo extractor first
    match engine.extract_info(url).await {
        Ok(info) => {
            println!("Title:       {}", info.title);
            if let Some(uploader) = &info.uploader {
                println!("Uploader:    {}", uploader);
            }
            if let Some(duration) = info.duration {
                println!("Duration:    {} seconds", duration);
            }
            if let Some(webpage) = &info.webpage_url {
                println!("Webpage:     {}", webpage);
            }
            if let Some(thumb) = &info.thumbnail {
                println!("Thumbnail:   {}", thumb);
            }

            println!("\nAvailable Formats ({} total):", info.formats.len());
            println!(
                "{:<12} {:<6} {:<12} {:<6} {:<12} {:<15} {:<15}",
                "FORMAT_ID", "EXT", "RESOLUTION", "FPS", "BITRATE", "VCODEC", "ACODEC"
            );
            println!("{:-<78}", "");

            for f in &info.formats {
                let res = match (f.width, f.height) {
                    (Some(w), Some(h)) => format!("{}x{}", w, h),
                    _ => {
                        if f.is_audio_only() {
                            "audio only".into()
                        } else {
                            "-".into()
                        }
                    }
                };
                let fps_str = f
                    .fps
                    .map(|fps| format!("{:.0}", fps))
                    .unwrap_or_else(|| "-".into());
                let tbr_str = f
                    .tbr
                    .map(|tbr| format!("{:.1}k", tbr))
                    .unwrap_or_else(|| "-".into());
                let vcodec = f.vcodec.as_deref().unwrap_or("none");
                let acodec = f.acodec.as_deref().unwrap_or("none");

                println!(
                    "{:<12} {:<6} {:<12} {:<6} {:<12} {:<15} {:<15}",
                    f.format_id, f.ext, res, fps_str, tbr_str, vcodec, acodec
                );
            }
        }
        Err(_) => {
            // Fall back to PlatformMetadata
            let meta = engine.extract_metadata(url.to_string()).await?;
            println!("Title:       {}", meta.title);
            println!("Artist:      {}", meta.artist);
            println!("Duration:    {} seconds", meta.duration_seconds);
            println!("Webpage:     {}", meta.webpage_url);
            if let Some(thumb) = &meta.thumbnail_url {
                println!("Thumbnail:   {}", thumb);
            }
        }
    }

    Ok(())
}

async fn handle_download(
    engine: &MmDlpEngine,
    url: String,
    output_dir: String,
    audio_format: String,
    quality: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let download_config = DownloadConfig {
        output_dir,
        filename_template: "%(title)s.%(ext)s".into(),
        extract_audio: true,
        audio_format,
        quality,
    };

    let callback = Box::new(CliProgressCallback::new());
    let task_id = format!(
        "cli_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );

    let _result = engine
        .execute_download(task_id, url, download_config, callback)
        .await?;

    Ok(())
}

async fn handle_search(
    engine: &MmDlpEngine,
    platform: &str,
    query: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[mm-dlp] Searching {} for '{}'...", platform, query);
    let results = engine.search(platform, query).await?;

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    println!("\nFound {} results:", results.len());
    for (i, track) in results.iter().enumerate() {
        println!(
            "{}. [{}] {} - {} ({})",
            i + 1,
            track.id,
            track.artist,
            track.title,
            track.webpage_url
        );
    }

    Ok(())
}

async fn handle_torrent(
    engine: &MmDlpEngine,
    source: String,
    output_dir: String,
    concurrency: usize,
    info_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[mm-dlp] BitTorrent Engine: {}", source);

    if info_only {
        let meta = engine.get_torrent_metadata(source).await?;
        println!("Name:        {}", meta.name);
        println!("Info Hash:   {}", meta.info_hash);
        let mb = meta.total_bytes as f64 / 1_048_576.0;
        println!("Total Size:  {} bytes ({:.2} MiB)", meta.total_bytes, mb);
        println!("Files ({} total):", meta.files.len());
        for f in &meta.files {
            println!("  [{:>3}] {:<50} {:>10} bytes", f.index, f.path, f.length);
        }
        return Ok(());
    }

    let callback = Box::new(CliProgressCallback::new());
    let task_id = format!(
        "torrent_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );

    let result = engine
        .download_torrent(task_id, source, output_dir, concurrency as u32, callback)
        .await?;

    println!("\n[mm-dlp] BitTorrent download finished successfully!");
    println!("Saved to: {}", result.file_path);
    println!(
        "Total:    {} bytes in {:.2}s",
        result.total_bytes,
        result.elapsed_millis as f64 / 1000.0
    );

    Ok(())
}
