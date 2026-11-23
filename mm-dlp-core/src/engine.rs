use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config::{DownloadConfig, DownloadResult, EngineConfig, MediaMetadata};
pub use crate::downloader::dash::DashDownloader;
pub use crate::downloader::hls::HlsDownloader;
use crate::downloader::parallel::SequencedChunk;
pub use crate::downloader::torrent::{TorrentDownloader, TorrentFileEntry, TorrentMetadata};
use crate::downloader::{build_http_client_with_config, fetch_with_fallback, stream_to_file};
use crate::error::EngineError;
use crate::extractor::PlatformRegistry;
use crate::extractor::format_selector::{FormatSelector, SelectedFormat};
use crate::extractor::traits::MediaFormat;
use crate::processor::{
    download_cover_art, extract_audio_from_file, mux_streams_piped_with_buffer,
    mux_streams_to_file, tag_file_in_place,
};
use crate::resolver::SourceResolver;
use crate::utils::{
    expand_output_template_with_meta, resolve_duplicate_path, sanitize_filename, sanitize_url,
    validate_output_path,
};

#[derive(Debug, Clone)]
pub struct MediaDownloadOptions {
    pub format_selector: String,
    pub output_dir: String,
    pub output_template: String,
    pub concurrent_fragments: usize,
    pub extract_audio: bool,
    pub audio_format: String,
    pub audio_quality: String,
    pub write_thumbnail: bool,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub loudness_norm: bool,
    pub buffer_size_bytes: Option<usize>,
}

impl Default for MediaDownloadOptions {
    fn default() -> Self {
        Self {
            format_selector: "bestvideo+bestaudio/best".to_string(),
            output_dir: ".".to_string(),
            output_template: "%(title)s [%(id)s].%(ext)s".to_string(),
            concurrent_fragments: 8,
            extract_audio: false,
            audio_format: "mp3".to_string(),
            audio_quality: "best".to_string(),
            write_thumbnail: false,
            embed_thumbnail: false,
            embed_metadata: true,
            loudness_norm: false,
            buffer_size_bytes: None,
        }
    }
}

pub trait DownloadProgressCallback: Send + Sync {
    fn on_status(&self, status: String);
    fn on_progress(&self, progress_percentage: f64);
    fn on_error(&self, error: EngineError);
    fn on_complete(&self, result: DownloadResult);
}

pub struct MmDlpEngine {
    config: EngineConfig,
    client: Client,
    registry: Arc<PlatformRegistry>,
    resolver: SourceResolver,
    active_tasks: RwLock<HashMap<String, CancellationToken>>,
}

impl MmDlpEngine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let client = build_http_client_with_config(&config)?;
        let registry = Arc::new(PlatformRegistry::new());
        let resolver = SourceResolver::new(registry.clone());
        Ok(Self {
            config,
            client,
            registry,
            resolver,
            active_tasks: RwLock::new(HashMap::new()),
        })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub async fn extract_metadata(&self, raw_url: String) -> Result<MediaMetadata, EngineError> {
        let clean_url = sanitize_url(&raw_url)?;
        let extractor = self.registry.route_url(&clean_url).ok_or_else(|| {
            EngineError::UnsupportedUrl(format!("No extractor for URL: {}", clean_url))
        })?;
        extractor.extract_metadata(&self.client, &clean_url).await
    }

    pub async fn get_torrent_metadata(
        &self,
        source: String,
    ) -> Result<TorrentMetadata, EngineError> {
        let session_dir = std::env::temp_dir().join("mm-dlp-torrents");
        let downloader = TorrentDownloader::new(session_dir)?;
        downloader.get_metadata(&source).await
    }

    pub async fn download_torrent(
        &self,
        task_id: String,
        source: String,
        output_dir: String,
        concurrency: u32,
        callback: Box<dyn DownloadProgressCallback>,
    ) -> Result<DownloadResult, EngineError> {
        let callback: Arc<dyn DownloadProgressCallback> = Arc::from(callback);
        let cancel_token = CancellationToken::new();
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let out_path = validate_output_path(&output_dir)?;
        let session_dir = out_path.join(".mm-dlp-session");
        let downloader = TorrentDownloader::new(session_dir)?;

        let result = downloader
            .download(
                task_id.clone(),
                &source,
                &out_path,
                concurrency as usize,
                callback.clone(),
                cancel_token,
            )
            .await;

        {
            let mut tasks = self.active_tasks.write().await;
            tasks.remove(&task_id);
        }

        if let Err(ref err) = result {
            callback.on_error(err.clone());
        }

        result
    }

    pub async fn extract_info(
        &self,
        raw_url: &str,
    ) -> Result<crate::extractor::traits::MediaInfo, EngineError> {
        let clean_url = sanitize_url(raw_url)?;
        let extractor = crate::extractor::registry::route_url_with_fallback(&clean_url)
            .ok_or_else(|| {
                EngineError::UnsupportedUrl(format!("No extractor for URL: {}", clean_url))
            })?;
        extractor.extract_metadata(&self.client, &clean_url).await
    }

    pub async fn search(
        &self,
        platform: &str,
        query: &str,
    ) -> Result<Vec<MediaMetadata>, EngineError> {
        let extractor = self.registry.get_extractor(platform).ok_or_else(|| {
            EngineError::UnsupportedUrl(format!("Unsupported platform: {}", platform))
        })?;
        extractor.search(&self.client, query).await
    }

    pub fn registry(&self) -> Arc<PlatformRegistry> {
        self.registry.clone()
    }

    pub async fn execute_download(
        &self,
        task_id: String,
        raw_url: String,
        download_config: DownloadConfig,
        callback: Box<dyn DownloadProgressCallback>,
    ) -> Result<DownloadResult, EngineError> {
        let start_time = Instant::now();
        let cancel_token = CancellationToken::new();
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let callback: Arc<dyn DownloadProgressCallback> = Arc::from(callback);
        let callback_run = callback.clone();

        let run_download = async {
            callback_run.on_status("Validating parameters".to_string());
            let clean_url = sanitize_url(&raw_url)?;
            let output_dir = validate_output_path(&download_config.output_dir)?;

            if clean_url.starts_with("magnet:") || clean_url.ends_with(".torrent") {
                let session_dir = output_dir.join(".mm-dlp-session");
                let downloader = TorrentDownloader::new(session_dir)?;
                return downloader
                    .download(
                        task_id.clone(),
                        &clean_url,
                        &output_dir,
                        8,
                        callback_run,
                        cancel_token.clone(),
                    )
                    .await;
            }

            callback_run.on_status("Extracting metadata".to_string());
            let extractor = self.registry.route_url(&clean_url).ok_or_else(|| {
                EngineError::UnsupportedUrl(format!("No extractor for URL: {}", clean_url))
            })?;

            let metadata = extractor.extract_metadata(&self.client, &clean_url).await?;

            callback_run.on_status("Resolving audio stream".to_string());
            let candidate = if extractor
                .get_stream_url(&self.client, &metadata.id)
                .await
                .is_ok()
            {
                extractor.get_stream_url(&self.client, &metadata.id).await?
            } else {
                self.resolver
                    .resolve_best_stream(&self.client, &metadata)
                    .await?
            };

            callback_run.on_status("Fetching audio stream".to_string());
            let response = fetch_with_fallback(&self.client, &candidate.url).await?;

            let clean_title = sanitize_filename(&metadata.title);
            let target_filename = format!("{}.{}", clean_title, download_config.audio_format);
            let raw_target_path = output_dir.join(target_filename);
            let final_target_path = resolve_duplicate_path(&raw_target_path);

            callback_run.on_status("Downloading stream".to_string());
            let downloaded_bytes =
                stream_to_file(response, &final_target_path, cancel_token.clone()).await?;
            callback_run.on_progress(100.0);

            if let Some(art_url) = &metadata.thumbnail_url {
                callback_run.on_status("Fetching cover art".to_string());
                let cover_art = download_cover_art(&self.client, art_url).await;
                callback_run.on_status("Embedding metadata tags".to_string());
                let _ = tag_file_in_place(&final_target_path, &metadata, cover_art);
            }

            let elapsed_millis = start_time.elapsed().as_millis() as u64;
            let result = DownloadResult {
                file_path: final_target_path.to_string_lossy().to_string(),
                total_bytes: downloaded_bytes,
                elapsed_millis,
            };

            callback_run.on_complete(result.clone());
            Ok(result)
        };

        let result: Result<DownloadResult, EngineError> = run_download.await;
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.remove(&task_id);
        }

        if let Err(ref err) = result {
            callback.on_error(err.clone());
        }

        result
    }

    pub async fn cancel_download(&self, task_id: &str) -> bool {
        let tasks = self.active_tasks.read().await;
        if let Some(token) = tasks.get(task_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn download_media(
        &self,
        task_id: String,
        raw_url: String,
        options: MediaDownloadOptions,
        callback: Arc<dyn DownloadProgressCallback>,
    ) -> Result<DownloadResult, EngineError> {
        let start_time = Instant::now();
        let cancel_token = CancellationToken::new();
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let callback_run = callback.clone();
        let run_dl = async {
            let clean_url = sanitize_url(&raw_url)?;
            let is_stdout = options.output_template == "-" || options.output_dir == "-";

            let output_dir_path = if is_stdout {
                std::env::temp_dir()
            } else {
                validate_output_path(&options.output_dir)?
            };

            if clean_url.starts_with("magnet:") || clean_url.ends_with(".torrent") {
                let session_dir = output_dir_path.join(".mm-dlp-session");
                let downloader = TorrentDownloader::new(session_dir)?;
                return downloader
                    .download(
                        task_id.clone(),
                        &clean_url,
                        &output_dir_path,
                        options.concurrent_fragments,
                        callback_run,
                        cancel_token.clone(),
                    )
                    .await;
            }

            callback_run.on_status(format!("Extracting metadata from {}", clean_url));
            let info = self.extract_info(&clean_url).await?;

            callback_run.on_status(format!("Resolving format for '{}'", info.title));
            let selected = FormatSelector::select(&info, &options.format_selector)?;

            let ext_str: String;
            let ext: &str = if options.extract_audio {
                if options.audio_format == "best" {
                    match &selected {
                        SelectedFormat::Merged { audio, .. } => audio.ext.as_str(),
                        SelectedFormat::Single(fmt) => {
                            if fmt.is_audio_only() {
                                fmt.ext.as_str()
                            } else {
                                "mp3"
                            }
                        }
                    }
                } else {
                    &options.audio_format
                }
            } else {
                ext_str = selected.primary_ext().to_string();
                &ext_str
            };

            let final_target_path = if is_stdout {
                Path::new("-").to_path_buf()
            } else {
                let (h, w, fps, fid) = match &selected {
                    SelectedFormat::Merged { video, .. } => (
                        video.height,
                        video.width,
                        video.fps,
                        Some(video.format_id.as_str()),
                    ),
                    SelectedFormat::Single(fmt) => {
                        (fmt.height, fmt.width, fmt.fps, Some(fmt.format_id.as_str()))
                    }
                };
                let ctx = crate::utils::TemplateContext {
                    id: &info.id,
                    title: &info.title,
                    uploader: info.uploader.as_deref(),
                    ext,
                    height: h,
                    width: w,
                    fps,
                    format_id: fid,
                };
                let target_filename =
                    expand_output_template_with_meta(&options.output_template, &ctx);
                let raw_target_path = output_dir_path.join(&target_filename);
                resolve_duplicate_path(&raw_target_path)
            };

            let mut total_bytes = 0u64;

            match selected {
                SelectedFormat::Merged { video, audio } => {
                    if options.extract_audio {
                        // Audio extraction requested: stream ONLY audio, avoiding redundant video download
                        callback_run.on_status(format!(
                            "Extracting audio-only stream [{}] ({}) to target format {}",
                            audio.format_id, audio.ext, options.audio_format
                        ));
                        let tmp_audio =
                            output_dir_path.join(format!("{}.tmp.raw.a.{}", info.id, audio.ext));
                        let a_bytes = self
                            .download_format_stream(
                                audio,
                                &tmp_audio,
                                options.concurrent_fragments,
                                cancel_token.clone(),
                            )
                            .await?;
                        total_bytes += a_bytes;

                        if let Err(e) = extract_audio_from_file(
                            &tmp_audio,
                            &final_target_path,
                            &options.audio_format,
                            &options.audio_quality,
                            options.loudness_norm,
                        )
                        .await
                        {
                            let _ = tokio::fs::remove_file(&tmp_audio).await;
                            return Err(e);
                        }
                        let _ = tokio::fs::remove_file(&tmp_audio).await;
                    } else {
                        callback_run.on_status(format!(
                            "Piping concurrent video [{}] and audio [{}] via FFmpeg muxer",
                            video.format_id, audio.format_id
                        ));

                        // Use on-the-fly piped muxing (zero disk files for stdout, 50% less I/O for disk)
                        let (v_tx, v_rx) =
                            tokio::sync::mpsc::channel(options.concurrent_fragments.max(4));
                        let (a_tx, a_rx) =
                            tokio::sync::mpsc::channel(options.concurrent_fragments.max(4));

                        let v_fut = self.stream_format_to_channel(
                            video,
                            v_tx,
                            options.concurrent_fragments,
                            cancel_token.clone(),
                        );
                        let a_fut = self.stream_format_to_channel(
                            audio,
                            a_tx,
                            options.concurrent_fragments,
                            cancel_token.clone(),
                        );
                        let buffer_bytes = options.buffer_size_bytes.unwrap_or_else(
                            crate::downloader::flusher::default_streaming_buffer_size,
                        );
                        let mux_fut = mux_streams_piped_with_buffer(
                            v_rx,
                            a_rx,
                            &final_target_path,
                            &video.ext,
                            &audio.ext,
                            cancel_token.clone(),
                            buffer_bytes,
                        );

                        let (v_res, a_res, mux_res) = tokio::join!(v_fut, a_fut, mux_fut);

                        match mux_res {
                            Ok(bytes) => {
                                let _ = v_res;
                                let _ = a_res;
                                total_bytes += bytes;
                            }
                            Err(mux_err) => {
                                if is_stdout {
                                    return Err(mux_err);
                                }
                                // Fallback to sequential disk download & mux if piped muxing fails
                                callback_run.on_status(
                                    "Piped muxing fallback to sequential disk muxing".into(),
                                );
                                let tmp_video = output_dir_path
                                    .join(format!("{}.tmp.v.{}", info.id, video.ext));
                                let tmp_audio = output_dir_path
                                    .join(format!("{}.tmp.a.{}", info.id, audio.ext));

                                let v_bytes = self
                                    .download_format_stream(
                                        video,
                                        &tmp_video,
                                        options.concurrent_fragments,
                                        cancel_token.clone(),
                                    )
                                    .await?;
                                total_bytes += v_bytes;

                                let a_bytes = self
                                    .download_format_stream(
                                        audio,
                                        &tmp_audio,
                                        options.concurrent_fragments,
                                        cancel_token.clone(),
                                    )
                                    .await?;
                                total_bytes += a_bytes;

                                if let Err(e) =
                                    mux_streams_to_file(&tmp_video, &tmp_audio, &final_target_path)
                                        .await
                                {
                                    let _ = tokio::fs::remove_file(&tmp_video).await;
                                    let _ = tokio::fs::remove_file(&tmp_audio).await;
                                    return Err(e);
                                }

                                let _ = tokio::fs::remove_file(&tmp_video).await;
                                let _ = tokio::fs::remove_file(&tmp_audio).await;
                            }
                        }
                    }
                }
                SelectedFormat::Single(format) => {
                    callback_run.on_status(format!(
                        "Downloading single format [{}] ({})",
                        format.format_id, format.ext
                    ));

                    if options.extract_audio && format.has_video() {
                        let tmp_video =
                            output_dir_path.join(format!("{}.tmp.raw.{}", info.id, format.ext));
                        let bytes = self
                            .download_format_stream(
                                format,
                                &tmp_video,
                                options.concurrent_fragments,
                                cancel_token.clone(),
                            )
                            .await?;
                        total_bytes += bytes;

                        callback_run.on_status(format!(
                            "Extracting audio track to {}",
                            options.audio_format
                        ));
                        if let Err(e) = extract_audio_from_file(
                            &tmp_video,
                            &final_target_path,
                            &options.audio_format,
                            &options.audio_quality,
                            options.loudness_norm,
                        )
                        .await
                        {
                            let _ = tokio::fs::remove_file(&tmp_video).await;
                            return Err(e);
                        }
                        let _ = tokio::fs::remove_file(&tmp_video).await;
                    } else {
                        let bytes = self
                            .download_format_stream(
                                format,
                                &final_target_path,
                                options.concurrent_fragments,
                                cancel_token.clone(),
                            )
                            .await?;
                        total_bytes += bytes;
                    }
                }
            }

            if !is_stdout
                && options.write_thumbnail
                && let Some(ref thumb_url) = info.thumbnail
            {
                callback_run.on_status("Writing thumbnail image".into());
                let thumb_path = final_target_path.with_extension("jpg");
                if let Ok(thumb_data) = self.client.get(thumb_url).send().await
                    && let Ok(bytes) = thumb_data.bytes().await
                {
                    let _ = tokio::fs::write(&thumb_path, bytes).await;
                }
            }

            if !is_stdout
                && options.embed_metadata
                && let Some(ref art_url) = info.thumbnail
            {
                let cover_art = download_cover_art(&self.client, art_url).await;
                let legacy_meta = MediaMetadata {
                    id: info.id.clone(),
                    title: info.title.clone(),
                    artist: info.uploader.clone().unwrap_or_else(|| "Unknown".into()),
                    album: None,
                    duration_seconds: info.duration.unwrap_or(0) as u64,
                    thumbnail_url: info.thumbnail.clone(),
                    webpage_url: info.webpage_url.clone().unwrap_or_default(),
                };
                let _ = tag_file_in_place(&final_target_path, &legacy_meta, cover_art);
            }

            callback_run.on_progress(100.0);

            let elapsed_millis = start_time.elapsed().as_millis() as u64;
            let result = DownloadResult {
                file_path: final_target_path.to_string_lossy().to_string(),
                total_bytes,
                elapsed_millis,
            };
            callback_run.on_complete(result.clone());
            Ok(result)
        };

        let result: Result<DownloadResult, EngineError> = run_dl.await;
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.remove(&task_id);
        }

        if let Err(ref err) = result {
            callback.on_error(err.clone());
        }

        result
    }

    /// Stream a media format chunks directly into an async mpsc sender (for piped on-the-fly muxing).
    pub async fn stream_format_to_channel(
        &self,
        format: &MediaFormat,
        tx: tokio::sync::mpsc::Sender<Result<SequencedChunk, EngineError>>,
        concurrency: usize,
        cancel_token: CancellationToken,
    ) -> Result<(), EngineError> {
        if format.protocol.as_deref() == Some("m3u8_native") || format.url.contains(".m3u8") {
            let hls = HlsDownloader::new(self.client.clone(), concurrency);
            hls.stream_to_channel(&format.url, tx, cancel_token).await
        } else if format.protocol.as_deref() == Some("http_dash_segments")
            || format.url.contains(".mpd")
        {
            let dash = DashDownloader::new(self.client.clone(), concurrency);
            dash.stream_to_channel(&format.url, tx, cancel_token).await
        } else {
            let mut resp = fetch_with_fallback(&self.client, &format.url).await?;
            let mut idx = 0usize;
            while let Some(chunk) = resp
                .chunk()
                .await
                .map_err(|e| EngineError::Network(e.to_string()))?
            {
                if cancel_token.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }
                if tx
                    .send(Ok(SequencedChunk {
                        index: idx,
                        data: chunk,
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
                idx += 1;
            }
            Ok(())
        }
    }

    async fn download_format_stream(
        &self,
        format: &MediaFormat,
        dest: &Path,
        concurrency: usize,
        cancel_token: CancellationToken,
    ) -> Result<u64, EngineError> {
        if format.protocol.as_deref() == Some("m3u8_native") || format.url.contains(".m3u8") {
            let hls = HlsDownloader::new(self.client.clone(), concurrency);
            hls.download_to_file(&format.url, dest, cancel_token, None)
                .await
        } else if format.protocol.as_deref() == Some("http_dash_segments")
            || format.url.contains(".mpd")
        {
            let dash = DashDownloader::new(self.client.clone(), concurrency);
            dash.download_to_file(&format.url, dest, cancel_token, None)
                .await
        } else {
            let resp = fetch_with_fallback(&self.client, &format.url).await?;
            stream_to_file(resp, dest, cancel_token).await
        }
    }
}
