//! Colors, the Wong palette, named colors and colormaps.

mod named;

use crate::error::{Error, Result};

/// An sRGB-encoded color with straight (non-premultiplied) alpha, like Colors.jl's `RGBAf`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Color {
        Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
    /// `Color::hex(0x0072B2)`.
    pub const fn hex(v: u32) -> Color {
        Color::rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }
    /// Gray level `v` in 0..1.
    pub const fn gray(v: f32) -> Color {
        Color::rgb(v, v, v)
    }

    /// Parses `"#0072B2"`, `"#0072B280"`, `"#fff"`, CSS / Colors.jl names (`"red"`, `"gray50"`),
    /// and `"transparent"`.
    pub fn parse(s: &str) -> Result<Color> {
        let t = s.trim();
        if let Some(h) = t.strip_prefix('#') {
            let v = u32::from_str_radix(h, 16).map_err(|_| bad(s))?;
            return match h.len() {
                3 => Ok(Color::rgb8(
                    ((v >> 8) & 0xF) as u8 * 17,
                    ((v >> 4) & 0xF) as u8 * 17,
                    (v & 0xF) as u8 * 17,
                )),
                6 => Ok(Color::hex(v)),
                8 => Ok(Color::hex(v >> 8).with_alpha((v & 0xFF) as f32 / 255.0)),
                _ => Err(bad(s)),
            };
        }
        let key: String = t
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '_')
            .collect::<String>()
            .to_ascii_lowercase();
        if key == "transparent" {
            return Ok(Color::TRANSPARENT);
        }
        if let Some(rest) = key
            .strip_prefix("gray")
            .or_else(|| key.strip_prefix("grey"))
        {
            if let Ok(n) = rest.parse::<u32>() {
                if n <= 100 {
                    return Ok(Color::gray(n as f32 / 100.0));
                }
            }
        }
        named::lookup(&key).map(Color::hex).ok_or_else(|| bad(s))
    }

    /// Premultiplied RGBA8 packed little-endian (r in the low byte), for GPU storage buffers.
    pub(crate) fn to_premul_u32(self) -> u32 {
        let a = self.a.clamp(0.0, 1.0);
        let q = |v: f32| ((v.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u32;
        q(self.r) | (q(self.g) << 8) | (q(self.b) << 16) | (((a * 255.0 + 0.5) as u32) << 24)
    }

    /// Straight-alpha RGBA8.
    pub(crate) fn to_rgba8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// Linear interpolation in sRGB-encoded space (what Makie does).
    pub fn lerp(self, other: Color, t: f32) -> Color {
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }
}

fn bad(s: &str) -> Error {
    Error::Parse(format!(
        "unknown color {s:?}; use a CSS name like \"red\", \"gray50\", or hex \"#0072B2\""
    ))
}

/// Named colors (CSS / Colors.jl names).
pub mod colors {
    use super::Color;
    pub const BLACK: Color = Color::hex(0x000000);
    pub const WHITE: Color = Color::hex(0xFFFFFF);
    pub const RED: Color = Color::hex(0xFF0000);
    pub const GREEN: Color = Color::hex(0x008000);
    pub const BLUE: Color = Color::hex(0x0000FF);
    pub const ORANGE: Color = Color::hex(0xFFA500);
    pub const PURPLE: Color = Color::hex(0x800080);
    pub const YELLOW: Color = Color::hex(0xFFFF00);
    pub const CYAN: Color = Color::hex(0x00FFFF);
    pub const MAGENTA: Color = Color::hex(0xFF00FF);
    pub const GRAY: Color = Color::hex(0x808080);
    pub const LIGHTGRAY: Color = Color::hex(0xD3D3D3);
    pub const DARKGRAY: Color = Color::hex(0xA9A9A9);
    pub const BROWN: Color = Color::hex(0xA52A2A);
    pub const PINK: Color = Color::hex(0xFFC0CB);
    pub const TRANSPARENT: Color = Color::TRANSPARENT;
}

/// Makie's default categorical palette (Wong 2011), in cycle order.
pub const WONG: [Color; 7] = [
    Color::hex(0x0072B2),
    Color::hex(0xE69F00),
    Color::hex(0x009E73),
    Color::hex(0xCC79A7),
    Color::hex(0x56B4E9),
    Color::hex(0xD55E00),
    Color::hex(0xF0E442),
];

/// Anything usable as a single color: `Color`, `"red"`, `"#0072B2"`, `(color, alpha)`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a color",
    label = "expected a Color, a color name like \"red\", a hex string, or (color, alpha)"
)]
pub trait IntoColor {
    fn into_color(self) -> Color;
}

impl IntoColor for Color {
    fn into_color(self) -> Color {
        self
    }
}
impl IntoColor for &Color {
    fn into_color(self) -> Color {
        *self
    }
}
impl IntoColor for &str {
    #[track_caller]
    fn into_color(self) -> Color {
        match Color::parse(self) {
            Ok(c) => c,
            Err(e) => panic!("{e}"),
        }
    }
}
impl IntoColor for String {
    #[track_caller]
    fn into_color(self) -> Color {
        self.as_str().into_color()
    }
}
impl<C: IntoColor> IntoColor for (C, f32) {
    fn into_color(self) -> Color {
        let c = self.0.into_color();
        c.with_alpha(c.a * self.1)
    }
}
impl<C: IntoColor> IntoColor for (C, f64) {
    fn into_color(self) -> Color {
        let c = self.0.into_color();
        c.with_alpha(c.a * self.1 as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_and_names() {
        assert_eq!(Color::parse("#0072B2").unwrap(), WONG[0]);
        assert_eq!(Color::parse("#fff").unwrap(), colors::WHITE);
        assert_eq!(Color::parse("red").unwrap(), colors::RED);
        assert_eq!(Color::parse("gray50").unwrap(), Color::gray(0.5));
        assert!((Color::parse("#00000080").unwrap().a - 128.0 / 255.0).abs() < 1e-6);
        assert!(Color::parse("notacolor").is_err());
    }

    #[test]
    fn premul_packing() {
        assert_eq!(
            Color::rgba(1.0, 0.0, 0.0, 0.5).to_premul_u32(),
            0x80_00_00_80
        );
    }
}
