use std::io::Cursor;

use image::ImageFormat;

use super::{Asset, Context, MediaType, ProcessesAssets, ProcessingError};

/// Converts `favicon.png` files to `32x32` pixel `favicon.ico` files.
pub struct FaviconProcessor;

impl ProcessesAssets for FaviconProcessor {
    fn process(&self, _context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::Png {
            tracing::debug!(
                "skipping asset {}: not a PNG image: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        // Only process files named "favicon.png".
        let path = asset.path();
        let file_name = path.as_str().rsplit('/').next().unwrap_or(path.as_str());
        if file_name != "favicon.png" {
            tracing::debug!("skipping asset {}: not a favicon.png", asset.path());
            return Ok(());
        }

        // Load the PNG image.
        let image_bytes = asset.as_bytes();
        let png =
            image::load_from_memory_with_format(image_bytes, ImageFormat::Png).map_err(|e| {
                ProcessingError::Malformed {
                    message: e.to_string().into(),
                }
            })?;

        // Resize the PNG to fit within 32x32 (standard favicon size).
        let png = png.thumbnail(32, 32);

        // Encode as ICO.
        let ico_frame = image::codecs::ico::IcoFrame::as_png(
            png.as_bytes(),
            png.width(),
            png.height(),
            png.color().into(),
        )
        .map_err(|e| ProcessingError::Malformed {
            message: e.to_string().into(),
        })?;

        let mut ico_bytes = Vec::new();
        let ico_encoder = image::codecs::ico::IcoEncoder::new(Cursor::new(&mut ico_bytes));
        ico_encoder
            .encode_images(&[ico_frame])
            .map_err(|e| ProcessingError::Malformed {
                message: e.to_string().into(),
            })?;

        // Replace asset content with ICO and update media type.
        asset.replace_with_bytes(ico_bytes, MediaType::Ico);

        tracing::debug!("converted {} to ICO format", asset.path());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_favicon_png_to_ico() {
        // Create a simple PNG image for testing.
        let source_bytes = std::fs::read("test/example.png").unwrap();

        // Wrap in an asset named "favicon.png".
        let mut asset = Asset::new("favicon.png".into(), source_bytes);
        assert_eq!(asset.media_type(), &MediaType::Png);

        // Process the favicon.
        FaviconProcessor
            .process(&mut Context::default(), &mut asset)
            .unwrap();

        // Verify the media type changed to ICO.
        assert_eq!(asset.media_type(), &MediaType::Ico);

        // Verify the content is valid ICO data (starts with ICO magic bytes).
        let ico_bytes = asset.as_bytes();
        assert!(ico_bytes.len() > 6);
        // ICO files start with 00 00 01 00 (reserved, type=1 for ICO).
        assert_eq!(&ico_bytes[0..4], &[0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn skips_non_favicon_png() {
        let source_bytes = std::fs::read("test/example.png").unwrap();

        // Wrap in an asset with a different name.
        let mut asset = Asset::new("other-image.png".into(), source_bytes.clone());
        let original_len = asset.as_bytes().len();

        // Process should skip this file.
        FaviconProcessor
            .process(&mut Context::default(), &mut asset)
            .unwrap();

        // Verify the asset wasn't modified.
        assert_eq!(asset.media_type(), &MediaType::Png);
        assert_eq!(asset.as_bytes().len(), original_len);
    }
}
