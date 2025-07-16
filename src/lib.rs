extern crate alloc;

use codas::types::Text;
use codas_macros::export_coda;
use palette::{IntoColor, Oklab, Oklch, Srgb};

pub mod curve;

export_coda!("src/coda.md");

impl Color {
    /// Return a color decoded from a hexadecimal
    /// string containing a non-linear sRGB color.
    pub fn try_from_hex(hex: Text) -> Result<Self, Error> {
        let srgb: Srgb<u8> = hex.parse().map_err(|_| Error::InvalidColor)?;
        let srgb = srgb.into_linear();
        let oklch: Oklch = srgb.into_color();

        Ok(oklch.into())
    }

    /// Returns a hexadecimal string containing
    /// the non-linear sRGB encoding of this color.
    pub fn to_hex(&self) -> Text {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<u8>::from_linear(oklch.into_color());
        format!("#{srgb:x}").into()
    }

    /// Returns a tuple of the `(r, g, b)` values
    /// of the non-linear sRGB encoding of this color.
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<u8>::from_linear(oklch.into_color());
        (srgb.red, srgb.green, srgb.blue)
    }

    /// Returns a copy of this color with the given
    /// `lightness` value, with an adjusted hue value.
    ///
    /// The adjusted hue is calculated by sampling a
    /// point on a quadratic curve between pure white
    /// and black, controlled by this color.
    pub fn at_hue_adjusted_lightness(&self, lightness: f32) -> Self {
        assert!((0.0..=1.0).contains(&lightness));

        let oklch: Oklch = self.into();
        let oklab: Oklab = oklch.into_color();

        let sampled_color = curve::generate_oklab_samples(oklab, &[lightness])[0];
        let sampled_oklch: Oklch = sampled_color.into_color();

        sampled_oklch.into()
    }

    /// Returns a copy of this color with the given `lightness` value.
    pub fn at_lightness(&self, lightness: f32) -> Self {
        assert!((0.0..=1.0).contains(&lightness));
        Self {
            l: lightness,
            ..self.clone()
        }
    }
}

impl From<Oklch> for Color {
    fn from(value: Oklch) -> Self {
        Self {
            l: value.l,
            c: value.chroma,
            h: value.hue.into_positive_degrees(),
        }
    }
}

impl From<&Color> for Oklch {
    fn from(value: &Color) -> Self {
        Self::from_components((value.l, value.c, value.h))
    }
}

impl Neutrals {
    pub fn from_color_hue_adjusted(color: &Color) -> Self {
        Self {
            darkest: color.at_hue_adjusted_lightness(0.20),
            darker: color.at_hue_adjusted_lightness(0.25),
            dark: color.at_hue_adjusted_lightness(0.45),
            darkish: color.at_hue_adjusted_lightness(0.55),
            lightish: color.at_hue_adjusted_lightness(0.62),
            light: color.at_hue_adjusted_lightness(0.72),
            lighter: color.at_hue_adjusted_lightness(0.92),
            lightest: color.at_hue_adjusted_lightness(0.97),
        }
    }

    pub fn from_color(color: &Color) -> Self {
        Self {
            darkest: color.at_lightness(0.20),
            darker: color.at_lightness(0.25),
            dark: color.at_lightness(0.45),
            darkish: color.at_lightness(0.55),
            lightish: color.at_lightness(0.62),
            light: color.at_lightness(0.72),
            lighter: color.at_lightness(0.92),
            lightest: color.at_lightness(0.97),
        }
    }
}

impl<'a> IntoIterator for &'a Neutrals {
    type Item = &'a Color;
    type IntoIter = alloc::vec::IntoIter<Self::Item>;

    /// Returns an iterator over the neutral colors,
    /// in increasing order of their lightness values.
    fn into_iter(self) -> Self::IntoIter {
        vec![
            &self.darkest,
            &self.darker,
            &self.dark,
            &self.darkish,
            &self.lightish,
            &self.light,
            &self.lighter,
            &self.lightest,
        ]
        .into_iter()
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidColor,
}
