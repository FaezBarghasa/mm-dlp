use bytes::Bytes;
use reqwest::Client;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

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

        // 1. Look for BaseURL tag
        let mut effective_base = base_url.to_string();
        if let Some(start) = manifest.find("<BaseURL>")
            && let Some(end) = manifest[start + 9..].find("</BaseURL>")
        {
            let inner = &manifest[start + 9..start + 9 + end];
            effective_base = resolve_url(base_url, inner.trim());
        }

        // 2. Parse SegmentTemplate attributes
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

                // Simple segment count estimate from SegmentTimeline or duration
                let mut segment_count = 10;
                if let Some(timeline_start) = manifest.find("<SegmentTimeline>")
                    && let Some(timeline_end) =
                        manifest[timeline_start..].find("</SegmentTimeline>")
                {
                    let timeline_content = &manifest[timeline_start..timeline_start + timeline_end];
                    let count = timeline_content.matches("<S ").count();
                    if count > 0 {
                        segment_count = count;
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

        // 3. Fallback to SegmentList or direct segment URLs if SegmentTemplate not found
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
        })
    }

    /// Download all segments of a DASH manifest into a destination file.
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
        let total_segments = manifest.media_segments.len();

        let mut file = File::create(dest_path).await?;
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
            file.write_all(&init_bytes).await?;
        }

        if total_segments == 0 {
            file.flush().await?;
            return Ok(total_bytes);
        }

        let semaphore = Arc::new(Semaphore::new(self.concurrency));
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

                Ok((seg.index, bytes))
            });
        }

        let mut chunks: Vec<Option<Bytes>> = vec![None; total_segments];
        let mut completed_count = 0;

        while let Some(res) = join_set.join_next().await {
            let (idx, data) = res.map_err(|e| EngineError::Network(e.to_string()))??;
            chunks[idx] = Some(data);
            completed_count += 1;
            if let Some(ref cb) = progress {
                cb(completed_count, total_segments);
            }
        }

        for bytes in chunks.into_iter().flatten() {
            total_bytes += bytes.len() as u64;
            file.write_all(&bytes).await?;
        }

        file.flush().await?;
        Ok(total_bytes)
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
}
