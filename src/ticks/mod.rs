//! Tick selection and tick-label formatting, ported from Makie 0.24 / PlotUtils 1.5.
//!
//! - [`wilkinson`]: PlotUtils' `optimize_ticks` (Makie's default `WilkinsonTicks(5; k_min = 3)`).
//! - [`format`]: Makie's `format_ticks_auto` (plain or `×10ⁿ` labels, U+2212 minus).
//! - [`log`]: log-axis majors: integer decades by default (plan D1), or Makie-exact `LogTicks`.
//! - [`minor`]: `IntervalsBetween(n)` and the log-axis `2..9·10ⁿ` minors (plan D2).
//! - [`julia`]: bit-exact ports of the Julia numerics these rely on.
//!
//! Axis attributes take the public spec types [`TickSpec`], [`MinorSpec`] and [`TickFormat`];
//! the scene builder calls [`resolve_ticks`] and [`resolve_minor`].
//!
//! Provenance: tick resolution (`raw_ticks`, `resolve_ticks`, `resolve_minor`, `is_within_limits`,
//! `TickFormat::labels`) is adapted from Makie 0.24.14 `src/makielayout/lineaxis.jl` (`get_ticks`,
//! `get_tickvalues`, `get_ticklabels`, `get_minor_tickvalues`, `is_within_limits`). The spec
//! defaults follow the `Axis` attributes in `src/makielayout/types.jl` (`xticks`, `xtickformat`,
//! `xminorticks = IntervalsBetween(2)`). `TickSpec::LogInteger` and the log-decade
//! `MinorSpec::Auto` are sciplot's own. MIT licensed; see THIRD_PARTY_NOTICES.md.

pub(crate) mod format;
pub(crate) mod julia;
mod julia_tables;
pub(crate) mod log;
pub(crate) mod minor;
pub(crate) mod wilkinson;

use std::fmt;
use std::sync::Arc;

use crate::attrs::Conv;
use crate::data::Num;
use crate::text::RichText;
use crate::transform::Scale;

pub use format::{format_ticks_auto, format_with};
use log::LogBase;
pub use wilkinson::Wilkinson;

/// Major ticks: values in data space and their labels.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ticks {
    pub values: Vec<f64>,
    pub labels: Vec<RichText>,
}

/// A user tick function: `(vmin, vmax) -> values`.
pub type TickFn = Arc<dyn Fn(f64, f64) -> Vec<f64> + Send + Sync>;
/// A user label function: `value -> label`.
pub type LabelFn = Arc<dyn Fn(f64) -> RichText + Send + Sync>;

/// Where the major ticks go (Makie's `xticks`/`yticks`).
///
/// Converts from plain values (`xticks = [0.0, 1.0, 2.0]`), `(values, labels)` pairs
/// (`xticks = (vec![1.0, 2.0], vec!["a", "b"])`), a [`Wilkinson`] and closures
/// `|vmin: f64, vmax: f64| -> Vec<f64>`.
#[derive(Clone, Default)]
pub enum TickSpec {
    /// Makie's `automatic`: `WilkinsonTicks(5; k_min = 3)` on linear axes, [`TickSpec::LogInteger`]
    /// on log axes.
    #[default]
    Automatic,
    /// Wilkinson's algorithm with custom parameters, on data values (also on log axes, like Makie).
    Wilkinson(Wilkinson),
    /// Fixed tick values, labelled by the tick format.
    Values(Vec<f64>),
    /// Fixed tick values with their own labels (the tick format is ignored).
    Labeled(Vec<f64>, Vec<RichText>),
    /// Log axes: majors on whole powers of the base (`10ⁿ` labels) whenever at least two are
    /// visible, with a Wilkinson-chosen decade step; otherwise linear Wilkinson ticks with plain
    /// labels. Same as `Automatic` on non-log axes.
    LogInteger,
    /// Log axes: Makie's exact `LogTicks(WilkinsonTicks(5; k_min = 3))`, which runs Wilkinson on
    /// the exponents and can produce `10^0.5`. Same as `Automatic` on non-log axes.
    LogMakie,
    /// A function `(vmin, vmax) -> values`, labelled by the tick format.
    Func(TickFn),
}

impl fmt::Debug for TickSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TickSpec::Automatic => f.write_str("Automatic"),
            TickSpec::Wilkinson(w) => f.debug_tuple("Wilkinson").field(w).finish(),
            TickSpec::Values(v) => f.debug_tuple("Values").field(v).finish(),
            TickSpec::Labeled(v, l) => f.debug_tuple("Labeled").field(v).field(l).finish(),
            TickSpec::LogInteger => f.write_str("LogInteger"),
            TickSpec::LogMakie => f.write_str("LogMakie"),
            TickSpec::Func(_) => f.write_str("Func(..)"),
        }
    }
}

/// Where the minor ticks go (Makie's `xminorticks`/`yminorticks`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum MinorSpec {
    /// Makie's `IntervalsBetween(2)` on linear axes. On log axes with majors on whole decades:
    /// `2..9·10ⁿ` (or the skipped decades when the majors skip some); otherwise
    /// `IntervalsBetween(2)`.
    #[default]
    Auto,
    /// Makie's `IntervalsBetween(n)`: `n - 1` minor ticks per major interval, continued past the
    /// first and last major. `n < 2` gives no minor ticks.
    IntervalsBetween(usize),
    /// Fixed minor tick values.
    Values(Vec<f64>),
}

/// How tick values become labels (Makie's `xtickformat`/`ytickformat`).
///
/// Converts from a format string (`xtickformat = "{:.2f}"`) and closures
/// `|v: f64| -> impl Into<RichText>` (`xtickformat = |v: f64| format!("{v:.1}")`).
#[derive(Clone, Default)]
pub enum TickFormat {
    /// Makie's `format_ticks_auto`; `10ⁿ` labels for log-axis decade ticks.
    #[default]
    Automatic,
    /// One label per value.
    Func(LabelFn),
    /// A Format.jl / Python-style format string, see [`format_with`].
    Format(String),
}

impl fmt::Debug for TickFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TickFormat::Automatic => f.write_str("Automatic"),
            TickFormat::Func(_) => f.write_str("Func(..)"),
            TickFormat::Format(s) => f.debug_tuple("Format").field(s).finish(),
        }
    }
}

impl TickFormat {
    /// Labels for `values` (Makie's `get_ticklabels`).
    pub fn labels(&self, values: &[f64]) -> Vec<RichText> {
        match self {
            TickFormat::Automatic => format_ticks_auto(values),
            TickFormat::Func(f) => values.iter().map(|&v| f(v)).collect(),
            TickFormat::Format(s) => values.iter().map(|&v| RichText::from(format_with(s, v))).collect(),
        }
    }
    fn is_automatic(&self) -> bool {
        matches!(self, TickFormat::Automatic)
    }
}

impl TickSpec {
    /// The major ticks inside `[lo, hi]` (either order) on an axis with `scale`: values in data
    /// space and their labels.
    pub fn compute(&self, format: &TickFormat, lo: f64, hi: f64, scale: Scale) -> (Vec<f64>, Vec<RichText>) {
        let t = resolve_ticks(self, format, lo, hi, scale);
        (t.values, t.labels)
    }
}

impl MinorSpec {
    /// The minor ticks inside `[lo, hi]` (either order) for the given major tick values.
    pub fn compute(&self, major: &[f64], lo: f64, hi: f64, scale: Scale) -> Vec<f64> {
        resolve_minor(self, major, lo, hi, scale)
    }
}

/// Makie's `is_within_limits`: inside `[lo, hi]` up to 100 ulps.
fn is_within_limits(v: f64, lo: f64, hi: f64) -> bool {
    lo - 100.0 * julia::eps(lo) < v && v < hi + 100.0 * julia::eps(hi)
}

fn sorted(lo: f64, hi: f64) -> (f64, f64) {
    if lo <= hi { (lo, hi) } else { (hi, lo) }
}

/// Unfiltered values and (if the spec provides them) labels, like Makie's `get_ticks`.
fn raw_ticks(
    spec: &TickSpec,
    format: &TickFormat,
    lo: f64,
    hi: f64,
    scale: Scale,
) -> (Vec<f64>, Option<Vec<RichText>>) {
    let base = LogBase::of(scale);
    let linear = || (Wilkinson::default().tick_values(lo, hi), None);
    let log_integer = |b: LogBase| match log::log_integer(lo, hi, b) {
        Some((values, exps)) => {
            let labels = format.is_automatic().then(|| log::exponent_labels(&exps, b));
            (values, labels)
        }
        None => linear(),
    };
    match spec {
        TickSpec::Automatic | TickSpec::LogInteger => match base {
            Some(b) => log_integer(b),
            None => linear(),
        },
        TickSpec::LogMakie => match base {
            Some(b) => {
                let (values, labels) = log::log_makie(&Wilkinson::default(), lo, hi, b);
                (values, format.is_automatic().then_some(labels))
            }
            None => linear(),
        },
        TickSpec::Wilkinson(w) => (w.tick_values(lo, hi), None),
        TickSpec::Values(v) => (v.clone(), None),
        TickSpec::Labeled(v, l) => {
            let n = v.len().min(l.len());
            (v[..n].to_vec(), Some(l[..n].to_vec()))
        }
        TickSpec::Func(f) => (f(lo, hi), None),
    }
}

/// Major ticks for the visible range `[lo, hi]` (either order): values in data space and labels,
/// filtered to the limits like Makie (labels are formatted before filtering, as in Makie).
pub(crate) fn resolve_ticks(spec: &TickSpec, format: &TickFormat, lo: f64, hi: f64, scale: Scale) -> Ticks {
    let (lo, hi) = sorted(lo, hi);
    if !(lo.is_finite() && hi.is_finite()) {
        return Ticks::default();
    }
    let (values, labels) = raw_ticks(spec, format, lo, hi, scale);
    let labels = labels.unwrap_or_else(|| format.labels(&values));
    let (values, labels) = values.into_iter().zip(labels).filter(|(v, _)| is_within_limits(*v, lo, hi)).unzip();
    Ticks { values, labels }
}

/// Minor tick values inside `[lo, hi]` (either order) for the (filtered) major tick values.
pub(crate) fn resolve_minor(spec: &MinorSpec, major: &[f64], lo: f64, hi: f64, scale: Scale) -> Vec<f64> {
    let (lo, hi) = sorted(lo, hi);
    if !(lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    let base = LogBase::of(scale);
    let raw = match spec {
        MinorSpec::Auto => match base {
            Some(b) => minor::log_decade_minors(major, lo, hi, b)
                .unwrap_or_else(|| minor::intervals_between(2, true, Some(b), major, lo, hi)),
            None => minor::intervals_between(2, true, None, major, lo, hi),
        },
        MinorSpec::IntervalsBetween(n) => minor::intervals_between(*n, true, base, major, lo, hi),
        MinorSpec::Values(v) => v.clone(),
    };
    raw.into_iter().filter(|&v| is_within_limits(v, lo, hi)).collect()
}

/// Automatic major ticks for the visible data range `[lo, hi]` (either order).
pub(crate) fn major_ticks(lo: f64, hi: f64, scale: Scale) -> Ticks {
    resolve_ticks(&TickSpec::Automatic, &TickFormat::Automatic, lo, hi, scale)
}

/// Makie's default labels for tick values (`format_ticks_auto`).
pub(crate) fn format_ticks(values: &[f64]) -> Vec<RichText> {
    format_ticks_auto(values)
}

/// Minor ticks: `IntervalsBetween(n)` on linear axes, [`MinorSpec::Auto`] on log axes.
pub(crate) fn minor_ticks(major: &[f64], lo: f64, hi: f64, scale: Scale, n: usize) -> Vec<f64> {
    let spec = if scale.is_log() { MinorSpec::Auto } else { MinorSpec::IntervalsBetween(n) };
    resolve_minor(&spec, major, lo, hi, scale)
}

// ---------------------------------------------------------------------------------------------
// Attribute conversions

crate::attrs::conv_identity!(TickSpec, MinorSpec, TickFormat);

fn to_f64s<N: Num>(v: impl IntoIterator<Item = N>) -> Vec<f64> {
    v.into_iter().map(|x| x.to_f64()).collect()
}

impl<N: Num> Conv<TickSpec> for Vec<N> {
    fn conv(self) -> TickSpec {
        TickSpec::Values(to_f64s(self))
    }
}
impl<N: Num + Copy> Conv<TickSpec> for &[N] {
    fn conv(self) -> TickSpec {
        TickSpec::Values(to_f64s(self.iter().copied()))
    }
}
impl<N: Num + Copy> Conv<TickSpec> for &Vec<N> {
    fn conv(self) -> TickSpec {
        TickSpec::Values(to_f64s(self.iter().copied()))
    }
}
impl<N: Num, const K: usize> Conv<TickSpec> for [N; K] {
    fn conv(self) -> TickSpec {
        TickSpec::Values(to_f64s(self))
    }
}
/// Integer ranges like Makie's `xticks = 0:2:10`: `0..=10`, `(0..=10).step_by(2)`.
macro_rules! int_range_ticks {
    ($($t:ty),*) => {$(
        impl Conv<TickSpec> for std::ops::RangeInclusive<$t> {
            fn conv(self) -> TickSpec {
                TickSpec::Values(to_f64s(self))
            }
        }
        impl Conv<TickSpec> for std::iter::StepBy<std::ops::RangeInclusive<$t>> {
            fn conv(self) -> TickSpec {
                TickSpec::Values(to_f64s(self))
            }
        }
    )*};
}
int_range_ticks!(i32, i64, usize);

#[track_caller]
fn labeled(values: Vec<f64>, labels: Vec<RichText>) -> TickSpec {
    assert_eq!(values.len(), labels.len(), "there are {} tick values but {} tick labels", values.len(), labels.len());
    TickSpec::Labeled(values, labels)
}

/// `(values, labels)`; panics if the lengths differ (Makie errors too).
impl<N: Num, S: Into<RichText>> Conv<TickSpec> for (Vec<N>, Vec<S>) {
    #[track_caller]
    fn conv(self) -> TickSpec {
        labeled(to_f64s(self.0), self.1.into_iter().map(Into::into).collect())
    }
}
/// `(values, labels)` arrays; panics if the lengths differ.
impl<N: Num, S: Into<RichText>, const K: usize, const L: usize> Conv<TickSpec> for ([N; K], [S; L]) {
    #[track_caller]
    fn conv(self) -> TickSpec {
        labeled(to_f64s(self.0), self.1.into_iter().map(Into::into).collect())
    }
}
/// `(values, labels)` slices; panics if the lengths differ.
impl<N: Num + Copy, S: Into<RichText> + Clone> Conv<TickSpec> for (&[N], &[S]) {
    #[track_caller]
    fn conv(self) -> TickSpec {
        labeled(to_f64s(self.0.iter().copied()), self.1.iter().cloned().map(Into::into).collect())
    }
}
impl Conv<TickSpec> for Wilkinson {
    fn conv(self) -> TickSpec {
        TickSpec::Wilkinson(self)
    }
}
/// A tick function `(vmin, vmax) -> values`.
impl<F: Fn(f64, f64) -> Vec<f64> + Send + Sync + 'static> Conv<TickSpec> for F {
    fn conv(self) -> TickSpec {
        TickSpec::Func(Arc::new(self))
    }
}

impl<N: Num> Conv<MinorSpec> for Vec<N> {
    fn conv(self) -> MinorSpec {
        MinorSpec::Values(to_f64s(self))
    }
}
impl<N: Num + Copy> Conv<MinorSpec> for &[N] {
    fn conv(self) -> MinorSpec {
        MinorSpec::Values(to_f64s(self.iter().copied()))
    }
}
impl<N: Num, const K: usize> Conv<MinorSpec> for [N; K] {
    fn conv(self) -> MinorSpec {
        MinorSpec::Values(to_f64s(self))
    }
}

impl Conv<TickFormat> for &str {
    fn conv(self) -> TickFormat {
        TickFormat::Format(self.to_string())
    }
}
impl Conv<TickFormat> for String {
    fn conv(self) -> TickFormat {
        TickFormat::Format(self)
    }
}
/// A label function `value -> label`.
impl<F: Fn(f64) -> R + Send + Sync + 'static, R: Into<RichText>> Conv<TickFormat> for F {
    fn conv(self) -> TickFormat {
        TickFormat::Func(Arc::new(move |v| self(v).into()))
    }
}

// ---------------------------------------------------------------------------------------------

/// Internals exposed for the fixture tests (`tests/ticks.rs`). Not a stable API.
#[doc(hidden)]
pub mod testing {
    use super::*;

    /// Makie's `get_minor_tickvalues(IntervalsBetween(n), scale, ticks, vmin, vmax)`, unfiltered.
    pub fn intervals_between(n: usize, scale: Scale, ticks: &[f64], vmin: f64, vmax: f64) -> Vec<f64> {
        minor::intervals_between(n, true, LogBase::of(scale), ticks, vmin, vmax)
    }
    /// Makie's `get_ticks(LogTicks(WilkinsonTicks(5; k_min = 3)), scale, Automatic(), vmin, vmax)`.
    pub fn log_makie(vmin: f64, vmax: f64, scale: Scale) -> Option<(Vec<f64>, Vec<RichText>)> {
        Some(log::log_makie(&Wilkinson::default(), vmin, vmax, LogBase::of(scale)?))
    }
    /// Julia's `collect(start:step:stop)`.
    pub fn colon(start: f64, step: f64, stop: f64) -> Option<Vec<f64>> {
        julia::colon(start, step, stop).map(|r| r.collect())
    }
    /// Julia's `collect(range(start, stop; length = len))`.
    pub fn linspace(start: f64, stop: f64, len: i64) -> Option<Vec<f64>> {
        julia::linspace(start, stop, len).map(|r| r.collect())
    }
    /// Julia's `10.0^n`.
    pub fn powi10(n: i64) -> f64 {
        julia::powi(10.0, n)
    }
    /// Julia's `log10`, `log2`, `log`, `exp10`, `exp2`, `exp` (by name).
    pub fn math(f: &str, x: f64) -> f64 {
        match f {
            "log10" => julia::log10(x),
            "log2" => julia::log2(x),
            "log" => julia::ln(x),
            "exp10" => julia::exp10(x),
            "exp2" => julia::exp2(x),
            "exp" => julia::exp(x),
            _ => f64::NAN,
        }
    }
    /// Julia's `Base.Ryu.reduce_shortest(Float32(x))[2]`.
    pub fn shortest_e10(x: f64) -> i64 {
        format::shortest_e10_f32(x as f32)
    }
    /// Julia's `round(x; sigdigits = n)`.
    pub fn round_sigdigits(x: f64, n: i64) -> f64 {
        julia::round_sigdigits(x, n)
    }
}
