use std::io::Cursor;

use image::ImageFormat;

use crate::{MediaCategory, proc::asset::Asset};

use super::{ProcessesAssets, ProcessingError};

/// Resizes images to fit within a given width and height,
/// preserving the image's original aspect ratio.
///
/// If the image is already within the given width and height,
/// this processor does nothing.
///
/// This processor uses a [Lanczos](https://mazzo.li/posts/lanczos.html)
/// filter when resizing images. This filter is one of the slowest, but
/// produces consistently high-quality results, making it best suited
/// for processing _static_ content.
pub struct ImageResizeProcessor {
    /// The maximum width of the resized image.
    width: u32,

    /// The maximum height of the resized image.
    height: u32,
}

impl ProcessesAssets for ImageResizeProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        // Skip assets that aren't images.
        if asset.media_type().category() != MediaCategory::Image {
            tracing::debug!(
                "skipping asset {}: not an image: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        // Extract image bytes.
        let image_format = ImageFormat::from_path(asset.path().as_str()).map_err(|e| {
            ProcessingError::Malformed {
                message: e.to_string().into(),
            }
        })?;
        let image_bytes = asset.as_mut_bytes()?;
        let image =
            image::load_from_memory(image_bytes).map_err(|e| ProcessingError::Malformed {
                message: e.to_string().into(),
            })?;

        // Skip resizing if the image is already inside the bounding box.
        if image.width() <= self.width && image.height() <= self.height {
            tracing::debug!(
                "skipping asset {}: already fits within {}x{}px",
                asset.path(),
                self.width,
                self.height
            );
            return Ok(());
        }

        // Resize the image to fit the bounding box.
        let image = image.resize(
            self.width,
            self.height,
            image::imageops::FilterType::Lanczos3,
        );

        // Write resized image.
        image_bytes.clear();
        let mut cursor = Cursor::new(image_bytes);
        image
            .write_to(&mut cursor, image_format)
            .map_err(|e| ProcessingError::Malformed {
                message: e.to_string().into(),
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_log::test]
    #[test_log(default_log_filter = "debug")]
    fn resizes_image() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .try_init();

        let source_bytes = std::fs::read("test/example.png").unwrap();

        // Wrap the source image in an asset.
        let mut asset = Asset::new("test/example.png".into(), source_bytes.clone());

        // Resize the image.
        let (width, height) = (300, 300);
        ImageResizeProcessor { width, height }
            .process(&mut asset)
            .unwrap();

        // Check the dimensions of the resized image.
        let resized_image = image::load_from_memory(asset.as_bytes()).unwrap();
        assert_eq!(width, resized_image.width());
        assert_eq!(243, resized_image.height());
    }
}
