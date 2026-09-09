use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;
use crate::extractor::PlatformRegistry;

#[derive(Clone)]
struct CachedStream {
    candidate: StreamCandidate,
    cached_at: Instant,
}

pub struct ResolutionCache {
    entries: RwLock<HashMap<String, CachedStream>>,
    ttl: Duration,
}

impl ResolutionCache {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    pub async fn get(&self, key: &str) -> Option<StreamCandidate> {
        let cache = self.entries.read().await;
        if let Some(entry) = cache.get(key) {
            if entry.cached_at.elapsed() < self.ttl {
                return Some(entry.candidate.clone());
            }
        }
        None
    }

    pub async fn insert(&self, key: String, candidate: StreamCandidate) {
        let mut cache = self.entries.write().await;
        cache.insert(
            key,
            CachedStream {
                candidate,
                cached_at: Instant::now(),
            },
        );
    }
}

pub struct SourceResolver {
    registry: Arc<PlatformRegistry>,
    cache: ResolutionCache,
}

impl SourceResolver {
    pub fn new(registry: Arc<PlatformRegistry>) -> Self {
        Self {
            registry,
            cache: ResolutionCache::new(3600),
        }
    }

    pub fn calculate_match_score(target: &MediaMetadata, candidate: &MediaMetadata) -> f64 {
        let title_sim = strsim::normalized_levenshtein(&target.title.to_lowercase(), &candidate.title.to_lowercase());
        let artist_sim = strsim::normalized_levenshtein(&target.artist.to_lowercase(), &candidate.artist.to_lowercase());

        let duration_score = if target.duration_seconds > 0 && candidate.duration_seconds > 0 {
            let diff = (target.duration_seconds as i64 - candidate.duration_seconds as i64).abs();
            if diff <= 2 {
                1.0
            } else if diff <= 10 {
                0.8
            } else if diff <= 30 {
                0.5
            } else {
                0.0
            }
        } else {
            0.5
        };

        (title_sim * 0.5) + (artist_sim * 0.3) + (duration_score * 0.2)
    }

    pub async fn resolve_best_stream(
        &self,
        client: &Client,
        metadata: &MediaMetadata,
    ) -> Result<StreamCandidate, EngineError> {
        let cache_key = format!("{}:{}", metadata.artist, metadata.title);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return Ok(cached);
        }

        let query = format!("{} {}", metadata.artist, metadata.title);

        let sc_extractor = self.registry.get_extractor("soundcloud");
        let yt_extractor = self.registry.get_extractor("youtube");

        let client_sc = client.clone();
        let client_yt = client.clone();
        let query_sc = query.clone();
        let query_yt = query.clone();

        let (sc_res, yt_res) = tokio::join!(
            async move {
                if let Some(ext) = sc_extractor {
                    ext.search(&client_sc, &query_sc).await.unwrap_or_default()
                } else {
                    vec![]
                }
            },
            async move {
                if let Some(ext) = yt_extractor {
                    ext.search(&client_yt, &query_yt).await.unwrap_or_default()
                } else {
                    vec![]
                }
            }
        );

        let mut best_candidate: Option<(&'static str, String, f64)> = None;

        for track in sc_res {
            let score = Self::calculate_match_score(metadata, &track);
            if score > best_candidate.as_ref().map(|c| c.2).unwrap_or(0.0) {
                best_candidate = Some(("soundcloud", track.id, score));
            }
        }

        for track in yt_res {
            let score = Self::calculate_match_score(metadata, &track);
            if score > best_candidate.as_ref().map(|c| c.2).unwrap_or(0.0) {
                best_candidate = Some(("youtube", track.id, score));
            }
        }

        let (platform, track_id, score) = best_candidate.ok_or_else(|| {
            EngineError::StreamNotFound(format!("Could not find stream matches for {}", query))
        })?;

        if score < 0.4 {
            return Err(EngineError::StreamNotFound(format!(
                "Best stream match for '{}' scored too low ({:.2})",
                query, score
            )));
        }

        let extractor = self.registry.get_extractor(platform).unwrap();
        let candidate = extractor.get_stream_url(client, &track_id).await?;

        self.cache.insert(cache_key, candidate.clone()).await;
        Ok(candidate)
    }
}
