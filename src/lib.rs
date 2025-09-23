extern crate alloc;

use codas::types::Text;
use codas_macros::export_coda;
use palette::{FromColor, IntoColor, Oklab, Oklch, Srgb};

pub mod asset;
pub mod cmyk;
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

    /// Return a color decoded from an `[r, g, b]`
    /// array of non-linear sRGB color channels
    /// with a `0.0` to `1.0` range.
    pub fn from_srgb(srgb: [f32; 3]) -> Self {
        let srgb = Srgb::<f32>::from_components((srgb[0], srgb[1], srgb[2]));
        let oklch = Oklch::from_color(srgb);
        oklch.into()
    }

    /// Returns a hexadecimal string containing
    /// the non-linear sRGB encoding of this color.
    pub fn to_hex(&self) -> Text {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<u8>::from_linear(oklch.into_color());
        format!("#{srgb:x}").into()
    }

    /// Return an `[r, g, b]` array of non-linear
    /// sRGB color channels with a `0.0` to `1.0` range
    /// representing this color.
    pub fn to_srgb(&self) -> [f32; 3] {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<f32>::from_color(oklch);
        [srgb.red, srgb.green, srgb.blue]
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

        let sampled_color = curve::sample_quadratic_bezier_oklab_curve(oklab, &[lightness])[0];
        let sampled_oklch: Oklch = sampled_color.into_color();

        sampled_oklch.into()
    }
}

impl alloc::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex().to_uppercase())
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
            darkest: color.at_hue_adjusted_lightness(0.19),
            darker: color.at_hue_adjusted_lightness(0.24),
            dark: color.at_hue_adjusted_lightness(0.41),
            neutral: color.at_hue_adjusted_lightness(0.58),
            light: color.at_hue_adjusted_lightness(0.75),
            lighter: color.at_hue_adjusted_lightness(0.92),
            lightest: color.at_hue_adjusted_lightness(0.97),
        }
    }

    pub fn to_cmyk_adjusted(self) -> Self {
        Self {
            darkest: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.darkest)),
            darker: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.darker)),
            dark: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.dark)),
            neutral: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.neutral)),
            light: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.light)),
            lighter: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.lighter)),
            lightest: crate::cmyk::from_cmyk(&crate::cmyk::to_cmyk(&self.lightest)),
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
            &self.neutral,
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
