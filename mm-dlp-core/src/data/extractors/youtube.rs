use async_trait::async_trait;
use anyhow::{anyhow, Result};
use crate::extractor::traits::{AudioPlatformExtractor, AudioQuality, StreamInfo, TrackMetadata, AudioSource};
use crate::js::decipher::JsDecipher;
use serde_json::Value;
use std::time::Duration;

pub struct YouTubeMusicExtractor {
    client: reqwest::Client,
    decipher: JsDecipher,
}

impl YouTubeMusicExtractor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            decipher: JsDecipher::new()?,
        })
    }
}

#[async_trait]
impl AudioPlatformExtractor for YouTubeMusicExtractor {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>> {
        let search_url = format!("https://music.youtube.com/search?q={}", query);
        let response = self.client.get(&search_url).send().await?.text().await?;

        let results_regex = regex::Regex::new(r"window\[\"ytInitialData\"\] = (\{.*?\});")?;
        let captures = results_regex.captures(&response).ok_or_else(|| anyhow!("Failed to find search results in YouTube Music response"))?;
        let data: Value = serde_json::from_str(&captures[1])?;

        let contents = data["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"].as_array().unwrap();
        let mut tracks = Vec::new();

        for item in contents {
            if let Some(music_shelf) = item.get("musicShelfRenderer") {
                for track_item in music_shelf["contents"].as_array().unwrap() {
                    if let Some(track_data) = track_item.get("musicResponsiveListItemRenderer") {
                        let track_id = track_data["playlistItemData"]["videoId"].as_str().unwrap_or("").to_string();
                        let title = track_data["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"].as_str().unwrap_or("").to_string();
                        let artist = track_data["flexColumns"][1]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"].as_str().unwrap_or("").to_string();

                        tracks.push(TrackMetadata {
                            title,
                            artist,
                            album: None,
                            album_art_url: None,
                            track_id,
                            source: AudioSource::YouTubeMusic,
                        });
                    }
                }
            }
        }
        Ok(tracks)
    }

    async fn get_stream_url(&self, track_id: &str, _quality: AudioQuality) -> Result<StreamInfo> {
        let video_url = format!("https://www.youtube.com/watch?v={}", track_id);
        let response = self.client.get(&video_url).send().await?.text().await?;

        let player_response_regex = regex::Regex::new(r"var ytInitialPlayerResponse = (\{.*?\});")?;
        let player_response_captures = player_response_regex.captures(&response).ok_or_else(|| anyhow!("Failed to find player response"))?;
        let player_response: Value = serde_json::from_str(&player_response_captures[1])?;

        let streaming_data = &player_response["streamingData"];
        let adaptive_formats = streaming_data["adaptiveFormats"].as_array().ok_or_else(|| anyhow!("No adaptive formats found"))?;

        let audio_format = adaptive_formats
            .iter()
            .filter(|f| f["mimeType"].as_str().unwrap_or("").starts_with("audio/"))
            .max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0))
            .ok_or_else(|| anyhow!("No audio formats found"))?;

        let mut stream_url = audio_format["url"].as_str().unwrap_or("").to_string();

        if stream_url.is_empty() {
            let signature_cipher = audio_format["signatureCipher"].as_str().ok_or_else(|| anyhow!("Signature cipher not found"))?;
            let params: Vec<&str> = signature_cipher.split('&').collect();
            let mut url = String::new();
            let mut sp = String::new();
            let mut s = String::new();

            for param in params {
                let mut parts = param.splitn(2, '=');
                let key = parts.next().unwrap();
                let value = parts.next().unwrap();
                match key {
                    "url" => url = urlencoding::decode(value)?.into_owned(),
                    "sp" => sp = value.to_string(),
                    "s" => s = urlencoding::decode(value)?.into_owned(),
                    _ => {}
                }
            }

            let js_player_url_regex = regex::Regex::new(r#""jsUrl":"(/s/player/[^"]+)""#)?;
            let js_player_url_captures = js_player_url_regex.captures(&response).ok_or_else(|| anyhow!("Failed to find JS player URL"))?;
            let js_player_url = format!("https://www.youtube.com{}", &js_player_url_captures[1]);
            let js_code = self.client.get(&js_player_url).send().await?.text().await?;

            let deciphered_signature = self.decipher.decipher(&js_code, &s)?;
            stream_url = format!("{}&{}={}", url, sp, deciphered_signature);
        }

        let metadata = TrackMetadata {
            title: player_response["videoDetails"]["title"].as_str().unwrap_or("").to_string(),
            artist: player_response["videoDetails"]["author"].as_str().unwrap_or("").to_string(),
            album: None,
            album_art_url: Some(player_response["videoDetails"]["thumbnail"]["thumbnails"].as_array().unwrap().last().unwrap()["url"].as_str().unwrap().to_string()),
            track_id: track_id.to_string(),
            source: AudioSource::YouTubeMusic,
        };

        Ok(StreamInfo {
            stream_url,
            format: audio_format["mimeType"].as_str().unwrap_or("").split(';').next().unwrap_or("").to_string(),
            bitrate: audio_format["bitrate"].as_u64().unwrap_or(0) as u32,
            duration_secs: player_response["videoDetails"]["lengthSeconds"].as_str().unwrap_or("0").parse()?,
            metadata,
        })
    }
}
