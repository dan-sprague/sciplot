//! Log-axis major ticks.
//!
//! - [`log_makie`]: Makie's `LogTicks(WilkinsonTicks(5, k_min = 3))`, exact. Wilkinson runs on
//!   the exponents, so the ticks can land on `10^0.5`.
//! - [`log_integer`]: the sciplot default (plan deviation D1). Majors sit on whole decades
//!   whenever at least two of them are visible; Wilkinson picks the decade step.

use super::format::{format_ticks_plain, log_label};
use super::julia;
use super::wilkinson::{Wilkinson, optimize_ticks};
use crate::text::RichText;
use crate::transform::Scale;

/// The log base of a scale, for the Julia-exact forward/inverse transforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LogBase {
    Ten,
    Two,
    E,
}

impl LogBase {
    pub(crate) fn of(scale: Scale) -> Option<LogBase> {
        match scale {
            Scale::Log10 => Some(LogBase::Ten),
            Scale::Log2 => Some(LogBase::Two),
            Scale::Ln => Some(LogBase::E),
            _ => None,
        }
    }
    /// Julia's `log10`/`log2`/`log`.
    pub(crate) fn forward(self, v: f64) -> f64 {
        match self {
            LogBase::Ten => julia::log10(v),
            LogBase::Two => julia::log2(v),
            LogBase::E => julia::ln(v),
        }
    }
    /// Julia's `exp10`/`exp2`/`exp` (Makie's `inverse_transform`).
    pub(crate) fn inverse(self, v: f64) -> f64 {
        match self {
            LogBase::Ten => julia::exp10(v),
            LogBase::Two => julia::exp2(v),
            LogBase::E => julia::exp(v),
        }
    }
    /// `base^n` for an integer `n`, correctly rounded for base 10 (`1e23`, not Julia's
    /// `exp10(23.0) == 1.0000000000000001e23`).
    pub(crate) fn int_pow(self, n: i64) -> f64 {
        match self {
            LogBase::Ten => format!("1e{n}").parse().unwrap_or(f64::NAN),
            LogBase::Two => julia::powi(2.0, n),
            LogBase::E => julia::exp(n as f64),
        }
    }
    /// Makie's `_logbase`: the label base string.
    pub(crate) fn label(self) -> &'static str {
        match self {
            LogBase::Ten => "10",
            LogBase::Two => "2",
            LogBase::E => "e",
        }
    }
}

/// Makie's `get_ticks(LogTicks(p), scale, Automatic(), vmin, vmax)`: values and `10ⁿ` labels.
pub(crate) fn log_makie(p: &Wilkinson, vmin: f64, vmax: f64, base: LogBase) -> (Vec<f64>, Vec<RichText>) {
    let scaled = p.tick_values(base.forward(vmin), base.forward(vmax));
    let values = scaled.iter().map(|&e| base.inverse(e)).collect();
    let labels = format_ticks_plain(&scaled).iter().map(|e| log_label(base.label(), e)).collect();
    (values, labels)
}

/// Integer-decade log ticks (D1): Wilkinson on the exponents, restricted to whole-number steps.
/// `None` when fewer than two whole decades are inside `[vmin, vmax]`; the caller then falls back
/// to linear Wilkinson ticks with plain labels.
///
/// Returns the values and the integer exponents.
pub(crate) fn log_integer(vmin: f64, vmax: f64, base: LogBase) -> Option<(Vec<f64>, Vec<f64>)> {
    let (a, b) = (base.forward(vmin), base.forward(vmax));
    if !(a.is_finite() && b.is_finite()) || b.floor() - a.ceil() < 1.0 {
        return None;
    }
    // Makie's k_min = 3, relaxed to 2 when only two whole decades are visible.
    let k_min = if b.floor() - a.ceil() >= 2.0 { 3 } else { 2 };
    let exps = optimize_ticks(a, b, &Wilkinson::default().k_min(k_min), true);
    if exps.len() < 2 {
        return None;
    }
    let values = exps.iter().map(|&e| base.int_pow(e as i64)).collect();
    Some((values, exps))
}

/// Labels `base^e` for integer exponents, formatted like Makie's `LogTicks` labels.
pub(crate) fn exponent_labels(exps: &[f64], base: LogBase) -> Vec<RichText> {
    format_ticks_plain(exps).iter().map(|e| log_label(base.label(), e)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_decades() {
        let (v, e) = log_integer(0.8, 1200.0, LogBase::Ten).unwrap();
        assert_eq!(v, vec![1.0, 10.0, 100.0, 1000.0]);
        assert_eq!(e, vec![0.0, 1.0, 2.0, 3.0]);
        let (v, _) = log_integer(1e-300, 1e300, LogBase::Ten).unwrap();
        assert!(v.len() >= 3 && v.len() <= 10, "{v:?}");
        assert!(v.iter().all(|x| format!("{x:e}").starts_with("1e")), "{v:?}");
        // Less than two whole decades: linear fallback.
        assert!(log_integer(2.0, 30.0, LogBase::Ten).is_none());
        let labels = exponent_labels(&[-2.0, 0.0], LogBase::Ten);
        assert_eq!(labels[0].spans[1].text, "\u{2212}2");
        assert_eq!(labels[0].spans[0].text, "10");
    }
}
