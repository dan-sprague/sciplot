//! Makie 0.24 tick-label formatting, ported from `Makie/src/tick_format.jl` (a vendored subset of
//! Showoff): `format_ticks_auto`, `format_ticks_plain`, and the scientific `×10ⁿ` labels.
//!
//! Also a small Python/Format.jl-style format-string formatter for `TickFormat::Format`.

use super::julia;
use crate::text::{RichText, TextSpan, superscript};

/// U+2212, used instead of the ASCII hyphen for negative numbers (`MINUS_SIGN` in Makie).
pub(crate) const MINUS: &str = "\u{2212}";

fn replace_leading_hyphen(s: &str) -> String {
    match s.strip_prefix('-') {
        Some(rest) => format!("{MINUS}{rest}"),
        None => s.to_string(),
    }
}

/// The decimal exponent `e10` of the shortest representation `m·10^e10` (integer `m` without
/// trailing zeros) of `y`: Julia's `Base.Ryu.reduce_shortest(y)[2]`.
pub(crate) fn shortest_e10_f32(y: f32) -> i64 {
    if y.is_infinite() {
        // Ryu treats the Inf bit pattern like a finite float and returns (1, 31).
        return 31;
    }
    if y == 0.0 || y.is_nan() {
        return 0;
    }
    let s = format!("{:e}", y.abs());
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let digits = mant.bytes().filter(u8::is_ascii_digit).count() as i64;
    let exp: i64 = exp.parse().unwrap_or(0);
    exp - (digits - 1)
}

/// Makie's `_plain_label_precision(xs)`: the smallest uniform number of decimals that keeps all
/// significant digits (measured on the shortest `Float32` representation, which hides `f64` noise
/// such as `0.30000000000000004`).
pub(crate) fn plain_precision(xs: &[f64]) -> usize {
    let mut e10min = i64::MAX;
    let mut e10max = i64::MIN + 1; // -(typemax(Int))
    let mut any = false;
    for &y in xs {
        if !y.is_finite() {
            continue;
        }
        any = true;
        let e10 = if y.abs() <= 1e-16 { e10min.min(0) } else { shortest_e10_f32(y as f32) };
        e10min = e10min.min(e10);
        e10max = e10max.max(e10);
    }
    if !any {
        return 0;
    }
    (-e10min).min(16 - e10max).max(0) as usize
}

/// `Base.Ryu.writefixed(x, precision)` with the leading hyphen replaced by U+2212.
pub(crate) fn format_plain_label(x: f64, precision: usize) -> String {
    let s = if x.is_finite() {
        format!("{x:.precision$}")
    } else if x.is_nan() {
        "NaN".into()
    } else if x > 0.0 {
        "Inf".into()
    } else {
        "-Inf".into()
    };
    replace_leading_hyphen(&s)
}

/// Makie's `format_ticks_plain(xs)`: fixed notation with a shared precision.
pub fn format_ticks_plain(xs: &[f64]) -> Vec<String> {
    let p = plain_precision(xs);
    xs.iter().map(|&x| format_plain_label(x, p)).collect()
}

fn scientific_precision(xs: &[f64]) -> usize {
    let ys: Vec<f64> = xs
        .iter()
        .filter(|x| x.is_finite())
        .map(|&x| {
            if x == 0.0 {
                0.0
            } else {
                let z = julia::log10(x.abs());
                // Julia's Float64^Float64 (libm here; the result is rounded to 15 digits anyway).
                julia::round_sigdigits(10f64.powf(z - z.floor()), 15)
            }
        })
        .collect();
    plain_precision(&ys)
}

/// `_split_scientific`: `"1.500e-05"` → (`"1.500"`, -5), hyphen replaced.
fn split_scientific(x: f64, precision: usize) -> (String, i64) {
    let s = format!("{x:.precision$e}");
    let (base, exp) = s.split_once('e').unwrap_or((&s, "0"));
    (replace_leading_hyphen(base), exp.parse().unwrap_or(0))
}

fn strip_trailing_zeros(s: &str) -> String {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
}

fn has_only_zero_fraction(base: &str) -> bool {
    match base.split_once('.') {
        None => true,
        Some((_, frac)) => frac.bytes().all(|b| b == b'0'),
    }
}

/// `rich(base, "×10", superscript(exp, offset = (0.1, 0)))`.
fn scientific_rich(base: String, exponent: i64) -> RichText {
    let e = if exponent < 0 { format!("{MINUS}{}", -exponent) } else { exponent.to_string() };
    RichText::from_spans([TextSpan::plain(base), TextSpan::plain("×10"), superscript(e).offset(0.1, 0.0)])
}

/// Whether Makie would label `xs` in scientific notation (`_pick_label_style`): the tick
/// *range* is above 1e4 or below 1e-4.
fn use_scientific(xs: &[f64]) -> bool {
    let Some(&first) = xs.first() else { return false };
    let (mut lo, mut hi) = (first, first);
    for &x in xs {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    hi != lo && julia::log10(hi - lo).abs() > 4.0
}

/// Makie's default tick formatter `format_ticks_auto(xs)`.
///
/// Plain fixed notation with the minimal shared precision (`0.0, 2.5, 5.0`), or `1.5×10⁻⁵`-style
/// labels when the tick range is above 1e4 or below 1e-4. Negative numbers use U+2212.
pub fn format_ticks_auto(xs: &[f64]) -> Vec<RichText> {
    if !use_scientific(xs) {
        return format_ticks_plain(xs).into_iter().map(RichText::from).collect();
    }
    let p = scientific_precision(xs);
    let pairs: Vec<Option<(String, i64)>> =
        xs.iter().map(|&x| (x != 0.0 && x.is_finite()).then(|| split_scientific(x, p))).collect();
    let can_strip = pairs.iter().all(|p| p.as_ref().is_none_or(|(b, _)| has_only_zero_fraction(b)));
    xs.iter()
        .zip(pairs)
        .map(|(&x, p)| match p {
            None if x == 0.0 => RichText::from("0"),
            None => RichText::from(format_plain_label(x, 0)),
            Some((base, e)) => scientific_rich(if can_strip { strip_trailing_zeros(&base) } else { base }, e),
        })
        .collect()
}

/// Log-axis label: `rich(base, superscript(exponent, offset = (0.1, 0)))`, e.g. "10" + "−2".
pub(crate) fn log_label(base: &str, exponent: &str) -> RichText {
    RichText::from_spans([TextSpan::plain(base), superscript(exponent).offset(0.1, 0.0)])
}

// ---------------------------------------------------------------------------------------------
// Format strings

/// Formats `v` with a Format.jl / Python style format string: literal text with `{}` or
/// `{:spec}` placeholders, where spec is `[+][0][width][,][.precision][type]` and type is one of
/// `f`, `e`, `E`, `g`, `G`, `d`, `%` (default: shortest representation). `{{`/`}}` escape braces.
pub fn format_with(fmt: &str, v: f64) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut inner = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    inner.push(c);
                }
                let spec = inner.split_once(':').map_or("", |(_, s)| s);
                out.push_str(&format_spec(spec, v));
            }
            c => out.push(c),
        }
    }
    out
}

fn format_spec(spec: &str, v: f64) -> String {
    let mut s = spec;
    let plus = s.starts_with('+');
    if plus || s.starts_with('-') || s.starts_with(' ') {
        s = &s[1..];
    }
    let zero = s.starts_with('0');
    let width_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let width: usize = s[..width_end].parse().unwrap_or(0);
    s = &s[width_end..];
    let comma = s.starts_with(',');
    if comma {
        s = &s[1..];
    }
    let mut precision = None;
    if let Some(rest) = s.strip_prefix('.') {
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        precision = rest[..end].parse::<usize>().ok();
        s = &rest[end..];
    }
    let ty = s.chars().next();
    let body = match ty {
        Some('f') | Some('F') => format!("{:.*}", precision.unwrap_or(6), v.abs()),
        Some('%') => format!("{:.*}%", precision.unwrap_or(6), v.abs() * 100.0),
        Some('e') | Some('E') => {
            let t = python_exp(v.abs(), precision.unwrap_or(6));
            if ty == Some('E') { t.to_uppercase() } else { t }
        }
        Some('g') | Some('G') => {
            let t = python_general(v.abs(), precision.unwrap_or(6));
            if ty == Some('G') { t.to_uppercase() } else { t }
        }
        Some('d') => format!("{:.0}", v.abs()),
        _ => match precision {
            Some(p) => python_general(v.abs(), p.max(1)),
            None => shortest(v.abs()),
        },
    };
    let body = if comma { group_thousands(&body) } else { body };
    let sign = if v.is_sign_negative() && v != 0.0 {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    let len = sign.chars().count() + body.chars().count();
    if len >= width {
        format!("{sign}{body}")
    } else if zero {
        format!("{sign}{}{body}", "0".repeat(width - len))
    } else {
        format!("{}{sign}{body}", " ".repeat(width - len))
    }
}

/// Julia's `string(x)` for floats: shortest round-trip digits, `1.0` for integers.
fn shortest(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() { "NaN".into() } else { "Inf".into() };
    }
    let a = v.abs();
    if a != 0.0 && !(1e-5..1e16).contains(&a) {
        let s = format!("{v:e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
        return format!("{m}e{e}");
    }
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// Python's `{:.pe}`: at least two exponent digits.
fn python_exp(v: f64, p: usize) -> String {
    let s = format!("{v:.p$e}");
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i64 = e.parse().unwrap_or(0);
    format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// Python's `{:.pg}`.
fn python_general(v: f64, p: usize) -> String {
    let p = p.max(1);
    if v == 0.0 {
        return "0".into();
    }
    let e = format!("{v:.*e}", p - 1);
    let exp: i64 = e.split_once('e').and_then(|(_, x)| x.parse().ok()).unwrap_or(0);
    if exp < -4 || exp >= p as i64 {
        let s = python_exp(v, p - 1);
        let (m, rest) = s.split_once('e').unwrap_or((&s, ""));
        format!("{}e{rest}", strip_trailing_zeros(m))
    } else {
        strip_trailing_zeros(&format!("{v:.*}", (p as i64 - 1 - exp).max(0) as usize))
    }
}

fn group_thousands(s: &str) -> String {
    let (int, rest) = s.find(|c: char| !c.is_ascii_digit()).map_or((s, ""), |i| s.split_at(i));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out + rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(xs: &[f64]) -> Vec<String> {
        format_ticks_auto(xs).iter().map(|r| r.plain_text()).collect()
    }

    #[test]
    fn makie_examples() {
        assert_eq!(plain(&[0.0, 2.5, 5.0, 7.5, 10.0]), ["0.0", "2.5", "5.0", "7.5", "10.0"]);
        assert_eq!(plain(&[0.0, 5.0, 10.0]), ["0", "5", "10"]);
        assert_eq!(plain(&[-1.0, -0.5, 0.0]), ["\u{2212}1.0", "\u{2212}0.5", "0.0"]);
        assert_eq!(plain(&[0.1, 0.2, 0.30000000000000004]), ["0.1", "0.2", "0.3"]);
        assert_eq!(plain(&[1.5e-5, 2e-5, 2.5e-5]), ["1.5×10\u{2212}5", "2.0×10\u{2212}5", "2.5×10\u{2212}5"]);
        assert_eq!(plain(&[0.0, 5e5, 1e6]), ["0", "5×105", "1×106"]);
    }

    #[test]
    fn format_strings() {
        assert_eq!(format_with("{:.2f}", 1.23456), "1.23");
        assert_eq!(format_with("{:.1f} ms", -2.0), "-2.0 ms");
        assert_eq!(format_with("{:.1e}", 12345.0), "1.2e+04");
        assert_eq!(format_with("{:d}", 7.0), "7");
        assert_eq!(format_with("{:.0f}%", 50.0), "50%");
        assert_eq!(format_with("{:,.0f}", 1234567.0), "1,234,567");
        assert_eq!(format_with("{}", 2.0), "2.0");
        assert_eq!(format_with("{:05.1f}", 2.5), "002.5");
    }
}
