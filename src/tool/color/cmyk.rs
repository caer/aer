//! Unstable
use moxcms::{ColorProfile, Layout, TransformOptions};

use super::Color;

/// Coated GRACoL 2006 ICC profile.
///
/// This profile was chosen as the default
/// CMYK ICC profile because it was what my
/// (Caer's) primary printing vendor ([Moo](https://moo.com))
/// used at the time of creating this module.
const ICC_COATED_GRACOL_2006: &[u8] = include_bytes!("GRACoL2006_Coated1v2.icc");

/// Converts `color` to CMYK within the
/// Coated GRACoL 2006 ICC profile, returning
/// an array of `[C, M, Y, K]` values fitted
/// to a range of `0.0` to `1.0`.
pub fn to_cmyk(color: &Color) -> [f32; 4] {
    // Load color profiles.
    let source_profile = ColorProfile::new_srgb();
    let target_profile = ColorProfile::new_from_slice(ICC_COATED_GRACOL_2006).unwrap();
    let transform = source_profile
        .create_transform_f32(
            Layout::Rgb,
            &target_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .unwrap();

    // Load source colors.
    let srgb = color.to_srgb();

    // Transform into destination colors.
    let mut cmyk = [0f32; 4];
    transform.transform(&srgb, &mut cmyk).unwrap();

    // Convert colors into 0.0 to 100.0 range.
    for channel in cmyk.iter_mut() {
        *channel *= 100.0;
    }

    cmyk
}

/// Converts `cmyk` color within the Coated GRACoL 2006
/// ICC profile to a [Color].
pub fn from_cmyk(cmyk: &[f32; 4]) -> Color {
    // Load color profiles.
    let source_profile = ColorProfile::new_from_slice(ICC_COATED_GRACOL_2006).unwrap();
    let target_profile = ColorProfile::new_srgb();
    let transform = source_profile
        .create_transform_f32(
            Layout::Rgba,
            &target_profile,
            Layout::Rgb,
            TransformOptions::default(),
        )
        .unwrap();

    // Load source colors.
    let mut cmyk = *cmyk;
    for channel in cmyk.iter_mut() {
        *channel /= 100.0;
    }

    // Transform into destination colors.
    let mut srgb = [0f32; 3];
    transform.transform(&cmyk, &mut srgb).unwrap();

    Color::from_srgb(srgb)
}
