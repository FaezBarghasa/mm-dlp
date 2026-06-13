use crate::error::{Result, EngineError};

#[derive(Debug, Clone)]
pub struct DownloadSegment {
    pub sequence_number: usize,
    pub uri: String,
    pub duration_seconds: f64,
    pub byte_range: Option<String>,
}

pub fn parse_hls_playlist(m3u8_payload: &str, base_url: &str) -> Result<Vec<DownloadSegment>> {
    let mut segments = Vec::new();
    let mut current_seq = 0;

    for line in m3u8_payload.lines() {
        if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
            current_seq = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
        } else if line.starts_with("#EXTINF:") {
            // Extract duration segment values
        } else if !line.starts_with('#') && !line.is_empty() {
            let full_uri = if line.starts_with("http") {
                line.to_string()
            } else {
                format!("{}/{}", base_url, line)
            };

            segments.push(DownloadSegment {
                sequence_number: current_seq,
                uri: full_uri,
                duration_seconds: 2.0, // default target fallback
                byte_range: None,
            });
            current_seq += 1;
        }
    }

    if segments.is_empty() {
        return Err(EngineError::ExtractorBanned { reason: "No valid streams found".to_string() });
    }

    Ok(segments)
}
