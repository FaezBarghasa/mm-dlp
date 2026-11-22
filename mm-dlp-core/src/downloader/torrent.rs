use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Magnet, ManagedTorrent, Session,
    SessionOptions,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::config::DownloadResult;
use crate::engine::DownloadProgressCallback;
use crate::error::EngineError;

/// Metadata for an individual file within a torrent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentFileEntry {
    pub index: u64,
    pub path: String,
    pub length: u64,
}

/// Comprehensive metadata for a BitTorrent swarm item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentMetadata {
    pub name: String,
    pub info_hash: String,
    pub total_bytes: u64,
    pub files: Vec<TorrentFileEntry>,
}

/// Standalone parser for magnet URIs (BTIH v1 hex and base32 info hashes).
pub fn parse_magnet_uri(uri: &str) -> Result<TorrentMetadata, EngineError> {
    let magnet = Magnet::parse(uri)
        .map_err(|e| EngineError::Torrent(format!("Invalid magnet URI: {}", e)))?;

    let info_hash = magnet
        .as_id20()
        .map(|id| id.as_string())
        .ok_or_else(|| EngineError::Torrent("Magnet link missing valid info hash".to_string()))?;

    let name = magnet.name.unwrap_or_else(|| "unnamed_torrent".to_string());

    Ok(TorrentMetadata {
        name,
        info_hash,
        total_bytes: 0,
        files: Vec::new(),
    })
}

/// Production-grade asynchronous, multi-threaded BitTorrent client.
/// Uses `librqbit` for DHT, peer discovery, rarest-first piece scheduling, and block integrity checking.
pub struct TorrentDownloader {
    session_dir: PathBuf,
}

impl TorrentDownloader {
    pub fn new(session_dir: PathBuf) -> Result<Self, EngineError> {
        if !session_dir.exists() {
            std::fs::create_dir_all(&session_dir).map_err(|e| {
                EngineError::IoError(format!("Failed to create torrent session dir: {}", e))
            })?;
        }
        Ok(Self { session_dir })
    }

    /// Retrieve metadata for a magnet URI or .torrent file without initiating full file download.
    pub async fn get_metadata(&self, source: &str) -> Result<TorrentMetadata, EngineError> {
        // Fast-path: if offline and magnet URI, parse basic metadata directly
        if source.starts_with("magnet:")
            && let Ok(meta) = parse_magnet_uri(source)
            && !meta.name.is_empty()
            && meta.name != "unnamed_torrent"
        {
            debug!("Parsed offline magnet metadata: {:?}", meta);
        }

        let session = Session::new_with_opts(
            self.session_dir.clone(),
            SessionOptions {
                disable_dht: false,
                fastresume: false,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            EngineError::Torrent(format!("Failed to initialize torrent session: {}", e))
        })?;

        let add_opts = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };

        let add_torrent = Self::build_add_torrent(source).await?;
        let resp = session
            .add_torrent(add_torrent, Some(add_opts))
            .await
            .map_err(|e| {
                EngineError::Torrent(format!("Failed to parse torrent metainfo: {}", e))
            })?;

        match resp {
            AddTorrentResponse::ListOnly(list_only) => {
                let info_hash = list_only.info_hash.as_string();
                let name = list_only
                    .info
                    .name
                    .as_ref()
                    .and_then(|n| std::str::from_utf8(n.as_ref()).ok())
                    .unwrap_or("unnamed_torrent")
                    .to_string();

                let mut files = Vec::new();
                let mut total_bytes = 0u64;

                if let Some(meta_files) = &list_only.info.files {
                    for (idx, f) in meta_files.iter().enumerate() {
                        let path_str = f
                            .path
                            .iter()
                            .filter_map(|p| std::str::from_utf8(p.as_ref()).ok())
                            .collect::<Vec<_>>()
                            .join("/");
                        total_bytes += f.length;
                        files.push(TorrentFileEntry {
                            index: idx as u64,
                            path: path_str,
                            length: f.length,
                        });
                    }
                } else if let Some(len) = list_only.info.length {
                    total_bytes = len;
                    files.push(TorrentFileEntry {
                        index: 0,
                        path: name.clone(),
                        length: len,
                    });
                }

                Ok(TorrentMetadata {
                    name,
                    info_hash,
                    total_bytes,
                    files,
                })
            }
            AddTorrentResponse::Added(_, handle)
            | AddTorrentResponse::AlreadyManaged(_, handle) => {
                Self::extract_handle_metadata(&handle)
            }
        }
    }

    /// Download torrent files concurrently with multi-threaded piece scheduling,
    /// block verification, buffered disk writes, and live progress reporting.
    pub async fn download(
        &self,
        _task_id: String,
        source: &str,
        output_dir: &Path,
        concurrency: usize,
        callback: Arc<dyn DownloadProgressCallback>,
        cancel_token: CancellationToken,
    ) -> Result<DownloadResult, EngineError> {
        let start_time = Instant::now();
        callback.on_status("Initializing BitTorrent session".to_string());

        if !output_dir.exists() {
            tokio::fs::create_dir_all(output_dir)
                .await
                .map_err(|e| EngineError::IoError(format!("Failed to create output dir: {}", e)))?;
        }

        let session = Session::new_with_opts(
            output_dir.to_path_buf(),
            SessionOptions {
                disable_dht: false,
                fastresume: true,
                defer_writes_up_to: Some(16), // Buffer background writes in MB
                cancellation_token: Some(cancel_token.clone()),
                concurrent_init_limit: Some(concurrency.max(1)),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| EngineError::Torrent(format!("Failed to create torrent session: {}", e)))?;

        callback.on_status("Resolving peers and torrent metainfo".to_string());
        let add_opts = AddTorrentOptions {
            overwrite: true,
            defer_writes: Some(true),
            ..Default::default()
        };

        let add_torrent = Self::build_add_torrent(source).await?;
        let handle = match session
            .add_torrent(add_torrent, Some(add_opts))
            .await
            .map_err(|e| EngineError::Torrent(format!("Error adding torrent: {}", e)))?
        {
            AddTorrentResponse::Added(_, h) => h,
            AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => {
                return Err(EngineError::Torrent(
                    "Unexpected ListOnly response during download".to_string(),
                ));
            }
        };

        callback.on_status("Connecting to swarm & scheduling pieces".to_string());

        // Background monitoring task for stats & progress reporting
        let handle_clone = handle.clone();
        let cancel_token_clone = cancel_token.clone();
        let callback_loop = callback.clone();

        let monitor_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token_clone.cancelled() => {
                        callback_loop.on_status("Torrent download cancelled".to_string());
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        let stats = handle_clone.stats();
                        let total = stats.total_bytes;
                        let progress = stats.progress_bytes;

                        if total > 0 {
                            let percentage = (progress as f64 / total as f64) * 100.0;
                            callback_loop.on_progress(percentage.min(100.0));
                        }

                        if let Some(ref live) = stats.live {
                            let speed_mbps = live.download_speed.mbps;
                            let peer_count = live.snapshot.peer_stats.live;
                            callback_loop.on_status(format!(
                                "Downloading: {:.1}% ({}/{} bytes) @ {:.2} MiB/s [{} active peers]",
                                if total > 0 { (progress as f64 / total as f64) * 100.0 } else { 0.0 },
                                progress,
                                total,
                                speed_mbps,
                                peer_count
                            ));
                        }

                        if stats.finished {
                            callback_loop.on_progress(100.0);
                            callback_loop.on_status("Torrent download completed & verified".to_string());
                            break;
                        }
                    }
                }
            }
        });

        // Wait until completion or cancellation
        tokio::select! {
            _ = cancel_token.cancelled() => {
                let _ = monitor_task.await;
                Err(EngineError::Cancelled)
            }
            res = handle.wait_until_completed() => {
                let _ = monitor_task.await;
                res.map_err(|e| EngineError::Torrent(format!("Torrent download error: {}", e)))?;

                let meta = Self::extract_handle_metadata(&handle)?;
                let elapsed_millis = start_time.elapsed().as_millis() as u64;

                let final_path = if meta.files.len() == 1 {
                    output_dir.join(&meta.files[0].path)
                } else {
                    output_dir.join(&meta.name)
                };

                let download_result = DownloadResult {
                    file_path: final_path.to_string_lossy().to_string(),
                    total_bytes: meta.total_bytes,
                    elapsed_millis,
                };

                callback.on_complete(download_result.clone());
                info!(
                    "Torrent download finished successfully: {} ({} bytes in {} ms)",
                    download_result.file_path, download_result.total_bytes, elapsed_millis
                );
                Ok(download_result)
            }
        }
    }

    async fn build_add_torrent(source: &str) -> Result<AddTorrent<'static>, EngineError> {
        if source.starts_with("magnet:")
            || source.starts_with("http://")
            || source.starts_with("https://")
        {
            Ok(AddTorrent::Url(Cow::Owned(source.to_string())))
        } else {
            let data = tokio::fs::read(source).await.map_err(|e| {
                EngineError::IoError(format!("Failed to read .torrent file '{}': {}", source, e))
            })?;
            Ok(AddTorrent::TorrentFileBytes(data.into()))
        }
    }

    fn extract_handle_metadata(handle: &ManagedTorrent) -> Result<TorrentMetadata, EngineError> {
        let info_hash = handle.info_hash().as_string();
        handle
            .with_metadata(|meta| {
                let name = meta
                    .info
                    .name
                    .as_ref()
                    .and_then(|n| std::str::from_utf8(n.as_ref()).ok())
                    .unwrap_or("unnamed_torrent")
                    .to_string();

                let mut files = Vec::new();
                let mut total_bytes = 0u64;

                if let Some(meta_files) = &meta.info.files {
                    for (idx, f) in meta_files.iter().enumerate() {
                        let path_str = f
                            .path
                            .iter()
                            .filter_map(|p| std::str::from_utf8(p.as_ref()).ok())
                            .collect::<Vec<_>>()
                            .join("/");
                        total_bytes += f.length;
                        files.push(TorrentFileEntry {
                            index: idx as u64,
                            path: path_str,
                            length: f.length,
                        });
                    }
                } else if let Some(len) = meta.info.length {
                    total_bytes = len;
                    files.push(TorrentFileEntry {
                        index: 0,
                        path: name.clone(),
                        length: len,
                    });
                }

                TorrentMetadata {
                    name,
                    info_hash: info_hash.clone(),
                    total_bytes,
                    files,
                }
            })
            .map_err(|e| EngineError::Torrent(format!("Failed to read handle metadata: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_magnet_uri_hex() {
        let magnet =
            "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=Ubuntu+21.04+Server";
        let meta = parse_magnet_uri(magnet).expect("Failed to parse valid hex magnet URI");
        assert_eq!(meta.info_hash, "cab507494d02ebb1178b38f2e9d7be299c86b862");
        assert_eq!(meta.name, "Ubuntu 21.04 Server");
    }

    #[test]
    fn test_parse_magnet_uri_with_trackers() {
        let magnet = "magnet:?xt=urn:btih:3b245504d5f72b1e7b638671948934f4f6920037&dn=Arch+Linux&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce";
        let meta = parse_magnet_uri(magnet).expect("Failed to parse magnet URI with trackers");
        assert_eq!(meta.info_hash, "3b245504d5f72b1e7b638671948934f4f6920037");
        assert_eq!(meta.name, "Arch Linux");
    }

    #[test]
    fn test_parse_magnet_invalid_scheme() {
        let invalid = "http://example.com/test.torrent";
        let res = parse_magnet_uri(invalid);
        assert!(res.is_err());
    }

    #[test]
    fn test_torrent_metadata_serialization() {
        let entry = TorrentFileEntry {
            index: 0,
            path: "media/sample.mp4".into(),
            length: 1048576,
        };
        let meta = TorrentMetadata {
            name: "sample.mp4".into(),
            info_hash: "cab507494d02ebb1178b38f2e9d7be299c86b862".into(),
            total_bytes: 1048576,
            files: vec![entry],
        };

        let json = serde_json::to_string(&meta).expect("Serialization failed");
        let deserialized: TorrentMetadata =
            serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(meta, deserialized);
    }
}
