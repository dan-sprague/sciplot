//! Tick selection and tick-label formatting.
//!
//! Interface used by the scene builder:
//! - [`major_ticks`] picks major tick values (data space) and their labels for a visible range,
//! - [`minor_ticks`] picks minor tick values between majors.
//!
//! (M1 placeholder: a simple "nice numbers" step search. M3 replaces it with a literal port of
//! PlotUtils' Wilkinson `optimize_ticks` and Makie's label formatting.)

use crate::text::{RichText, TextSpan};
use crate::transform::Scale;

/// Major ticks: values in data space and their labels.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ticks {
    pub values: Vec<f64>,
    pub labels: Vec<RichText>,
}

/// Automatic major ticks for the visible data range `[lo, hi]` (either order).
pub(crate) fn major_ticks(lo: f64, hi: f64, scale: Scale) -> Ticks {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return Ticks::default();
    }
    let values = if scale.is_log() {
        let (a, b) = (scale.forward(lo), scale.forward(hi));
        let vals: Vec<f64> = (a.ceil() as i64..=b.floor() as i64)
            .map(|e| scale.inverse(e as f64))
            .collect();
        if vals.len() >= 2 { vals } else { nice(lo, hi) }
    } else {
        nice(lo, hi)
    };
    let labels = format_ticks(&values);
    Ticks { values, labels }
}

fn nice(lo: f64, hi: f64) -> Vec<f64> {
    let span = hi - lo;
    let raw = span / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| span / s <= 7.0)
        .unwrap_or(10.0 * mag);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    (first..=last).map(|i| i as f64 * step).collect()
}

/// Labels with the minimal number of decimals that keeps them distinct.
pub(crate) fn format_ticks(values: &[f64]) -> Vec<RichText> {
    let mut dec = 0;
    while dec < 10 {
        let s: Vec<String> = values.iter().map(|v| format!("{:.*}", dec, v)).collect();
        let exact = values.iter().zip(&s).all(|(v, t)| {
            (t.parse::<f64>().unwrap_or(f64::NAN) - v).abs() <= 1e-9 * v.abs().max(1e-300)
        });
        if exact {
            break;
        }
        dec += 1;
    }
    values
        .iter()
        .map(|v| {
            let s = format!("{:.*}", dec, v);
            let s = if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() {
                s[1..].to_string()
            } else {
                s
            };
            RichText::from(TextSpan::plain(s.replace('-', "\u{2212}")))
        })
        .collect()
}

/// Minor ticks between majors (`n` intervals per major interval).
pub(crate) fn minor_ticks(major: &[f64], lo: f64, hi: f64, scale: Scale, n: usize) -> Vec<f64> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let mut out = Vec::new();
    if scale.is_log() {
        let (a, b) = (
            scale.forward(lo).floor() as i64,
            scale.forward(hi).ceil() as i64,
        );
        for e in a..b {
            let base = scale.inverse(e as f64);
            for k in 2..10 {
                let v = base * k as f64;
                if v >= lo && v <= hi {
                    out.push(v);
                }
            }
        }
        return out;
    }
    if major.len() < 2 || n < 2 {
        return out;
    }
    let step = major[1] - major[0];
    let sub = step / n as f64;
    let start = major[0] - step;
    let mut i = 0;
    loop {
        let v = start + i as f64 * sub;
        if v > hi + 1e-12 * step.abs() {
            break;
        }
        if v >= lo && i % n != 0 {
            out.push(v);
        }
        i += 1;
    }
    out
}
