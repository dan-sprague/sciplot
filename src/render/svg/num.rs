//! Byte-stable number and color formatting for SVG output.

use crate::color::Color;
use std::fmt::{self, Write as _};

/// A number with at most 3 decimals, trailing zeros trimmed, no `-0` (non-finite values print
/// as `0`; callers filter them before).
#[derive(Clone, Copy)]
pub(crate) struct N(pub f64);

impl fmt::Display for N {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = if self.0.is_finite() { self.0 } else { 0.0 };
        let r = (v * 1000.0).round();
        if r == 0.0 {
            return f.write_str("0");
        }
        if r.abs() >= 9.0e15 {
            // Beyond exact integers: no decimals left anyway.
            return write!(f, "{}", v.round());
        }
        if r < 0.0 {
            f.write_char('-')?;
        }
        let a = r.abs() as u64;
        write!(f, "{}", a / 1000)?;
        let mut frac = a % 1000;
        if frac != 0 {
            let mut digits = 3;
            while frac.is_multiple_of(10) {
                frac /= 10;
                digits -= 1;
            }
            write!(f, ".{frac:0digits$}")?;
        }
        Ok(())
    }
}

/// `#rrggbb` of an sRGB color (alpha ignored).
pub(crate) fn hex(c: Color) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Writes ` {attr}="#rrggbb"` plus ` {attr}-opacity=".."` for translucent colors.
pub(crate) fn paint(out: &mut String, attr: &str, c: Color) {
    let _ = write!(out, " {attr}=\"{}\"", hex(c));
    let a = c.a.clamp(0.0, 1.0) as f64;
    if a < 1.0 {
        let _ = write!(out, " {attr}-opacity=\"{}\"", N(a));
    }
}

/// Straight-alpha color from a premultiplied RGBA8 value packed with r in the low byte.
pub(crate) fn unpremul_u32(v: u32) -> Color {
    let [r, g, b, a] = v.to_le_bytes();
    if a == 0 {
        return Color::TRANSPARENT;
    }
    let un = |x: u8| (x as f32 / a as f32).min(1.0);
    Color::rgba(un(r), un(g), un(b), a as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        let s = |v: f64| N(v).to_string();
        assert_eq!(s(0.0), "0");
        assert_eq!(s(-0.0001), "0");
        assert_eq!(s(1.5), "1.5");
        assert_eq!(s(-2.25), "-2.25");
        assert_eq!(s(600.0), "600");
        assert_eq!(s(0.1 + 0.2), "0.3");
        assert_eq!(s(1.0005), "1.001");
        assert_eq!(s(0.05), "0.05");
        assert_eq!(s(-0.007), "-0.007");
        assert_eq!(s(12345.6789), "12345.679");
        assert_eq!(s(f64::NAN), "0");
    }

    #[test]
    fn colors() {
        assert_eq!(hex(Color::hex(0x0072B2)), "#0072b2");
        let mut o = String::new();
        paint(&mut o, "fill", Color::rgba(1.0, 0.0, 0.0, 0.5));
        assert_eq!(o, " fill=\"#ff0000\" fill-opacity=\"0.5\"");
        let c = unpremul_u32(Color::rgba(1.0, 0.0, 0.0, 0.5).to_premul_u32());
        assert!((c.r - 1.0).abs() < 0.01 && (c.a - 0.5).abs() < 0.01);
    }
}
