use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::flusher::SequentialFlusher;
use super::parallel::SequencedChunk;
use crate::error::EngineError;

#[derive(Debug, Clone, PartialEq)]
pub struct HlsKey {
    pub method: String,
    pub uri: String,
    pub iv: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsSegment {
    pub index: usize,
    pub url: String,
    pub duration: f64,
    pub key: Option<HlsKey>,
}

#[derive(Debug, Clone)]
pub struct HlsPlaylist {
    pub segments: Vec<HlsSegment>,
    pub is_master: bool,
    pub variant_urls: Vec<String>,
}

pub struct HlsDownloader {
    client: Client,
    concurrency: usize,
}

impl HlsDownloader {
    pub fn new(client: Client, concurrency: usize) -> Self {
        Self {
            client,
            concurrency: concurrency.max(1),
        }
    }

    /// Parse an M3U8 manifest content string into structured HlsPlaylist.
    pub fn parse_manifest(manifest: &str, base_url: &str) -> Result<HlsPlaylist, EngineError> {
        let mut segments = Vec::new();
        let mut variant_urls = Vec::new();
        let mut is_master = false;
        let mut current_key: Option<HlsKey> = None;
        let mut current_duration = 0.0;
        let mut index = 0;

        for line in manifest.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("#EXT-X-STREAM-INF:") {
                is_master = true;
                continue;
            }

            if line.starts_with("#EXT-X-KEY:") {
                current_key = Self::parse_key_tag(line, base_url);
                continue;
            }

            if let Some(dur_str) = line.strip_prefix("#EXTINF:") {
                let dur = dur_str
                    .split(',')
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                current_duration = dur;
                continue;
            }

            if line.starts_with('#') {
                continue;
            }

            // This is a URL line
            let resolved_url = resolve_url(base_url, line);

            if is_master {
                variant_urls.push(resolved_url);
            } else {
                segments.push(HlsSegment {
                    index,
                    url: resolved_url,
                    duration: current_duration,
                    key: current_key.clone(),
                });
                index += 1;
            }
        }

        Ok(HlsPlaylist {
            segments,
            is_master,
            variant_urls,
        })
    }

    fn parse_key_tag(line: &str, base_url: &str) -> Option<HlsKey> {
        let params = line.strip_prefix("#EXT-X-KEY:")?;
        let mut method = String::new();
        let mut uri = String::new();
        let mut iv = None;

        for part in params.split(',') {
            let mut kv = part.splitn(2, '=');
            let k = kv.next()?.trim();
            let v = kv.next()?.trim().trim_matches('"');

            match k {
                "METHOD" => method = v.to_string(),
                "URI" => uri = resolve_url(base_url, v),
                "IV" => {
                    let hex_str = v.trim_start_matches("0x").trim_start_matches("0X");
                    if let Ok(bytes) = hex::decode(hex_str) {
                        iv = Some(bytes);
                    }
                }
                _ => {}
            }
        }

        if method == "NONE" || uri.is_empty() {
            None
        } else {
            Some(HlsKey { method, uri, iv })
        }
    }

    /// Download all segments of an HLS playlist to a destination file.
    pub async fn download_to_file(
        &self,
        playlist_url: &str,
        dest_path: &Path,
        cancel_token: CancellationToken,
        progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    ) -> Result<u64, EngineError> {
        // Fetch initial playlist
        let resp =
            self.client.get(playlist_url).send().await.map_err(|e| {
                EngineError::Network(format!("Failed to fetch HLS playlist: {}", e))
            })?;

        let text = resp.text().await.map_err(|e| {
            EngineError::Network(format!("Failed to read HLS playlist text: {}", e))
        })?;

        let mut playlist = Self::parse_manifest(&text, playlist_url)?;

        // If master playlist, select highest quality variant
        if playlist.is_master && !playlist.variant_urls.is_empty() {
            let variant_url = &playlist.variant_urls[0];
            let v_resp =
                self.client.get(variant_url).send().await.map_err(|e| {
                    EngineError::Network(format!("Failed to fetch HLS variant: {}", e))
                })?;
            let v_text = v_resp
                .text()
                .await
                .map_err(|e| EngineError::Network(format!("Failed to read variant: {}", e)))?;
            playlist = Self::parse_manifest(&v_text, variant_url)?;
        }

        let total_segments = playlist.segments.len();
        if total_segments == 0 {
            return Err(EngineError::ExtractorError(
                "HLS playlist contains 0 media segments".into(),
            ));
        }

        let buffer_capacity = (self.concurrency * 2).max(16);
        let (tx, rx) = mpsc::channel(buffer_capacity);
        let key_cache: Arc<RwLock<HashMap<String, Bytes>>> = Arc::new(RwLock::new(HashMap::new()));
        let dest_path_buf = dest_path.to_path_buf();

        let flusher_handle = tokio::spawn(async move {
            let flusher = SequentialFlusher::new();
            flusher
                .flush_to_disk(dest_path_buf, rx, total_segments)
                .await
        });

        let semaphore = Arc::new(Semaphore::new(self.concurrency));
        let mut join_set = JoinSet::new();

        for seg in playlist.segments {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                EngineError::ConcurrencyError(format!("Semaphore acquisition failed: {}", e))
            })?;
            let client = self.client.clone();
            let token = cancel_token.clone();
            let cache = key_cache.clone();

            join_set.spawn(async move {
                let _permit = permit;
                if token.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }

                let bytes = client
                    .get(&seg.url)
                    .send()
                    .await
                    .map_err(|e| EngineError::Network(format!("Segment fetch failed: {}", e)))?
                    .bytes()
                    .await
                    .map_err(|e| EngineError::Network(format!("Segment read failed: {}", e)))?;

                // If segment has encryption key, decrypt it using cached key if available
                let data = if let Some(ref key_info) = seg.key {
                    if key_info.method == "AES-128" {
                        let key_bytes = {
                            let cached = cache.read().await.get(&key_info.uri).cloned();
                            if let Some(k) = cached {
                                k
                            } else {
                                let fetched = client
                                    .get(&key_info.uri)
                                    .send()
                                    .await
                                    .map_err(|e| {
                                        EngineError::Network(format!("HLS key fetch failed: {}", e))
                                    })?
                                    .bytes()
                                    .await
                                    .map_err(|e| {
                                        EngineError::Network(format!("HLS key read failed: {}", e))
                                    })?;
                                cache
                                    .write()
                                    .await
                                    .insert(key_info.uri.clone(), fetched.clone());
                                fetched
                            }
                        };

                        let iv = key_info
                            .iv
                            .clone()
                            .unwrap_or_else(|| default_iv_for_index(seg.index));

                        decrypt_aes_128_cbc(&bytes, &key_bytes, &iv)?
                    } else {
                        bytes
                    }
                } else {
                    bytes
                };

                Ok(SequencedChunk {
                    index: seg.index,
                    data,
                })
            });
        }

        let mut completed_count = 0;
        while let Some(res) = join_set.join_next().await {
            let chunk_res = res.map_err(|e| EngineError::Network(e.to_string()))?;
            match chunk_res {
                Ok(chunk) => {
                    if tx.send(Ok(chunk)).await.is_err() {
                        return Err(EngineError::IoError("Flusher receiver dropped".into()));
                    }
                    completed_count += 1;
                    if let Some(ref cb) = progress {
                        cb(completed_count, total_segments);
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(err.clone())).await;
                    return Err(err);
                }
            }
        }

        drop(tx);
        let total_bytes = flusher_handle
            .await
            .map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;

        Ok(total_bytes)
    }
}

fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if let Ok(base_url) = url::Url::parse(base)
        && let Ok(joined) = base_url.join(relative)
    {
        return joined.to_string();
    }
    relative.to_string()
}

fn default_iv_for_index(index: usize) -> Vec<u8> {
    let mut iv = vec![0u8; 16];
    let bytes = (index as u64).to_be_bytes();
    iv[8..16].copy_from_slice(&bytes);
    iv
}

fn decrypt_aes_128_cbc(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Bytes, EngineError> {
    if key.len() != 16 {
        return Err(EngineError::ExtractorError(format!(
            "Invalid AES-128 key length: {}",
            key.len()
        )));
    }
    let cipher = boring::symm::Cipher::aes_128_cbc();
    let decrypted = boring::symm::decrypt(cipher, key, Some(iv), data).map_err(|e| {
        EngineError::ExtractorError(format!("AES-128-CBC decryption failed: {}", e))
    })?;
    Ok(Bytes::from(decrypted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hls_media_playlist() {
        let manifest = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:10.0,
segment0.ts
#EXTINF:9.5,
segment1.ts
#EXT-X-ENDLIST"#;

        let playlist =
            HlsDownloader::parse_manifest(manifest, "https://example.com/hls/master.m3u8").unwrap();
        assert!(!playlist.is_master);
        assert_eq!(playlist.segments.len(), 2);
        assert_eq!(
            playlist.segments[0].url,
            "https://example.com/hls/segment0.ts"
        );
        assert_eq!(playlist.segments[0].duration, 10.0);
        assert_eq!(
            playlist.segments[1].url,
            "https://example.com/hls/segment1.ts"
        );
        assert_eq!(playlist.segments[1].duration, 9.5);
    }

    #[test]
    fn test_parse_hls_encrypted_playlist() {
        let manifest = r#"#EXTM3U
#EXT-X-KEY:METHOD=AES-128,URI="https://example.com/key.bin",IV=0x00000000000000000000000000000001
#EXTINF:10.0,
encrypted_seg0.ts"#;

        let playlist =
            HlsDownloader::parse_manifest(manifest, "https://example.com/hls/stream.m3u8").unwrap();
        assert_eq!(playlist.segments.len(), 1);
        let seg = &playlist.segments[0];
        let key = seg.key.as_ref().expect("Key should be parsed");
        assert_eq!(key.method, "AES-128");
        assert_eq!(key.uri, "https://example.com/key.bin");
        assert_eq!(key.iv.as_ref().unwrap().len(), 16);
    }
}
