use reqwest::Client;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::flusher::SequentialFlusher;
use super::parallel::SequencedChunk;
use crate::error::EngineError;

#[derive(Debug, Clone, PartialEq)]
pub struct DashSegment {
    pub index: usize,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct DashManifest {
    pub init_url: Option<String>,
    pub media_segments: Vec<DashSegment>,
    pub is_dynamic: bool,
    pub min_update_period: Option<f64>,
}

pub struct DashDownloader {
    client: Client,
    concurrency: usize,
}

impl DashDownloader {
    pub fn new(client: Client, concurrency: usize) -> Self {
        Self {
            client,
            concurrency: concurrency.max(1),
        }
    }

    /// Parses an MPD XML manifest content and extracts media segments.
    pub fn parse_manifest(manifest: &str, base_url: &str) -> Result<DashManifest, EngineError> {
        let mut init_url = None;
        let mut segments = Vec::new();
        let mut is_dynamic = false;
        let mut min_update_period = None;

        // 1. Look for BaseURL tag
        let mut effective_base = base_url.to_string();
        if let Some(start) = manifest.find("<BaseURL>")
            && let Some(end) = manifest[start + 9..].find("</BaseURL>")
        {
            let inner = &manifest[start + 9..start + 9 + end];
            effective_base = resolve_url(base_url, inner.trim());
        }

        // 2. Parse MPD root attributes: type, mediaPresentationDuration, minimumUpdatePeriod
        let mut presentation_duration_secs: Option<f64> = None;
        if let Some(mpd_start) = manifest.find("<MPD") {
            let mpd_slice = &manifest[mpd_start..];
            let end_tag = mpd_slice.find('>').unwrap_or(mpd_slice.len());
            let mpd_attrs = &mpd_slice[..end_tag];

            if let Some(t_attr) = extract_attribute(mpd_attrs, "type")
                && t_attr.to_lowercase() == "dynamic"
            {
                is_dynamic = true;
            }

            if let Some(dur_str) = extract_attribute(mpd_attrs, "mediaPresentationDuration") {
                presentation_duration_secs = parse_iso8601_duration(&dur_str);
            }

            if let Some(mup_str) = extract_attribute(mpd_attrs, "minimumUpdatePeriod") {
                min_update_period = parse_iso8601_duration(&mup_str);
            }
        }

        // 3. Parse SegmentTemplate attributes
        if let Some(template_start) = manifest.find("<SegmentTemplate") {
            let template_slice = &manifest[template_start..];
            let end_tag = template_slice.find('>').unwrap_or(template_slice.len());
            let attrs = &template_slice[..end_tag];

            if let Some(init_attr) = extract_attribute(attrs, "initialization") {
                init_url = Some(resolve_url(&effective_base, &init_attr));
            }

            if let Some(media_attr) = extract_attribute(attrs, "media") {
                let start_num: usize = extract_attribute(attrs, "startNumber")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                let timescale: f64 = extract_attribute(attrs, "timescale")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1.0);

                let template_duration: Option<f64> =
                    extract_attribute(attrs, "duration").and_then(|s| s.parse().ok());

                // Check for SegmentTimeline with repetition 'r' attributes
                let mut timeline_segments = Vec::new();
                if let Some(timeline_start) = manifest.find("<SegmentTimeline>")
                    && let Some(timeline_end) =
                        manifest[timeline_start..].find("</SegmentTimeline>")
                {
                    let timeline_content = &manifest[timeline_start..timeline_start + timeline_end];
                    let mut curr_t = 0u64;
                    for part in timeline_content.split("<S ") {
                        let trimmed = part.trim();
                        if trimmed.is_empty() || trimmed.starts_with("<SegmentTimeline") {
                            continue;
                        }
                        let s_slice = if let Some(close) = trimmed.find("/>") {
                            &trimmed[..close]
                        } else if let Some(close) = trimmed.find('>') {
                            &trimmed[..close]
                        } else {
                            trimmed
                        };

                        if let Some(t_val) =
                            extract_attribute(s_slice, "t").and_then(|s| s.parse::<u64>().ok())
                        {
                            curr_t = t_val;
                        }
                        let d_val = extract_attribute(s_slice, "d")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);
                        let r_val = extract_attribute(s_slice, "r")
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);

                        let count = if r_val >= 0 { (r_val as usize) + 1 } else { 1 };
                        for _ in 0..count {
                            timeline_segments.push(curr_t);
                            curr_t += d_val;
                        }
                    }
                }

                if !timeline_segments.is_empty() {
                    for (i, time_val) in timeline_segments.iter().enumerate() {
                        let num = start_num + i;
                        let expanded_url = media_attr
                            .replace("$Number$", &num.to_string())
                            .replace("$Number%05d$", &format!("{:05}", num))
                            .replace("$Number%06d$", &format!("{:06}", num))
                            .replace("$Time$", &time_val.to_string());
                        let resolved = resolve_url(&effective_base, &expanded_url);
                        segments.push(DashSegment {
                            index: i,
                            url: resolved,
                        });
                    }
                } else {
                    let mut segment_count = 10;
                    if let (Some(total_sec), Some(dur)) =
                        (presentation_duration_secs, template_duration)
                        && dur > 0.0
                    {
                        let seg_duration_sec = dur / timescale;
                        if seg_duration_sec > 0.0 {
                            segment_count = (total_sec / seg_duration_sec).ceil() as usize;
                        }
                    }

                    for i in 0..segment_count {
                        let num = start_num + i;
                        let expanded_url = media_attr
                            .replace("$Number$", &num.to_string())
                            .replace("$Number%05d$", &format!("{:05}", num))
                            .replace("$Number%06d$", &format!("{:06}", num));
                        let resolved = resolve_url(&effective_base, &expanded_url);
                        segments.push(DashSegment {
                            index: i,
                            url: resolved,
                        });
                    }
                }
            }
        }

        // 4. Fallback to SegmentList or direct segment URLs if SegmentTemplate not found
        if segments.is_empty() {
            let mut index = 0;
            for line in manifest.lines() {
                if let Some(media_start) = line.find("media=\"") {
                    let start = media_start + 7;
                    if let Some(media_end) = line[start..].find('"') {
                        let url_part = &line[start..start + media_end];
                        let resolved = resolve_url(&effective_base, url_part);
                        segments.push(DashSegment {
                            index,
                            url: resolved,
                        });
                        index += 1;
                    }
                }
            }
        }

        Ok(DashManifest {
            init_url,
            media_segments: segments,
            is_dynamic,
            min_update_period,
        })
    }

    /// Download all segments of a DASH manifest into a destination file or stdout.
    pub async fn download_to_file(
        &self,
        manifest_url: &str,
        dest_path: &Path,
        cancel_token: CancellationToken,
        progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    ) -> Result<u64, EngineError> {
        let resp =
            self.client.get(manifest_url).send().await.map_err(|e| {
                EngineError::Network(format!("Failed to fetch DASH manifest: {}", e))
            })?;

        let text = resp.text().await.map_err(|e| {
            EngineError::Network(format!("Failed to read DASH manifest text: {}", e))
        })?;

        let manifest = Self::parse_manifest(&text, manifest_url)?;
        let is_stdout = dest_path == Path::new("-");
        let mut total_bytes = 0u64;

        // Fetch initialization segment if available
        if let Some(ref init_url) = manifest.init_url {
            let init_bytes = self
                .client
                .get(init_url)
                .send()
                .await
                .map_err(|e| EngineError::Network(format!("Failed to fetch init segment: {}", e)))?
                .bytes()
                .await
                .map_err(|e| EngineError::Network(format!("Failed to read init bytes: {}", e)))?;
            total_bytes += init_bytes.len() as u64;

            if is_stdout {
                let mut stdout = tokio::io::stdout();
                stdout.write_all(&init_bytes).await?;
                stdout.flush().await?;
            } else {
                let mut file = File::create(dest_path).await?;
                file.write_all(&init_bytes).await?;
                file.flush().await?;
            }
        }

        if manifest.is_dynamic {
            let dynamic_bytes = self
                .download_dynamic_stream(manifest_url, dest_path, manifest, cancel_token, progress)
                .await?;
            return Ok(total_bytes + dynamic_bytes);
        }

        let total_segments = manifest.media_segments.len();
        if total_segments == 0 {
            return Ok(total_bytes);
        }

        #[cfg(target_os = "ios")]
        let effective_concurrency = self.concurrency.clamp(1, 4);
        #[cfg(not(target_os = "ios"))]
        let effective_concurrency = self.concurrency;

        #[cfg(target_os = "ios")]
        let buffer_capacity = effective_concurrency.max(2);
        #[cfg(not(target_os = "ios"))]
        let buffer_capacity = (effective_concurrency * 2).max(16);

        let (tx, rx) = mpsc::channel(buffer_capacity);
        let dest_path_buf = dest_path.to_path_buf();

        let flusher_handle = tokio::spawn(async move {
            let flusher = SequentialFlusher::new();
            if is_stdout {
                flusher.flush_to_stdout(rx, total_segments).await
            } else {
                flusher
                    .flush_to_disk_with_append(dest_path_buf, rx, total_segments, true)
                    .await
            }
        });

        let semaphore = Arc::new(Semaphore::new(effective_concurrency));
        let mut join_set = JoinSet::new();

        for seg in manifest.media_segments {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                EngineError::ConcurrencyError(format!("Semaphore acquisition failed: {}", e))
            })?;
            let client = self.client.clone();
            let token = cancel_token.clone();

            join_set.spawn(async move {
                let _permit = permit;
                if token.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }

                let bytes = client
                    .get(&seg.url)
                    .send()
                    .await
                    .map_err(|e| EngineError::Network(format!("DASH segment fetch failed: {}", e)))?
                    .bytes()
                    .await
                    .map_err(|e| {
                        EngineError::Network(format!("DASH segment read failed: {}", e))
                    })?;

                Ok(SequencedChunk {
                    index: seg.index,
                    data: bytes,
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
        let flushed_bytes = flusher_handle
            .await
            .map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;

        Ok(total_bytes + flushed_bytes)
    }

    /// Dynamic DASH live stream edge tracker loop.
    pub async fn download_dynamic_stream(
        &self,
        manifest_url: &str,
        dest_path: &Path,
        initial_manifest: DashManifest,
        cancel_token: CancellationToken,
        progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
    ) -> Result<u64, EngineError> {
        let is_stdout = dest_path == Path::new("-");
        let dest_path_buf = dest_path.to_path_buf();

        #[cfg(target_os = "ios")]
        let effective_concurrency = self.concurrency.clamp(1, 4);
        #[cfg(not(target_os = "ios"))]
        let effective_concurrency = self.concurrency;

        #[cfg(target_os = "ios")]
        let buffer_capacity = effective_concurrency.max(2);
        #[cfg(not(target_os = "ios"))]
        let buffer_capacity = (effective_concurrency * 2).max(16);

        let (tx, rx) = mpsc::channel(buffer_capacity);
        let cancel_flusher = cancel_token.clone();

        let flusher_handle = tokio::spawn(async move {
            let flusher = SequentialFlusher::new();
            if is_stdout {
                flusher.flush_live_to_stdout(rx, cancel_flusher).await
            } else {
                let file = SequentialFlusher::open_file_optimized(&dest_path_buf, true).await?;
                let writer = tokio::io::BufWriter::new(file);
                flusher
                    .flush_live_to_writer(writer, rx, cancel_flusher)
                    .await
            }
        });

        let mut seen_urls = HashSet::new();
        let mut seen_queue = std::collections::VecDeque::new();
        const MAX_SEEN_HISTORY: usize = 500;

        let semaphore = Arc::new(Semaphore::new(effective_concurrency));
        let mut join_set = JoinSet::new();
        let mut current_seq_idx = 0usize;
        let mut current_manifest = initial_manifest;

        let poll_interval = Duration::from_secs_f64(
            current_manifest
                .min_update_period
                .unwrap_or(2.0)
                .clamp(0.5, 10.0),
        );
        let mut refresh_interval = tokio::time::interval(poll_interval);
        refresh_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            for seg in &current_manifest.media_segments {
                if seen_urls.insert(seg.url.clone()) {
                    seen_queue.push_back(seg.url.clone());
                    if seen_queue.len() > MAX_SEEN_HISTORY
                        && let Some(old) = seen_queue.pop_front()
                    {
                        seen_urls.remove(&old);
                    }

                    let assigned_index = current_seq_idx;
                    current_seq_idx += 1;

                    let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                        EngineError::ConcurrencyError(format!("Semaphore failed: {}", e))
                    })?;
                    let client = self.client.clone();
                    let token = cancel_token.clone();
                    let seg_url = seg.url.clone();

                    join_set.spawn(async move {
                        let _permit = permit;
                        if token.is_cancelled() {
                            return Err(EngineError::Cancelled);
                        }

                        let bytes = client
                            .get(&seg_url)
                            .send()
                            .await
                            .map_err(|e| {
                                EngineError::Network(format!(
                                    "Dynamic DASH segment fetch failed: {}",
                                    e
                                ))
                            })?
                            .bytes()
                            .await
                            .map_err(|e| {
                                EngineError::Network(format!(
                                    "Dynamic DASH segment read failed: {}",
                                    e
                                ))
                            })?;

                        Ok(SequencedChunk {
                            index: assigned_index,
                            data: bytes,
                        })
                    });
                }
            }

            if !current_manifest.is_dynamic && join_set.is_empty() {
                break;
            }

            tokio::select! {
                _ = cancel_token.cancelled() => break,
                Some(res) = join_set.join_next(), if !join_set.is_empty() => {
                    let chunk_res = res.map_err(|e| EngineError::Network(e.to_string()))?;
                    match chunk_res {
                        Ok(chunk) => {
                            if tx.send(Ok(chunk)).await.is_err() {
                                break;
                            }
                            if let Some(ref cb) = progress {
                                cb(current_seq_idx, current_seq_idx);
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            break;
                        }
                    }
                }
                _ = refresh_interval.tick() => {
                    if current_manifest.is_dynamic
                        && let Ok(resp) = self.client.get(manifest_url).send().await
                        && let Ok(text) = resp.text().await
                        && let Ok(updated) = Self::parse_manifest(&text, manifest_url)
                    {
                        current_manifest = updated;
                    }
                }
            }
        }

        while let Some(res) = join_set.join_next().await {
            if let Ok(Ok(chunk)) = res {
                let _ = tx.send(Ok(chunk)).await;
            }
        }

        drop(tx);
        let flushed_bytes = flusher_handle
            .await
            .map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;

        Ok(flushed_bytes)
    }

    /// Stream all segments directly into an mpsc sender (for real-time on-the-fly muxing).
    pub async fn stream_to_channel(
        &self,
        manifest_url: &str,
        tx: mpsc::Sender<Result<SequencedChunk, EngineError>>,
        cancel_token: CancellationToken,
    ) -> Result<(), EngineError> {
        let resp =
            self.client.get(manifest_url).send().await.map_err(|e| {
                EngineError::Network(format!("Failed to fetch DASH manifest: {}", e))
            })?;
        let text = resp.text().await.map_err(|e| {
            EngineError::Network(format!("Failed to read DASH manifest text: {}", e))
        })?;

        let manifest = Self::parse_manifest(&text, manifest_url)?;
        let mut next_idx = 0usize;

        // If initialization segment present, send it as index 0
        if let Some(ref init_url) = manifest.init_url {
            let init_bytes = self
                .client
                .get(init_url)
                .send()
                .await
                .map_err(|e| EngineError::Network(format!("Init fetch failed: {}", e)))?
                .bytes()
                .await
                .map_err(|e| EngineError::Network(format!("Init read failed: {}", e)))?;
            if tx
                .send(Ok(SequencedChunk {
                    index: next_idx,
                    data: init_bytes,
                }))
                .await
                .is_err()
            {
                return Ok(());
            }
            next_idx += 1;
        }

        let semaphore = Arc::new(Semaphore::new(self.concurrency));
        let mut join_set = JoinSet::new();

        for seg in manifest.media_segments {
            let permit =
                semaphore.clone().acquire_owned().await.map_err(|e| {
                    EngineError::ConcurrencyError(format!("Semaphore failed: {}", e))
                })?;
            let client = self.client.clone();
            let token = cancel_token.clone();
            let mapped_index = next_idx + seg.index;

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

                Ok(SequencedChunk {
                    index: mapped_index,
                    data: bytes,
                })
            });
        }

        while let Some(res) = join_set.join_next().await {
            let chunk_res = res.map_err(|e| EngineError::Network(e.to_string()))?;
            if tx.send(chunk_res).await.is_err() {
                break;
            }
        }

        Ok(())
    }
}

fn parse_iso8601_duration(duration_str: &str) -> Option<f64> {
    // Formats like: PT1H23M45.67S, PT30M, PT600S, PT1.5S
    let s = duration_str.strip_prefix("PT")?;
    let mut total_secs = 0.0;
    let mut current_num = String::new();

    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current_num.push(ch);
        } else {
            let val = current_num.parse::<f64>().ok()?;
            current_num.clear();
            match ch {
                'H' => total_secs += val * 3600.0,
                'M' => total_secs += val * 60.0,
                'S' => total_secs += val,
                _ => {}
            }
        }
    }

    if total_secs > 0.0 {
        Some(total_secs)
    } else {
        None
    }
}

fn extract_attribute(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!("{}=\"", name);
    let start = attrs.find(&pattern)? + pattern.len();
    let end = attrs[start..].find('"')?;
    Some(attrs[start..start + end].to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dash_manifest_template() {
        let manifest = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
            <BaseURL>https://cdn.example.com/dash/</BaseURL>
            <SegmentTemplate initialization="init.mp4" media="chunk-$Number$.m4s" startNumber="1">
                <SegmentTimeline>
                    <S t="0" d="1000"/>
                    <S t="1000" d="1000"/>
                    <S t="2000" d="1000"/>
                </SegmentTimeline>
            </SegmentTemplate>
        </MPD>"#;

        let parsed =
            DashDownloader::parse_manifest(manifest, "https://example.com/manifest.mpd").unwrap();
        assert_eq!(
            parsed.init_url,
            Some("https://cdn.example.com/dash/init.mp4".into())
        );
        assert_eq!(parsed.media_segments.len(), 3);
        assert_eq!(
            parsed.media_segments[0].url,
            "https://cdn.example.com/dash/chunk-1.m4s"
        );
        assert_eq!(
            parsed.media_segments[1].url,
            "https://cdn.example.com/dash/chunk-2.m4s"
        );
    }

    #[test]
    fn test_parse_dash_manifest_with_timeline_repeat() {
        let manifest = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
            <BaseURL>https://cdn.example.com/dash/</BaseURL>
            <SegmentTemplate initialization="init.mp4" media="chunk-$Time$.m4s">
                <SegmentTimeline>
                    <S t="0" d="2000" r="2"/>
                    <S t="6000" d="2000"/>
                </SegmentTimeline>
            </SegmentTemplate>
        </MPD>"#;

        let parsed =
            DashDownloader::parse_manifest(manifest, "https://example.com/manifest.mpd").unwrap();
        // r="2" means 1 + 2 = 3 segments, plus 1 more = 4 segments total
        assert_eq!(parsed.media_segments.len(), 4);
        assert_eq!(
            parsed.media_segments[0].url,
            "https://cdn.example.com/dash/chunk-0.m4s"
        );
        assert_eq!(
            parsed.media_segments[1].url,
            "https://cdn.example.com/dash/chunk-2000.m4s"
        );
        assert_eq!(
            parsed.media_segments[2].url,
            "https://cdn.example.com/dash/chunk-4000.m4s"
        );
        assert_eq!(
            parsed.media_segments[3].url,
            "https://cdn.example.com/dash/chunk-6000.m4s"
        );
    }

    #[test]
    fn test_parse_dash_manifest_duration_calc() {
        let manifest = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT1M30S">
            <BaseURL>https://cdn.example.com/dash/</BaseURL>
            <SegmentTemplate initialization="init.mp4" media="chunk-$Number%05d$.m4s" duration="2000" timescale="1000" startNumber="10">
            </SegmentTemplate>
        </MPD>"#;

        let parsed =
            DashDownloader::parse_manifest(manifest, "https://example.com/manifest.mpd").unwrap();
        // 90 seconds / 2 seconds = 45 segments
        assert_eq!(parsed.media_segments.len(), 45);
        assert_eq!(
            parsed.media_segments[0].url,
            "https://cdn.example.com/dash/chunk-00010.m4s"
        );
        assert_eq!(
            parsed.media_segments[44].url,
            "https://cdn.example.com/dash/chunk-00054.m4s"
        );
    }

    #[test]
    fn test_parse_dynamic_dash_manifest() {
        let manifest = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="dynamic" minimumUpdatePeriod="PT2S">
            <BaseURL>https://cdn.example.com/live/</BaseURL>
            <SegmentTemplate initialization="init.mp4" media="chunk-$Number$.m4s" startNumber="100">
                <SegmentTimeline>
                    <S t="0" d="1000"/>
                    <S t="1000" d="1000"/>
                </SegmentTimeline>
            </SegmentTemplate>
        </MPD>"#;

        let parsed =
            DashDownloader::parse_manifest(manifest, "https://example.com/live.mpd").unwrap();
        assert!(parsed.is_dynamic);
        assert_eq!(parsed.min_update_period, Some(2.0));
        assert_eq!(parsed.media_segments.len(), 2);
    }
}
