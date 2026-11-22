use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Traverse a JSON value using path segments (strings for keys, numbers for indices).
/// Equivalent to yt-dlp's `traverse_obj(obj, path)`
pub fn traverse_obj<'a>(obj: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = obj;
    for &key in path {
        if let Ok(idx) = key.parse::<usize>() {
            match current {
                Value::Array(arr) => {
                    current = arr.get(idx)?;
                }
                Value::Object(map) => {
                    current = map.get(key)?;
                }
                _ => return None,
            }
        } else {
            match current {
                Value::Object(map) => {
                    current = map.get(key)?;
                }
                _ => return None,
            }
        }
    }
    if current.is_null() {
        None
    } else {
        Some(current)
    }
}

/// Helper to get a string from a JSON traversal.
pub fn traverse_string(obj: &Value, path: &[&str]) -> Option<String> {
    traverse_obj(obj, path).and_then(|v| {
        if let Value::String(s) = v {
            Some(s.clone())
        } else if let Value::Number(n) = v {
            Some(n.to_string())
        } else {
            None
        }
    })
}

/// Helper to get a u64 from a JSON traversal.
pub fn traverse_u64(obj: &Value, path: &[&str]) -> Option<u64> {
    traverse_obj(obj, path).and_then(|v| {
        if let Value::Number(n) = v {
            n.as_u64()
        } else if let Value::String(s) = v {
            s.parse::<u64>().ok()
        } else {
            None
        }
    })
}

static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("Invalid tag regex"));

/// Strip HTML tags and entities, similar to yt-dlp `clean_html`.
pub fn clean_html(html: &str) -> String {
    let stripped = TAG_RE.replace_all(html, "");
    stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

/// Parse duration in format `HH:MM:SS` or `MM:SS` or seconds string.
pub fn parse_duration(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(secs);
    }
    if let Ok(secs_f) = trimmed.parse::<f64>() {
        return Some(secs_f as u64);
    }

    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.len() {
        2 => {
            let m = parts[0].parse::<u64>().ok()?;
            let s = parts[1].parse::<f64>().ok()? as u64;
            Some(m * 60 + s)
        }
        3 => {
            let h = parts[0].parse::<u64>().ok()?;
            let m = parts[1].parse::<u64>().ok()?;
            let s = parts[2].parse::<f64>().ok()? as u64;
            Some(h * 3600 + m * 60 + s)
        }
        _ => None,
    }
}

/// Determine file extension from URL or content-type.
pub fn determine_ext(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    if let Some(pos) = no_query.rfind('.') {
        let ext = &no_query[pos + 1..];
        if ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return ext.to_lowercase();
        }
    }
    "mp4".to_string()
}

/// Update URL queries with extra key-value pairs.
pub fn update_url_query(url: &str, params: &HashMap<&str, &str>) -> String {
    let mut parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return url.to_string(),
    };
    {
        let mut query_pairs = parsed.query_pairs_mut();
        for (k, v) in params {
            query_pairs.append_pair(k, v);
        }
    }
    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_traverse_obj() {
        let data = json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "items": [
                            {"title": "Track 1", "duration": 180},
                            {"title": "Track 2", "duration": 240}
                        ]
                    }
                }
            }
        });

        let title = traverse_string(
            &data,
            &[
                "contents",
                "twoColumnWatchNextResults",
                "results",
                "items",
                "0",
                "title",
            ],
        );
        assert_eq!(title, Some("Track 1".to_string()));

        let dur = traverse_u64(
            &data,
            &[
                "contents",
                "twoColumnWatchNextResults",
                "results",
                "items",
                "1",
                "duration",
            ],
        );
        assert_eq!(dur, Some(240));
    }

    #[test]
    fn test_clean_html() {
        let html = "<div>Hello &amp; welcome to <b>Rust</b>!&nbsp;</div>";
        assert_eq!(clean_html(html), "Hello & welcome to Rust!");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("03:45"), Some(225));
        assert_eq!(parse_duration("01:02:03"), Some(3723));
        assert_eq!(parse_duration("120"), Some(120));
    }

    #[test]
    fn test_determine_ext() {
        assert_eq!(
            determine_ext("https://example.com/video.mp4?token=123"),
            "mp4"
        );
        assert_eq!(determine_ext("https://example.com/audio.m4a"), "m4a");
        assert_eq!(determine_ext("https://example.com/stream"), "mp4");
    }
}
