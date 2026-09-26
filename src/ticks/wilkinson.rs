//! Literal port of PlotUtils 1.5.0 `optimize_ticks` (Wilkinson's extended labeling, via Gadfly),
//! `~/.julia/packages/PlotUtils/J9gzB/src/ticks.jl:9-349`, with Makie's arguments
//! (`extend_ticks = false`, `strict_span = true`, `span_buffer = nothing`).
//!
//! Loop order, strict comparisons and every `Float64` operation follow the Julia source, so near
//! ties are broken exactly like Makie.

use super::julia::{self, Mode};

/// Wilkinson tick-finder parameters (Makie's `WilkinsonTicks`).
///
/// [`Wilkinson::default`] is what a Makie `Axis` uses for automatic ticks:
/// `WilkinsonTicks(5; k_min = 3)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Wilkinson {
    /// Ideal number of ticks.
    pub k_ideal: usize,
    /// Minimum number of ticks.
    pub k_min: usize,
    /// Maximum number of ticks.
    pub k_max: usize,
    /// Nice step mantissas with their niceness score, in preference order.
    pub q: Vec<(f64, f64)>,
    /// Weight of hitting `k_ideal`.
    pub granularity_weight: f64,
    /// Weight of simple steps and of including zero.
    pub simplicity_weight: f64,
    /// Weight of covering the range tightly.
    pub coverage_weight: f64,
    /// Weight of the step's niceness score.
    pub niceness_weight: f64,
}

impl Default for Wilkinson {
    fn default() -> Self {
        Wilkinson { k_min: 3, ..Wilkinson::new(5) }
    }
}

impl Wilkinson {
    /// Makie's `WilkinsonTicks(k_ideal)` with its defaults (`k_min = 2`, `k_max = 10`).
    pub fn new(k_ideal: usize) -> Wilkinson {
        Wilkinson {
            k_ideal,
            k_min: 2,
            k_max: 10,
            q: vec![(1.0, 1.0), (5.0, 0.9), (2.0, 0.7), (2.5, 0.5), (3.0, 0.2)],
            granularity_weight: 1.0 / 4.0,
            simplicity_weight: 1.0 / 6.0,
            coverage_weight: 1.0 / 3.0,
            niceness_weight: 1.0 / 4.0,
        }
    }

    /// Sets the minimum number of ticks.
    pub fn k_min(mut self, k: usize) -> Wilkinson {
        self.k_min = k;
        self
    }

    /// Sets the maximum number of ticks.
    pub fn k_max(mut self, k: usize) -> Wilkinson {
        self.k_max = k;
        self
    }

    /// Makie's `get_tickvalues(::WilkinsonTicks, vmin, vmax)`.
    pub fn tick_values(&self, vmin: f64, vmax: f64) -> Vec<f64> {
        optimize_ticks(vmin, vmax, self, false)
    }

    fn valid(&self) -> bool {
        0 < self.k_min && self.k_min <= self.k_ideal && self.k_ideal <= self.k_max && !self.q.is_empty()
    }
}

/// `bounding_order_of_magnitude(xspan, 10.0)`: the smallest `b` with `xspan <= 10^b`.
fn bounding_order_of_magnitude(xspan: f64) -> i64 {
    let mut a = 1i64;
    while xspan < julia::powi(10.0, a) {
        a -= 1;
        if a < -400 {
            break;
        }
    }
    let mut b = 1i64;
    while xspan > julia::powi(10.0, b) {
        b += 1;
        if b > 400 {
            break;
        }
    }
    while a + 1 < b {
        let c = (a + b) / 2; // `div` truncates toward zero, like Rust
        if xspan < julia::powi(10.0, c) {
            b = c;
        } else {
            a = c;
        }
    }
    b
}

/// `postdecimal_digits(q)`: the number of decimals needed to write `q` exactly.
fn postdecimal_digits(x: f64) -> i64 {
    // floor(Int, log10(floatmin(Float64))) : ceil(Int, log10(floatmax(Float64)))
    for i in -308..=309 {
        if x == julia::round_digits(x, Mode::Down, i) {
            return i;
        }
    }
    0
}

/// `fallback_ticks(x_min, x_max, k_min, k_max, strict_span)`.
fn fallback_ticks(mut x_min: f64, mut x_max: f64, k_min: usize, strict_span: bool) -> Vec<f64> {
    if !strict_span && julia::isapprox(x_min, x_max, f64::EPSILON.sqrt()) {
        x_min = x_min.next_down();
        x_max = x_max.next_up();
    }
    if k_min != 2
        && x_min.is_finite()
        && x_max.is_finite()
        && let Some(r) = julia::linspace(x_min, x_max, k_min as i64)
    {
        return r.collect();
    }
    vec![x_min, x_max]
}

/// PlotUtils' `optimize_ticks(x_min, x_max; ...)`, returning only the tick values.
///
/// `integer_steps` restricts candidate steps to whole numbers (used for log-axis exponents, not
/// by Makie). Returns an empty vector for non-finite or reversed input and invalid parameters,
/// where the Julia code would throw or hang.
pub(crate) fn optimize_ticks(x_min: f64, x_max: f64, p: &Wilkinson, integer_steps: bool) -> Vec<f64> {
    if !(x_min.is_finite() && x_max.is_finite()) || x_min > x_max || !p.valid() {
        return Vec::new();
    }
    let rtol = 1000.0 * f64::EPSILON;
    if julia::isapprox(x_min, x_max, rtol) {
        return fallback_ticks(x_min, x_max, p.k_min, true);
    }
    for strict in [true, false] {
        if let Some(best) = optimize_ticks_typed(x_min, x_max, p, strict, integer_steps) {
            return best;
        }
        // Julia warns "No strict ticks found" and retries without strict span.
    }
    fallback_ticks(x_min, x_max, p.k_min, true)
}

fn optimize_ticks_typed(
    x_min: f64,
    x_max: f64,
    p: &Wilkinson,
    strict_span: bool,
    integer_steps: bool,
) -> Option<Vec<f64>> {
    let xspan = x_max - x_min;
    let (k_min, k_max, k_ideal) = (p.k_min as i64, p.k_max as i64, p.k_ideal as i64);
    let (gw, sw, cw, nw) = (p.granularity_weight, p.simplicity_weight, p.coverage_weight, p.niceness_weight);

    let mut z = bounding_order_of_magnitude(xspan);
    let max_post = p.q.iter().map(|&(q, _)| postdecimal_digits(q)).max().unwrap_or(0);
    let num_digits = bounding_order_of_magnitude(x_min.abs().max(x_max.abs())) + max_post;

    let mut high_score = f64::NEG_INFINITY;
    let mut best: Vec<f64> = Vec::new();
    let mut s: Vec<f64> = vec![0.0; (2 * k_max) as usize];

    while (2 * k_max) as f64 * julia::powi(10.0, z + 1) > xspan {
        let sigdigits = (num_digits - z).max(1);
        for k in k_min..=2 * k_max {
            for &(q, qscore0) in &p.q {
                let tickspan = q * julia::powi(10.0, z);
                if tickspan < f64::EPSILON {
                    continue;
                }
                let span = (k - 1) as f64 * tickspan;
                if span < xspan {
                    continue;
                }
                let r_float = (x_max - span) / tickspan;
                if !r_float.is_finite() {
                    continue;
                }
                if integer_steps && tickspan != tickspan.trunc() {
                    continue;
                }
                let mut r = r_float.ceil() as i64;
                let qscore = qscore0;
                let nice_scale = true;

                while r as f64 * tickspan <= x_min {
                    for i in 0..k {
                        s[i as usize] = (r + i) as f64 * tickspan;
                    }
                    let imax = k as usize;
                    let mut viewmin = julia::round_sigdigits(s[0], sigdigits);
                    s[0] = viewmin;
                    let mut viewmax = julia::round_sigdigits(s[imax - 1], sigdigits);
                    s[imax - 1] = viewmax;

                    let len = if strict_span {
                        viewmin = viewmin.max(x_min);
                        viewmax = viewmax.min(x_max);
                        let buf = 0.0 * (viewmax - viewmin);
                        let mut counter = 0;
                        for i in 0..imax {
                            if viewmin - buf <= s[i] && s[i] <= viewmax + buf {
                                s[counter] = s[i];
                                counter += 1;
                            }
                        }
                        counter as i64
                    } else {
                        imax as i64
                    };

                    let has_zero = r <= 0 && r.abs() < k;
                    let simplicity = if has_zero && nice_scale { 1.0 } else { 0.0 };
                    let g = if 0 < len && len < 2 * k_ideal {
                        1.0 - (len - k_ideal).abs() as f64 / k_ideal as f64
                    } else {
                        0.0
                    };
                    let c = if len > 1 {
                        let effective_span = (len - 1) as f64 * tickspan;
                        1.5 * xspan / effective_span
                    } else {
                        0.0
                    };
                    let mut score = gw * g + sw * simplicity + cw * c + nw * qscore;
                    if strict_span && span > xspan {
                        score -= 10000.0;
                    }
                    if span >= 2.0 * xspan {
                        score -= 1000.0;
                    }
                    if score > high_score && k_min <= len && len <= k_max {
                        high_score = score;
                        best.clear();
                        best.extend_from_slice(&s[..len as usize]);
                    }
                    r += 1;
                }
            }
        }
        z -= 1;
    }
    (high_score != f64::NEG_INFINITY).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_vectors() {
        let w = Wilkinson::default();
        let t = |a, b| w.tick_values(a, b);
        assert_eq!(t(0.55, 10.45), vec![2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(t(-1.1, 1.1), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        assert_eq!(t(-0.5, 10.5), vec![0.0, 5.0, 10.0]);
        // Only the ends of a candidate run are rounded, and here they are filtered out (same in Julia).
        assert_eq!(t(0.95, 2.05), vec![1.2000000000000002, 1.5000000000000002, 1.8000000000000003]);
        assert_eq!(t(12.3, 98.7), vec![25.0, 50.0, 75.0]);
        let plotutils = Wilkinson::new(5);
        assert_eq!(plotutils.tick_values(-1.0, 2.0), vec![-1.0, 0.0, 1.0, 2.0]);
        assert_eq!(plotutils.tick_values(1e11 - 1.0, 1e11 + 2.0), vec![1e11 - 1.0, 1e11, 1e11 + 1.0, 1e11 + 2.0]);
    }
}
