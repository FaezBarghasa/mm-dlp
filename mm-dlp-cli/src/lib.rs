use clap::{Parser, Subcommand};
use mm_dlp_core::config::{DownloadResult, EngineConfig};
use mm_dlp_core::engine::MediaDownloadOptions;
use mm_dlp_core::error::EngineError;
use mm_dlp_core::extractor::traits::MediaInfo;
use mm_dlp_core::{DownloadProgressCallback, MmDlpEngine};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct CliProgressCallback {
    has_completed: AtomicBool,
    is_stdout_stream: bool,
    newline: bool,
}

impl CliProgressCallback {
    fn new(is_stdout_stream: bool, newline: bool) -> Self {
        Self {
            has_completed: AtomicBool::new(false),
            is_stdout_stream,
            newline,
        }
    }
}

impl DownloadProgressCallback for CliProgressCallback {
    fn on_status(&self, status: String) {
        if self.is_stdout_stream {
            eprintln!("[download] {}", status);
        } else {
            println!("[download] {}", status);
        }
    }

    fn on_progress(&self, progress_percentage: f64) {
        if self.newline {
            if self.is_stdout_stream {
                eprintln!("[download] {:>5.1}%", progress_percentage);
            } else {
                println!("[download] {:>5.1}%", progress_percentage);
            }
        } else if self.is_stdout_stream {
            eprint!("\r[download] {:>5.1}%", progress_percentage);
            let _ = std::io::stderr().flush();
        } else {
            print!("\r[download] {:>5.1}%", progress_percentage);
            let _ = std::io::stdout().flush();
        }
    }

    fn on_error(&self, error: EngineError) {
        eprintln!("\n[error] Download failed: {}", error);
    }

    fn on_complete(&self, result: DownloadResult) {
        self.has_completed.store(true, Ordering::SeqCst);
        if self.is_stdout_stream {
            eprintln!(
                "\n[complete] 100% of {} in {:.2}s (at {})",
                format_file_size(result.total_bytes),
                result.elapsed_millis as f64 / 1000.0,
                result.file_path
            );
        } else {
            println!(
                "\n[complete] 100% of {} in {:.2}s (at {})",
                format_file_size(result.total_bytes),
                result.elapsed_millis as f64 / 1000.0,
                result.file_path
            );
        }
    }
}

/// High-performance drop-in replacement for yt-dlp & multi-threaded BitTorrent client
#[derive(Parser, Debug)]
#[command(
    name = "mm-dlp",
    author,
    version,
    about = "High-performance drop-in replacement for yt-dlp with Tokio async engine"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Target URLs to download or inspect
    #[arg(value_name = "URL")]
    pub urls: Vec<String>,

    /// List all available formats for the given media URL(s)
    #[arg(short = 'F', long = "list-formats")]
    pub list_formats: bool,

    /// Video/audio format code or selector expression (e.g. "bestvideo+bestaudio/best", "ba", "137+140")
    #[arg(
        short = 'f',
        long = "format",
        default_value = "bestvideo+bestaudio/best"
    )]
    pub format: String,

    /// Output filename template (e.g. "%(title)s [%(id)s].%(ext)s")
    #[arg(
        short = 'o',
        long = "output",
        default_value = "%(title)s [%(id)s].%(ext)s"
    )]
    pub output: String,

    /// Output directory path
    #[arg(short = 'P', long = "paths", default_value = ".")]
    pub paths: String,

    /// Dump JSON metadata for each video to stdout (single line per video)
    #[arg(short = 'j', long = "dump-json")]
    pub dump_json: bool,

    /// Dump single JSON metadata tree
    #[arg(short = 'J', long = "dump-single-json")]
    pub dump_single_json: bool,

    /// Print specific metadata field (e.g. "title", "id", "duration", "uploader")
    #[arg(long = "print")]
    pub print_field: Option<String>,

    /// Convert video files to audio-only files
    #[arg(short = 'x', long = "extract-audio")]
    pub extract_audio: bool,

    /// Target audio format (best, mp3, aac, flac, m4a, opus, vorbis, wav)
    #[arg(long = "audio-format", default_value = "mp3")]
    pub audio_format: String,

    /// Audio quality (0-9 for VBR, or bitrate e.g. 192k, 320k)
    #[arg(long = "audio-quality", default_value = "best")]
    pub audio_quality: String,

    /// Number of fragments/chunks to download simultaneously
    #[arg(short = 'N', long = "concurrent-fragments", default_value_t = 8)]
    pub concurrent_fragments: usize,

    /// Write thumbnail image to file
    #[arg(long = "write-thumbnail")]
    pub write_thumbnail: bool,

    /// Embed thumbnail into media file
    #[arg(long = "embed-thumbnail")]
    pub embed_thumbnail: bool,

    /// Embed metadata into media file
    #[arg(long = "add-metadata", alias = "embed-metadata")]
    pub embed_metadata: bool,

    /// Netscape cookies file path
    #[arg(long = "cookies")]
    pub cookies: Option<String>,

    /// Do not download playlist, only the single video
    #[arg(long = "no-playlist")]
    pub no_playlist: bool,

    /// Download entire playlist
    #[arg(long = "yes-playlist")]
    pub yes_playlist: bool,

    /// Write media stream directly to stdout without intermediate files (alias for -o -)
    #[arg(long = "stdout")]
    pub stdout: bool,

    /// Apply EBU R128 real-time loudness normalization filter
    #[arg(long = "loudness-norm")]
    pub loudness_norm: bool,

    /// Print various debugging information
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Activate quiet mode (suppress non-error output)
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Suppress warning messages
    #[arg(long = "no-warnings")]
    pub no_warnings: bool,

    /// Continue on download errors (do not exit immediately)
    #[arg(short = 'i', long = "ignore-errors")]
    pub ignore_errors: bool,

    /// Use the specified HTTP/HTTPS/SOCKS5 proxy
    #[arg(long = "proxy")]
    pub proxy: Option<String>,

    /// Load cookies from browser (e.g. chrome, firefox, brave, edge, safari)
    #[arg(long = "cookies-from-browser")]
    pub cookies_from_browser: Option<String>,

    /// Custom User-Agent header
    #[arg(long = "user-agent")]
    pub user_agent: Option<String>,

    /// Custom Referer header
    #[arg(long = "referer")]
    pub referer: Option<String>,

    /// Read URLs to download from a batch file (one URL per line)
    #[arg(short = 'a', long = "batch-file")]
    pub batch_file: Option<String>,

    /// Keep intermediate video file on disk after post-processing
    #[arg(short = 'k', long = "keep-video")]
    pub keep_video: bool,

    /// Do not overwrite existing output files
    #[arg(short = 'w', long = "no-overwrites")]
    pub no_overwrites: bool,

    /// Write subtitle file
    #[arg(long = "write-sub", alias = "write-subs")]
    pub write_subs: bool,

    /// Write automatically generated subtitle file
    #[arg(long = "write-auto-sub", alias = "write-auto-subs")]
    pub write_auto_subs: bool,

    /// Languages of the subtitles to download (comma-separated or regex)
    #[arg(long = "sub-langs", alias = "sub-lang")]
    pub sub_langs: Option<String>,

    /// Embed subtitles into media file
    #[arg(long = "embed-subs", alias = "embed-sub")]
    pub embed_subs: bool,

    /// Impersonate browser (e.g. "firefox", "gecko", "chrome")
    #[arg(long = "impersonate")]
    pub impersonate: Option<String>,

    /// Custom DNS server(s) (comma-separated, e.g. "1.1.1.1,9.9.9.9" or "8.8.8.8"). Defaults to 1.1.1.1 with fallback to 9.9.9.9.
    #[arg(long = "dns", alias = "dns-servers")]
    pub dns: Option<String>,

    /// Streaming buffer size in bytes or unit (e.g. "100M", "512M", "2G", "max"). Defaults to 100M on mobile, 512M on desktop.
    #[arg(long = "buffer-size", default_value = "default")]
    pub buffer_size: String,

    /// Simulate, quiet but print direct media URL(s)
    #[arg(short = 'g', long = "get-url")]
    pub get_url: bool,

    /// Simulate, quiet but print title
    #[arg(short = 'e', long = "get-title")]
    pub get_title: bool,

    /// Simulate, quiet but print id
    #[arg(long = "get-id")]
    pub get_id: bool,

    /// Simulate, quiet but print thumbnail URL
    #[arg(long = "get-thumbnail")]
    pub get_thumbnail: bool,

    /// Simulate, quiet but print duration (in seconds)
    #[arg(long = "get-duration")]
    pub get_duration: bool,

    /// Simulate, quiet but print output filename
    #[arg(long = "get-filename")]
    pub get_filename: bool,

    /// Output progress bar as new lines (ideal for GUI frontends like Tartube, OpenVideoDownloader)
    #[arg(long = "newline")]
    pub newline: bool,

    /// Do not download the video but write all related files (alias for --no-download)
    #[arg(long = "skip-download", alias = "no-download")]
    pub skip_download: bool,

    /// Suppress HTTPS certificate validation
    #[arg(long = "no-check-certificates", alias = "no-check-certificate")]
    pub no_check_certificates: bool,

    /// Do not use the Last-modified header to set the file modification time
    #[arg(long = "no-mtime")]
    pub no_mtime: bool,

    /// Prefer free video/audio formats (webm, opus)
    #[arg(long = "prefer-free-formats")]
    pub prefer_free_formats: bool,

    /// Abort downloading of further videos if an error occurs
    #[arg(long = "abort-on-error")]
    pub abort_on_error: bool,

    /// Specify a custom HTTP header and its value, separated by a colon ':'
    #[arg(
        long = "add-header",
        value_name = "FIELD:VALUE",
        action = clap::ArgAction::Append
    )]
    pub add_header: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Extract and display all available formats and metadata for a given URL without downloading
    Info {
        /// Target URL (YouTube, Vimeo, SoundCloud, Spotify, Generic)
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

pub async fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut config = EngineConfig::default();
    if let Some(ref ua) = cli.user_agent {
        config.user_agent = ua.clone();
    }
    config.proxy = cli.proxy.clone();
    config.cookies_file = cli.cookies.clone();
    config.cookies_from_browser = cli.cookies_from_browser.clone();
    config.impersonate_target = cli.impersonate.clone();
    config.dns_servers = cli.dns.clone();
    config.dangerously_insecure = cli.no_check_certificates;
    if !cli.add_header.is_empty() {
        config.custom_headers = Some(cli.add_header.clone());
    }

    if cli.verbose {
        if let Some(ref b) = cli.cookies_from_browser {
            eprintln!("[debug] Cookies from browser requested: {}", b);
        }
        if let Some(ref imp) = cli.impersonate {
            eprintln!("[debug] Browser impersonation active: {}", imp);
        }
        if let Some(ref d) = cli.dns {
            eprintln!("[debug] Custom DNS servers configured: {}", d);
        } else {
            eprintln!("[debug] DNS resolver configured with default 1.1.1.1 (primary) and 9.9.9.9 (fallback)");
        }
    }

    let buffer_size_bytes =
        match mm_dlp_core::downloader::flusher::parse_buffer_size(&cli.buffer_size) {
            Ok(sz) => sz,
            Err(e) => {
                eprintln!("[error] {}", e);
                if !cli.ignore_errors {
                    std::process::exit(1);
                }
                mm_dlp_core::downloader::flusher::default_streaming_buffer_size()
            }
        };
    config.buffer_size_bytes = Some(buffer_size_bytes as u64);

    if cli.verbose {
        eprintln!(
            "[debug] Streaming buffer size configured: {} ({:.1} MiB)",
            cli.buffer_size,
            buffer_size_bytes as f64 / (1024.0 * 1024.0)
        );
    }

    let engine = MmDlpEngine::new(config)?;

    // Handle explicit subcommands first
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Info { url } => {
                handle_info(&engine, &url).await?;
                return Ok(());
            }
            Commands::Download {
                url,
                output_dir,
                format,
                quality,
            } => {
                handle_legacy_download(&engine, url, output_dir, format, quality).await?;
                return Ok(());
            }
            Commands::Search { query, platform } => {
                handle_search(&engine, &platform, &query).await?;
                return Ok(());
            }
            Commands::Torrent {
                source,
                output_dir,
                concurrency,
                info_only,
            } => {
                handle_torrent(&engine, source, output_dir, concurrency, info_only).await?;
                return Ok(());
            }
        }
    }

    // Collect URLs from positional args and batch file
    let mut target_urls = cli.urls.clone();
    if let Some(ref batch_path) = cli.batch_file {
        match std::fs::read_to_string(batch_path) {
            Ok(content) => {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        target_urls.push(trimmed.to_string());
                    }
                }
            }
            Err(e) => {
                eprintln!("[error] Failed to read batch file {}: {}", batch_path, e);
                if !cli.ignore_errors {
                    std::process::exit(1);
                }
            }
        }
    }

    if target_urls.is_empty() {
        eprintln!("Usage: mm-dlp [OPTIONS] <URL>...\nFor more information, try '--help'.");
        std::process::exit(1);
    }

    let mut has_failure = false;

    for url in &target_urls {
        // 1. BitTorrent auto-routing
        if url.starts_with("magnet:") || url.ends_with(".torrent") {
            if cli.list_formats {
                handle_torrent(&engine, url.clone(), cli.paths.clone(), 8, true).await?;
            } else {
                handle_torrent(
                    &engine,
                    url.clone(),
                    cli.paths.clone(),
                    cli.concurrent_fragments,
                    false,
                )
                .await?;
            }
            continue;
        }

        // 2. Fetch MediaInfo
        let info = match engine.extract_info(url).await {
            Ok(info) => info,
            Err(e) => {
                eprintln!("[error] {}: {}", url, e);
                has_failure = true;
                if cli.abort_on_error || !cli.ignore_errors {
                    std::process::exit(1);
                }
                continue;
            }
        };

        let effective_format = if cli.extract_audio && cli.format == "bestvideo+bestaudio/best" {
            "bestaudio/best".to_string()
        } else if cli.prefer_free_formats && cli.format == "bestvideo+bestaudio/best" {
            "bestvideo[ext=webm]+bestaudio[ext=opus]/best[ext=webm]/best".to_string()
        } else {
            cli.format.clone()
        };

        // Handle simulation queries (-e, --get-id, --get-thumbnail, --get-duration, --get-filename, -g)
        let is_simulation = cli.get_title
            || cli.get_id
            || cli.get_thumbnail
            || cli.get_duration
            || cli.get_filename
            || cli.get_url;

        if is_simulation {
            if cli.get_title {
                println!("{}", info.title);
            }
            if cli.get_id {
                println!("{}", info.id);
            }
            if cli.get_thumbnail {
                if let Some(ref thumb) = info.thumbnail {
                    println!("{}", thumb);
                }
            }
            if cli.get_duration {
                if let Some(dur) = info.duration {
                    println!("{}", dur);
                }
            }
            if cli.get_filename {
                let sel = mm_dlp_core::extractor::format_selector::FormatSelector::select(
                    &info,
                    &effective_format,
                );
                let ext_str: String;
                let ext = if cli.extract_audio {
                    if cli.audio_format == "best" {
                        match &sel {
                            Ok(
                                mm_dlp_core::extractor::format_selector::SelectedFormat::Merged {
                                    audio,
                                    ..
                                },
                            ) => audio.ext.as_str(),
                            Ok(
                                mm_dlp_core::extractor::format_selector::SelectedFormat::Single(
                                    fmt,
                                ),
                            ) => {
                                if fmt.is_audio_only() {
                                    fmt.ext.as_str()
                                } else {
                                    "mp3"
                                }
                            }
                            Err(_) => "mp3",
                        }
                    } else {
                        &cli.audio_format
                    }
                } else {
                    match &sel {
                        Ok(s) => {
                            ext_str = s.primary_ext().to_string();
                            &ext_str
                        }
                        Err(_) => "mp4",
                    }
                };
                let (h, w, fps, fid) = match &sel {
                    Ok(mm_dlp_core::extractor::format_selector::SelectedFormat::Merged {
                        video,
                        ..
                    }) => (
                        video.height,
                        video.width,
                        video.fps,
                        Some(video.format_id.as_str()),
                    ),
                    Ok(mm_dlp_core::extractor::format_selector::SelectedFormat::Single(fmt)) => {
                        (fmt.height, fmt.width, fmt.fps, Some(fmt.format_id.as_str()))
                    }
                    Err(_) => (None, None, None, None),
                };
                let ctx = mm_dlp_core::utils::TemplateContext {
                    id: &info.id,
                    title: &info.title,
                    uploader: info.uploader.as_deref(),
                    ext,
                    height: h,
                    width: w,
                    fps,
                    format_id: fid,
                };
                let filename =
                    mm_dlp_core::utils::expand_output_template_with_meta(&cli.output, &ctx);
                println!("{}", filename);
            }
            if cli.get_url {
                match mm_dlp_core::extractor::format_selector::FormatSelector::select(
                    &info,
                    &effective_format,
                ) {
                    Ok(mm_dlp_core::extractor::format_selector::SelectedFormat::Single(fmt)) => {
                        println!("{}", fmt.url);
                    }
                    Ok(mm_dlp_core::extractor::format_selector::SelectedFormat::Merged {
                        video,
                        audio,
                    }) => {
                        println!("{}", video.url);
                        println!("{}", audio.url);
                    }
                    Err(_) => {
                        if let Some(first) = info.formats.first() {
                            println!("{}", first.url);
                        }
                    }
                }
            }
            continue;
        }

        // 3. Handle -F / --list-formats
        if cli.list_formats {
            print_formats_table(&info);
            continue;
        }

        // 4. Handle -j / --dump-json
        if cli.dump_json {
            let json_str = serde_json::to_string(&info)?;
            println!("{}", json_str);
            continue;
        }

        // 5. Handle -J / --dump-single-json
        if cli.dump_single_json {
            let json_str = serde_json::to_string_pretty(&info)?;
            println!("{}", json_str);
            continue;
        }

        // 6. Handle --print
        if let Some(ref field) = cli.print_field {
            match field.as_str() {
                "title" => println!("{}", info.title),
                "id" => println!("{}", info.id),
                "duration" => println!("{}", info.duration.unwrap_or(0)),
                "uploader" => println!("{}", info.uploader.as_deref().unwrap_or("")),
                "thumbnail" => println!("{}", info.thumbnail.as_deref().unwrap_or("")),
                "webpage_url" => println!("{}", info.webpage_url.as_deref().unwrap_or("")),
                "urls" => {
                    for f in &info.formats {
                        println!("{}", f.url);
                    }
                }
                _ => println!("{}", info.title),
            }
            continue;
        }

        // 7. Handle --skip-download
        if cli.skip_download {
            if !cli.quiet {
                println!("[info] Skipping download for {}", info.id);
            }
            continue;
        }

        // 8. Full download path
        let is_stdout = cli.output == "-" || cli.paths == "-" || cli.stdout;
        let effective_output = if is_stdout {
            "-".to_string()
        } else {
            cli.output.clone()
        };

        let options = MediaDownloadOptions {
            format_selector: effective_format,
            output_dir: if is_stdout {
                "-".to_string()
            } else {
                cli.paths.clone()
            },
            output_template: effective_output,
            concurrent_fragments: cli.concurrent_fragments,
            extract_audio: cli.extract_audio,
            audio_format: cli.audio_format.clone(),
            audio_quality: cli.audio_quality.clone(),
            write_thumbnail: cli.write_thumbnail && !is_stdout,
            embed_thumbnail: cli.embed_thumbnail && !is_stdout,
            embed_metadata: cli.embed_metadata && !is_stdout,
            loudness_norm: cli.loudness_norm,
            buffer_size_bytes: Some(buffer_size_bytes),
        };

        let callback: Arc<dyn DownloadProgressCallback> =
            Arc::new(CliProgressCallback::new(is_stdout, cli.newline));
        let task_id = format!(
            "dl_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
        );

        if is_stdout {
            eprintln!("[{}] {}: Streaming to stdout", info.id, info.title);
        } else if !cli.quiet {
            println!("[{}] {}: Downloading", info.id, info.title);
        }

        if let Err(e) = engine
            .download_media(task_id, url.clone(), options, callback)
            .await
        {
            eprintln!("[error] Download failed for {}: {}", url, e);
            has_failure = true;
            if cli.abort_on_error || !cli.ignore_errors {
                std::process::exit(1);
            }
        }
    }

    if has_failure && !cli.ignore_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn print_formats_table(info: &MediaInfo) {
    println!("[info] Available formats for {}:", info.id);
    println!(
        "{:<10} {:<6} {:<12} {:<5} │ {:<10} {:<8} {:<10} │ {:<15} {:<15}",
        "ID", "EXT", "RESOLUTION", "FPS", "FILESIZE", "TBR", "PROTO", "VCODEC", "ACODEC"
    );
    println!("{:-<95}", "");

    for f in &info.formats {
        let res = match (f.width, f.height) {
            (Some(w), Some(h)) => format!("{}x{}", w, h),
            _ => {
                if f.is_audio_only() {
                    "audio only".into()
                } else {
                    "unknown".into()
                }
            }
        };

        let fps = f
            .fps
            .map(|v| format!("{:.0}", v))
            .unwrap_or_else(|| "-".into());

        let size = f
            .filesize
            .map(format_file_size)
            .unwrap_or_else(|| "~NA".into());

        let tbr = f
            .tbr
            .map(|t| format!("{:.0}k", t))
            .unwrap_or_else(|| "-".into());

        let proto = f.protocol.as_deref().unwrap_or("https");
        let vcodec = f.vcodec.as_deref().unwrap_or("none");
        let acodec = f.acodec.as_deref().unwrap_or("none");

        println!(
            "{:<10} {:<6} {:<12} {:<5} │ {:<10} {:<8} {:<10} │ {:<15} {:<15}",
            f.format_id, f.ext, res, fps, size, tbr, proto, vcodec, acodec
        );
    }
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2}GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2}MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2}KiB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

async fn handle_info(engine: &MmDlpEngine, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    if url.starts_with("magnet:") || url.ends_with(".torrent") {
        return handle_torrent(engine, url.to_string(), ".".to_string(), 8, true).await;
    }

    let info = engine.extract_info(url).await?;
    println!("Title:       {}", info.title);
    if let Some(ref uploader) = info.uploader {
        println!("Uploader:    {}", uploader);
    }
    if let Some(dur) = info.duration {
        println!("Duration:    {} seconds", dur);
    }
    if let Some(ref webpage) = info.webpage_url {
        println!("Webpage:     {}", webpage);
    }
    if let Some(ref thumb) = info.thumbnail {
        println!("Thumbnail:   {}", thumb);
    }

    println!();
    print_formats_table(&info);
    Ok(())
}

async fn handle_legacy_download(
    engine: &MmDlpEngine,
    url: String,
    output_dir: String,
    audio_format: String,
    quality: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = MediaDownloadOptions {
        format_selector: "bestaudio/best".into(),
        output_dir,
        output_template: "%(title)s.%(ext)s".into(),
        concurrent_fragments: 8,
        extract_audio: true,
        audio_format,
        audio_quality: quality,
        write_thumbnail: false,
        embed_thumbnail: false,
        embed_metadata: true,
        loudness_norm: false,
        buffer_size_bytes: None,
    };

    let callback: Arc<dyn DownloadProgressCallback> =
        Arc::new(CliProgressCallback::new(false, false));
    let task_id = format!(
        "legacy_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );

    let _ = engine
        .download_media(task_id, url, options, callback)
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

    let callback = Box::new(CliProgressCallback::new(false, false));
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
