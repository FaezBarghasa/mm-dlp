use mm_dlp_core::postprocessor::ffmpeg::FfmpegMuxer;

#[test]
fn test_ffmpeg_progress_parsing_logic() {
    let line = "frame= 1234 fps= 30 q=28.0 size=   4096kB time=00:00:10.00 bitrate=3200.0kbits/s speed=1.5x";
    let progress = FfmpegMuxer::parse_progress_line(line);
    
    assert_eq!(progress.frame, Some(1234));
    assert_eq!(progress.fps, Some(30.0));
    assert_eq!(progress.q, Some(28.0));
    assert_eq!(progress.size_kb, Some(4096));
    assert_eq!(progress.time.as_deref(), Some("00:00:10.00"));
    assert_eq!(progress.bitrate.as_deref(), Some("3200.0kbits/s"));
    assert_eq!(progress.speed, Some(1.5));
}