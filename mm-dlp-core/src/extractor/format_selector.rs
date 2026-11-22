use crate::error::EngineError;
use crate::extractor::traits::{MediaFormat, MediaInfo};

#[derive(Debug, Clone, PartialEq)]
pub enum SelectedFormat<'a> {
    Single(&'a MediaFormat),
    Merged {
        video: &'a MediaFormat,
        audio: &'a MediaFormat,
    },
}

impl<'a> SelectedFormat<'a> {
    pub fn video_format(&self) -> Option<&'a MediaFormat> {
        match self {
            SelectedFormat::Single(f) if f.has_video() => Some(*f),
            SelectedFormat::Merged { video, .. } => Some(*video),
            _ => None,
        }
    }

    pub fn audio_format(&self) -> Option<&'a MediaFormat> {
        match self {
            SelectedFormat::Single(f) if f.has_audio() => Some(*f),
            SelectedFormat::Merged { audio, .. } => Some(*audio),
            _ => None,
        }
    }

    pub fn primary_ext(&self) -> &str {
        match self {
            SelectedFormat::Single(f) => &f.ext,
            SelectedFormat::Merged { video, .. } => {
                if video.ext == "webm" {
                    "webm"
                } else {
                    "mp4"
                }
            }
        }
    }
}

pub struct FormatSelector;

impl FormatSelector {
    /// Select format(s) from a MediaInfo according to a yt-dlp format specification string.
    /// Supports:
    /// - "bestvideo+bestaudio/best", "bv*+ba/b"
    /// - "bestvideo+bestaudio", "bv+ba"
    /// - "best", "worst", "b", "w"
    /// - "bestvideo", "bv"
    /// - "bestaudio", "ba"
    /// - Numeric format IDs e.g. "137+140", "22", "18"
    /// - Format filters e.g. "bestvideo[height<=1080]+bestaudio/best"
    pub fn select<'a>(info: &'a MediaInfo, spec: &str) -> Result<SelectedFormat<'a>, EngineError> {
        let spec = spec.trim();
        let fallback_specs: Vec<&str> = spec.split('/').map(|s| s.trim()).collect();

        for candidate_spec in fallback_specs {
            if candidate_spec.is_empty() {
                continue;
            }
            if let Some(selected) = Self::try_select_single_or_merged(info, candidate_spec) {
                return Ok(selected);
            }
        }

        // Final fallback: best format available
        if let Some(f) = info.best_format() {
            return Ok(SelectedFormat::Single(f));
        }

        Err(EngineError::InvalidConfig(format!(
            "No matching format found for selector '{}'",
            spec
        )))
    }

    fn try_select_single_or_merged<'a>(
        info: &'a MediaInfo,
        spec: &str,
    ) -> Option<SelectedFormat<'a>> {
        if spec.contains('+') {
            let parts: Vec<&str> = spec.splitn(2, '+').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                let v_part = parts[0];
                let a_part = parts[1];

                let video = Self::select_video_candidate(info, v_part);
                let audio = Self::select_audio_candidate(info, a_part);

                if let (Some(v), Some(a)) = (video, audio) {
                    return Some(SelectedFormat::Merged { video: v, audio: a });
                }
            }
        }

        // Single format selection
        Self::select_single_candidate(info, spec).map(SelectedFormat::Single)
    }

    fn select_single_candidate<'a>(info: &'a MediaInfo, spec: &str) -> Option<&'a MediaFormat> {
        let (name, filter) = Self::parse_name_and_filter(spec);

        match name {
            "best" | "b" | "" => {
                let all: Vec<&MediaFormat> = info.formats.iter().collect();
                let candidates = Self::filter_candidates(&all, filter);
                candidates
                    .into_iter()
                    .max_by_key(|f| Self::general_score(f))
            }
            "worst" | "w" => {
                let all: Vec<&MediaFormat> = info.formats.iter().collect();
                let candidates = Self::filter_candidates(&all, filter);
                candidates
                    .into_iter()
                    .min_by_key(|f| Self::general_score(f))
            }
            "bestvideo" | "bv" | "bestvideo*" | "bv*" => Self::select_video_candidate(info, spec),
            "bestaudio" | "ba" | "bestaudio*" | "ba*" => Self::select_audio_candidate(info, spec),
            id => {
                // Check by exact format_id match
                info.formats.iter().find(|f| f.format_id == id)
            }
        }
    }

    fn select_video_candidate<'a>(info: &'a MediaInfo, spec: &str) -> Option<&'a MediaFormat> {
        let (name, filter) = Self::parse_name_and_filter(spec);
        if !["bestvideo", "bv", "bestvideo*", "bv*", "best", "b", ""].contains(&name)
            && let Some(f) = info
                .formats
                .iter()
                .find(|f| f.format_id == name && f.has_video())
        {
            return Some(f);
        }
        let candidates: Vec<&'a MediaFormat> =
            info.formats.iter().filter(|f| f.has_video()).collect();

        let filtered = Self::filter_candidates(&candidates, filter);
        filtered.into_iter().max_by_key(|f| Self::video_score(f))
    }

    fn select_audio_candidate<'a>(info: &'a MediaInfo, spec: &str) -> Option<&'a MediaFormat> {
        let (name, filter) = Self::parse_name_and_filter(spec);
        if !["bestaudio", "ba", "bestaudio*", "ba*", "best", "b", ""].contains(&name)
            && let Some(f) = info
                .formats
                .iter()
                .find(|f| f.format_id == name && f.has_audio())
        {
            return Some(f);
        }
        let candidates: Vec<&'a MediaFormat> =
            info.formats.iter().filter(|f| f.has_audio()).collect();

        let filtered = Self::filter_candidates(&candidates, filter);
        filtered.into_iter().max_by_key(|f| Self::audio_score(f))
    }

    fn parse_name_and_filter(spec: &str) -> (&str, Option<&str>) {
        if let Some(start_bracket) = spec.find('[')
            && let Some(end_bracket) = spec.rfind(']')
            && end_bracket > start_bracket
        {
            let name = spec[..start_bracket].trim();
            let filter = spec[start_bracket + 1..end_bracket].trim();
            return (name, Some(filter));
        }
        (spec.trim(), None)
    }

    fn filter_candidates<'a>(
        candidates: &[&'a MediaFormat],
        filter: Option<&str>,
    ) -> Vec<&'a MediaFormat> {
        let filter = match filter {
            Some(f) if !f.is_empty() => f,
            _ => return candidates.to_vec(),
        };

        candidates
            .iter()
            .copied()
            .filter(|f| Self::matches_filter(f, filter))
            .collect()
    }

    fn matches_filter(format: &MediaFormat, filter: &str) -> bool {
        // Simple filter evaluations: height<=1080, height>=720, ext=mp4, ext!=webm
        if let Some(val) = filter.strip_prefix("height<=")
            && let Ok(limit) = val.parse::<u32>()
        {
            return format.height.unwrap_or(0) <= limit;
        }
        if let Some(val) = filter.strip_prefix("height>=")
            && let Ok(limit) = val.parse::<u32>()
        {
            return format.height.unwrap_or(0) >= limit;
        }
        if let Some(val) = filter.strip_prefix("height=")
            && let Ok(limit) = val.parse::<u32>()
        {
            return format.height.unwrap_or(0) == limit;
        }
        if let Some(val) = filter.strip_prefix("ext=") {
            return format.ext == val;
        }
        if let Some(val) = filter.strip_prefix("ext!=") {
            return format.ext != val;
        }
        if filter == "vcodec!=none" {
            return format.has_video();
        }
        if filter == "acodec!=none" {
            return format.has_audio();
        }
        true
    }

    fn video_score(f: &MediaFormat) -> (u32, u32, u64, u8) {
        let height = f.height.unwrap_or(0);
        let width = f.width.unwrap_or(0);
        let tbr = (f.tbr.unwrap_or(0.0) * 1000.0) as u64;
        let codec_score = match f.vcodec.as_deref() {
            Some(c) if c.starts_with("av01") => 3,
            Some(c) if c.starts_with("vp9") => 2,
            Some(c) if c.starts_with("avc1") || c.starts_with("h264") => 1,
            _ => 0,
        };
        (height, width, tbr, codec_score)
    }

    fn audio_score(f: &MediaFormat) -> (u8, u64, u8) {
        let audio_only_bonus = if f.is_audio_only() { 1 } else { 0 };
        let tbr = (f.tbr.unwrap_or(0.0) * 1000.0) as u64;
        let codec_score = match f.acodec.as_deref() {
            Some(c) if c.contains("opus") => 4,
            Some(c) if c.contains("mp4a") || c.contains("aac") => 3,
            Some(c) if c.contains("mp3") => 2,
            Some(c) if c.contains("vorbis") => 1,
            _ => 0,
        };
        (audio_only_bonus, tbr, codec_score)
    }

    fn general_score(f: &MediaFormat) -> (u32, u64) {
        let res = f.height.unwrap_or(0) * f.width.unwrap_or(0);
        let tbr = (f.tbr.unwrap_or(0.0) * 1000.0) as u64;
        (res, tbr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_media_info() -> MediaInfo {
        MediaInfo {
            id: "test1234".into(),
            title: "Test Video".into(),
            description: None,
            uploader: Some("Tester".into()),
            duration: Some(300),
            thumbnail: None,
            webpage_url: Some("https://example.com/watch?v=test1234".into()),
            formats: vec![
                MediaFormat {
                    format_id: "140".into(),
                    url: "https://example.com/140.m4a".into(),
                    ext: "m4a".into(),
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("none".into()),
                    acodec: Some("mp4a.40.2".into()),
                    filesize: Some(3000000),
                    tbr: Some(128.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "251".into(),
                    url: "https://example.com/251.webm".into(),
                    ext: "webm".into(),
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("none".into()),
                    acodec: Some("opus".into()),
                    filesize: Some(3500000),
                    tbr: Some(160.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "18".into(),
                    url: "https://example.com/18.mp4".into(),
                    ext: "mp4".into(),
                    width: Some(640),
                    height: Some(360),
                    fps: Some(30.0),
                    vcodec: Some("avc1.42001E".into()),
                    acodec: Some("mp4a.40.2".into()),
                    filesize: Some(15000000),
                    tbr: Some(500.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "137".into(),
                    url: "https://example.com/137.mp4".into(),
                    ext: "mp4".into(),
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(30.0),
                    vcodec: Some("avc1.640028".into()),
                    acodec: Some("none".into()),
                    filesize: Some(50000000),
                    tbr: Some(4000.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "248".into(),
                    url: "https://example.com/248.webm".into(),
                    ext: "webm".into(),
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(30.0),
                    vcodec: Some("vp9".into()),
                    acodec: Some("none".into()),
                    filesize: Some(45000000),
                    tbr: Some(3500.0),
                    protocol: None,
                    http_headers: None,
                },
            ],
            subtitles: None,
        }
    }

    #[test]
    fn test_select_bestvideo_plus_bestaudio() {
        let info = create_test_media_info();
        let selected = FormatSelector::select(&info, "bestvideo+bestaudio/best").unwrap();
        match selected {
            SelectedFormat::Merged { video, audio } => {
                assert_eq!(video.format_id, "137");
                assert_eq!(audio.format_id, "251");
            }
            _ => panic!("Expected Merged format"),
        }
    }

    #[test]
    fn test_select_shorthand_bv_ba() {
        let info = create_test_media_info();
        let selected = FormatSelector::select(&info, "bv+ba").unwrap();
        match selected {
            SelectedFormat::Merged { video, audio } => {
                assert_eq!(video.format_id, "137");
                assert_eq!(audio.format_id, "251");
            }
            _ => panic!("Expected Merged format"),
        }
    }

    #[test]
    fn test_select_single_audio_only() {
        let info = create_test_media_info();
        let selected = FormatSelector::select(&info, "bestaudio").unwrap();
        match selected {
            SelectedFormat::Single(f) => {
                assert_eq!(f.format_id, "251");
            }
            _ => panic!("Expected Single format"),
        }
    }

    #[test]
    fn test_select_explicit_ids() {
        let info = create_test_media_info();
        let selected = FormatSelector::select(&info, "137+140").unwrap();
        match selected {
            SelectedFormat::Merged { video, audio } => {
                assert_eq!(video.format_id, "137");
                assert_eq!(audio.format_id, "140");
            }
            _ => panic!("Expected Merged format"),
        }
    }

    #[test]
    fn test_select_filter_height() {
        let info = create_test_media_info();
        let selected =
            FormatSelector::select(&info, "bestvideo[height<=720]+bestaudio/best").unwrap();
        match selected {
            SelectedFormat::Merged { video, audio } => {
                assert_eq!(video.format_id, "18");
                assert_eq!(audio.format_id, "251");
            }
            _ => panic!("Expected Merged format"),
        }
    }
}
