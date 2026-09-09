use crate::client::EngineError;
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::Id;
use std::path::Path;

pub struct NativeMuxer;

impl NativeMuxer {
    pub fn new() -> Result<Self, EngineError> {
        ffmpeg::init().map_err(|e| {
            EngineError::OsApiError(format!("Failed to initialize ffmpeg-next context: {}", e))
        })?;
        Ok(Self)
    }

    pub fn mux_video_audio<P1, P2, P3>(
        &self,
        video_path: P1,
        audio_path: P2,
        output_path: P3,
    ) -> Result<(), EngineError>
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
        P3: AsRef<Path>,
    {
        let mut ictx_video = ffmpeg::format::input(&video_path)
            .map_err(|e| EngineError::OsApiError(format!("Failed to open video context: {}", e)))?;
        let mut ictx_audio = ffmpeg::format::input(&audio_path)
            .map_err(|e| EngineError::OsApiError(format!("Failed to open audio context: {}", e)))?;
        let mut octx = ffmpeg::format::output(&output_path)
            .map_err(|e| EngineError::OsApiError(format!("Failed to prepare output: {}", e)))?;

        let video_stream = ictx_video
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| EngineError::OsApiError("No valid video stream found".into()))?;
        let video_idx = video_stream.index();

        let video_time_base = {
            let mut ost_video = octx
                .add_stream(ffmpeg::encoder::find(Id::None))
                .map_err(|e| EngineError::OsApiError(e.to_string()))?;
            ost_video.set_parameters(video_stream.parameters());
            ost_video.time_base()
        };

        let audio_stream = ictx_audio
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .ok_or_else(|| EngineError::OsApiError("No valid audio stream found".into()))?;
        let audio_idx = audio_stream.index();

        let mut needs_aac_reencode = false;
        let audio_time_base = {
            let mut ost_audio = octx
                .add_stream(ffmpeg::encoder::find(Id::None))
                .map_err(|e| EngineError::OsApiError(e.to_string()))?;

            if audio_stream.parameters().id() != Id::AAC {
                needs_aac_reencode = true;
                
                let mut audio_enc = ffmpeg::codec::context::Context::new()
                    .encoder()
                    .audio()
                    .unwrap();
                audio_enc.set_rate(44100);
                audio_enc.set_channel_layout(ffmpeg::channel_layout::ChannelLayout::STEREO);
                audio_enc.set_format(ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar));
                ost_audio.set_parameters(audio_enc);
            } else {
                ost_audio.set_parameters(audio_stream.parameters());
            }
            ost_audio.time_base()
        };

        octx.write_header()
            .map_err(|e| EngineError::OsApiError(e.to_string()))?;

        let mut video_packets = ictx_video.packets();
        let mut audio_packets = ictx_audio.packets();

        let mut v_done = false;
        let mut a_done = false;

        loop {
            if v_done && a_done { break; }

            if !v_done {
                if let Some((stream, mut packet)) = video_packets.next() {
                    if stream.index() == video_idx {
                        packet.set_stream(0);
                        packet.rescale_ts(stream.time_base(), video_time_base);
                        let _ = packet.write_interleaved(&mut octx);
                    }
                } else {
                    v_done = true;
                }
            }

            if !a_done {
                if let Some((stream, mut packet)) = audio_packets.next() {
                    if stream.index() == audio_idx {
                        packet.set_stream(1);
                        packet.rescale_ts(stream.time_base(), audio_time_base);
                        
                        if needs_aac_reencode {
                            // In full deployment, inject a resampling channel bridge here using SoftwareResampler
                            let _ = packet.write_interleaved(&mut octx);
                        } else {
                            let _ = packet.write_interleaved(&mut octx);
                        }
                    }
                } else {
                    a_done = true;
                }
            }
        }

        octx.write_trailer()
            .map_err(|e| EngineError::OsApiError(e.to_string()))?;
        Ok(())
    }
}