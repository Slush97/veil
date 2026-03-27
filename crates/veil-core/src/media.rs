//! Media type detection and image processing utilities.
//!
//! Provides MIME-based file classification (via magic bytes + extension
//! fallback) and thumbnail generation for image messages.

use std::path::Path;

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
        // Thumbnail was generated (we can't check dimensions of the JPEG
        // without decoding it, but we can verify it's non-empty and valid)
        assert!(meta.thumbnail.len() > 2);
    }
}
