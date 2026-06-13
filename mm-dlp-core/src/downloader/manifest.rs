use crate::client::EngineError;

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadSegment {
    pub index: usize,
    pub url: String,
}

pub fn parse_m3u8(manifest_content: &str, base_url: &str) -> Result<Vec<DownloadSegment>, EngineError> {
    let mut segments = Vec::new();
    let mut index = 0;

    for line in manifest_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("{}/{}", base_url.trim_end_matches('/'), trimmed.trim_start_matches('/'))
        };

        segments.push(DownloadSegment { index, url });
        index += 1;
    }

    Ok(segments)
}

pub fn parse_dash(manifest_content: &str, base_url: &str) -> Result<Vec<DownloadSegment>, EngineError> {
    let mut segments = Vec::new();
    let mut index = 0;

    for line in manifest_content.lines() {
        if let Some(media_start) = line.find("media=\"") {
            let start = media_start + 7;
            if let Some(media_end) = line[start..].find('"') {
                let url_part = &line[start..start + media_end];
                let url = if url_part.starts_with("http://") || url_part.starts_with("https://") {
                    url_part.to_string()
                } else {
                    format!("{}/{}", base_url.trim_end_matches('/'), url_part.trim_start_matches('/'))
                };

                segments.push(DownloadSegment { index, url });
                index += 1;
            }
        }
    }

    Ok(segments)
}