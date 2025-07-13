use codas::types::Text;
use codas_macros::export_coda;
use palette::{IntoColor, Oklch, Srgb};

export_coda!("src/coda.md");

impl Color {
    /// Return a color decoded from a hexadecimal
    /// string containing a non-linear sRGB color.
    pub fn from_hex(hex: Text) -> Self {
        let srgb: Srgb<u8> = hex.parse().unwrap();
        let srgb = srgb.into_linear();
        let oklch: Oklch = srgb.into_color();

        oklch.into()
    }

    /// Returns a hexadecimal string containing
    /// the non-linear sRGB encoding of this color.
    pub fn to_hex(&self) -> Text {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<u8>::from_linear(oklch.into_color());
        format!("#{:x}", srgb).into()
    }

    /// Returns a tuple of the `(r, g, b)` values
    /// of the non-linear sRGB encoding of this color.
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        let oklch = Oklch::from(self);
        let srgb = Srgb::<u8>::from_linear(oklch.into_color());
        (srgb.red, srgb.green, srgb.blue)
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
