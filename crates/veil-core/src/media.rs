//! Media type detection and processing utilities.
//!
//! Provides MIME-based file classification (via magic bytes + extension
//! fallback), thumbnail generation for image messages, and audio
//! metadata extraction (duration + waveform).

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Coarse classification of a file's content type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Video,
    Audio,
    File,
}

/// Result of extracting image metadata and generating a thumbnail.
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    /// JPEG-encoded thumbnail, max 200px on the longest side.
    pub thumbnail: Vec<u8>,
}

/// Maximum thumbnail dimension (longest side).
const THUMBNAIL_MAX_DIM: u32 = 200;

/// Detect the media type of `data`, using the file extension from
/// `filename` as a fallback when magic-byte detection is ambiguous.
pub fn detect(data: &[u8], filename: Option<&str>) -> MediaType {
    if let Some(kind) = infer::get(data) {
        return match kind.matcher_type() {
            infer::MatcherType::Image => MediaType::Image,
            infer::MatcherType::Video => MediaType::Video,
            infer::MatcherType::Audio => MediaType::Audio,
            _ => MediaType::File,
        };
    }

    // Fallback: file extension
    if let Some(name) = filename {
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        if let Some(ext) = ext {
            return match ext.as_str() {
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "svg" => {
                    MediaType::Image
                }
                "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" => MediaType::Video,
                "mp3" | "ogg" | "wav" | "flac" | "aac" | "opus" => MediaType::Audio,
                _ => MediaType::File,
            };
        }
    }

    MediaType::File
}

/// Extract dimensions and generate a JPEG thumbnail from image bytes.
///
/// Returns `None` if the image cannot be decoded. This is intentionally
/// non-fatal — the caller can fall back to `MessageKind::File`.
pub fn extract_image_meta(data: &[u8]) -> Option<ImageMeta> {
    let img = image::load_from_memory(data).ok()?;
    let (width, height) = (img.width(), img.height());

    let thumb = img.thumbnail(THUMBNAIL_MAX_DIM, THUMBNAIL_MAX_DIM);

    let mut jpeg_buf = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut jpeg_buf, image::ImageFormat::Jpeg)
        .ok()?;

    Some(ImageMeta {
        width,
        height,
        thumbnail: jpeg_buf.into_inner(),
    })
}

/// Result of extracting audio metadata.
pub struct AudioMeta {
    /// Duration in seconds (0.0 if unknown).
    pub duration_secs: f32,
    /// 64 amplitude samples normalized to 0–255 for waveform visualization.
    pub waveform: Vec<u8>,
}

/// Number of waveform samples to generate.
const WAVEFORM_SAMPLES: usize = 64;

/// Extract duration and waveform from audio bytes.
///
/// Returns `None` if the audio cannot be decoded. Non-fatal — the caller
/// can fall back to `MessageKind::File`.
pub fn extract_audio_meta(data: &[u8]) -> Option<AudioMeta> {
    let cursor = std::io::Cursor::new(data.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let probe = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;

    let mut format = probe.format;
    let track = format.default_track()?;
    let sample_rate = track.codec_params.sample_rate? as f64;
    let track_id = track.id;

    // Try to get duration from metadata
    let duration_secs = track
        .codec_params
        .n_frames
        .map(|n| n as f64 / sample_rate)
        .unwrap_or(0.0) as f32;

    // Decode samples for waveform
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut peak_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let num_samples = decoded.capacity();
        let mut sample_buf = SampleBuffer::<f32>::new(num_samples as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);

        let channels = spec.channels.count().max(1);
        // Take mono mix: average across channels, store absolute peak per chunk
        for frame in sample_buf.samples().chunks(channels) {
            let mono: f32 = frame.iter().sum::<f32>() / channels as f32;
            peak_samples.push(mono.abs());
        }
    }

    // Downsample to WAVEFORM_SAMPLES buckets
    let waveform = if peak_samples.is_empty() {
        vec![0u8; WAVEFORM_SAMPLES]
    } else {
        let bucket_size = peak_samples.len().div_ceil(WAVEFORM_SAMPLES);
        let mut buckets: Vec<f32> = Vec::with_capacity(WAVEFORM_SAMPLES);

        for chunk in peak_samples.chunks(bucket_size) {
            let peak = chunk.iter().copied().fold(0.0f32, f32::max);
            buckets.push(peak);
        }

        // Pad if we got fewer than WAVEFORM_SAMPLES
        while buckets.len() < WAVEFORM_SAMPLES {
            buckets.push(0.0);
        }

        // Normalize to 0–255
        let max_peak = buckets.iter().copied().fold(0.0f32, f32::max);
        if max_peak > 0.0 {
            buckets
                .iter()
                .map(|&v| (v / max_peak * 255.0) as u8)
                .collect()
        } else {
            vec![0u8; WAVEFORM_SAMPLES]
        }
    };

    Some(AudioMeta {
        duration_secs,
        waveform,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_jpeg_from_magic_bytes() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect(&data, None), MediaType::Image);
    }

    #[test]
    fn detect_png_from_magic_bytes() {
        let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect(&data, None), MediaType::Image);
    }

    #[test]
    fn detect_mp4_from_magic_bytes() {
        // MP4 ftyp box: 4-byte size + "ftyp" at offset 4
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&8u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"isom");
        assert_eq!(detect(&data, None), MediaType::Video);
    }

    #[test]
    fn fallback_to_extension() {
        let data = [0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect(&data, Some("photo.png")), MediaType::Image);
        assert_eq!(detect(&data, Some("clip.mp4")), MediaType::Video);
        assert_eq!(detect(&data, Some("song.flac")), MediaType::Audio);
        assert_eq!(detect(&data, Some("readme.txt")), MediaType::File);
    }

    #[test]
    fn unknown_falls_to_file() {
        let data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(detect(&data, None), MediaType::File);
    }

    #[test]
    fn extract_meta_from_valid_png() {
        // Create a minimal 1x1 red PNG in memory
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();

        let meta = extract_image_meta(buf.get_ref()).unwrap();
        assert_eq!(meta.width, 1);
        assert_eq!(meta.height, 1);
        assert!(!meta.thumbnail.is_empty());
        // Verify thumbnail is valid JPEG (starts with FF D8)
        assert_eq!(meta.thumbnail[0], 0xFF);
        assert_eq!(meta.thumbnail[1], 0xD8);
    }

    #[test]
    fn extract_meta_from_garbage_returns_none() {
        let data = [0x00, 0x01, 0x02, 0x03];
        assert!(extract_image_meta(&data).is_none());
    }

    #[test]
    fn thumbnail_respects_max_dimension() {
        // Create a 400x200 image — thumbnail should scale to 200x100
        let img = image::RgbaImage::new(400, 200);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();

        let meta = extract_image_meta(buf.get_ref()).unwrap();
        assert_eq!(meta.width, 400);
        assert_eq!(meta.height, 200);
        assert!(meta.thumbnail.len() > 2);
    }

    #[test]
    fn extract_audio_from_wav() {
        // Build a minimal WAV file: 44-byte header + 1 second of 440Hz sine at 8kHz mono
        let sample_rate: u32 = 8000;
        let num_samples = sample_rate; // 1 second
        let bits_per_sample: u16 = 16;
        let num_channels: u16 = 1;
        let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample) / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * u32::from(block_align);

        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&num_channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());

        // Generate 440Hz sine wave samples
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin();
            let pcm = (sample * 16000.0) as i16;
            wav.extend_from_slice(&pcm.to_le_bytes());
        }

        let meta = extract_audio_meta(&wav).unwrap();
        // Duration should be ~1 second
        assert!(meta.duration_secs > 0.9 && meta.duration_secs < 1.1);
        // Waveform should have exactly 64 samples
        assert_eq!(meta.waveform.len(), WAVEFORM_SAMPLES);
        // Waveform should have non-zero values (sine wave has amplitude)
        assert!(meta.waveform.iter().any(|&v| v > 0));
        // Max value should be 255 (normalized)
        assert_eq!(*meta.waveform.iter().max().unwrap(), 255);
    }

    #[test]
    fn extract_audio_from_garbage_returns_none() {
        let data = [0x00, 0x01, 0x02, 0x03];
        assert!(extract_audio_meta(&data).is_none());
    }
}
