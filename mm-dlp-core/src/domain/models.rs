use serde::{Deserialize, Serialize};

/// The audio format of a downloaded or streamed audio track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    Opus,
    Aac,
    Mp3,
    Flac,
    Wav,
}

impl AudioFormat {
    /// Returns the canonical file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::Opus => "opus",
            AudioFormat::Aac => "m4a",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Flac => "flac",
            AudioFormat::Wav => "wav",
        }
    }

    /// Returns the MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            AudioFormat::Opus => "audio/ogg; codecs=opus",
            AudioFormat::Aac => "audio/mp4",
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Flac => "audio/flac",
            AudioFormat::Wav => "audio/wav",
        }
    }
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}
