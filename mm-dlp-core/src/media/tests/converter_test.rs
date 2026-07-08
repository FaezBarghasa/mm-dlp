use crate::media::converter::AudioFormat;
use std::path::Path;

fn generate_ffmpeg_command(input: &Path, output: &Path, format: AudioFormat) -> Vec<String> {
    let codec = match format {
        AudioFormat::Flac => "flac",
        AudioFormat::Wav => "pcm_s16le",
        AudioFormat::Mp3 => "libmp3lame",
    };

    vec![
        "ffmpeg".to_string(),
        "-i".to_string(),
        input.to_str().unwrap().to_string(),
        "-c:a".to_string(),
        codec.to_string(),
        "-b:a".to_string(),
        "320k".to_string(),
        output.to_str().unwrap().to_string(),
    ]
}

#[test]
fn test_ffmpeg_command_generation() {
    let input = Path::new("input.opus");
    let output_flac = Path::new("output.flac");
    let output_wav = Path::new("output.wav");
    let output_mp3 = Path::new("output.mp3");

    let flac_command = generate_ffmpeg_command(input, output_flac, AudioFormat::Flac);
    assert_eq!(flac_command[5], "flac");

    let wav_command = generate_ffmpeg_command(input, output_wav, AudioFormat::Wav);
    assert_eq!(wav_command[5], "pcm_s16le");

    let mp3_command = generate_ffmpeg_command(input, output_mp3, AudioFormat::Mp3);
    assert_eq!(mp3_command[5], "libmp3lame");
}
