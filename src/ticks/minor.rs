//! Minor ticks: Makie's `IntervalsBetween(n)` (exact port of `get_minor_tickvalues`,
//! `Makie/src/makielayout/lineaxis.jl:880-948`) and the ezviz log-axis default (plan D2).

use super::julia;
use super::log::LogBase;

/// Makie's `get_minor_tickvalues(IntervalsBetween(n, mirror), scale, tickvalues, vmin, vmax)`.
///
/// Unfiltered and in Makie's order (mirrored ticks below the first major come first, in
/// descending order). `n < 2` yields nothing (Makie rejects it).
pub(crate) fn intervals_between(
    n: usize,
    mirror: bool,
    base: Option<LogBase>,
    t: &[f64],
    vmin: f64,
    vmax: f64,
) -> Vec<f64> {
    let mut vals = Vec::new();
    if t.len() < 2 || n < 2 {
        return vals;
    }
    let nf = n as f64;
    let last = t.len() - 1;

    if mirror {
        let stepsize = match base {
            None => (t[1] - t[0]) / nf,
            Some(b) => {
                let first_scaled = b.forward(t[1]) - b.forward(t[0]);
                let prevtick = b.inverse(b.forward(t[0]) - first_scaled);
                (t[0] - prevtick) / nf
            }
        };
        let v = t[0] - stepsize;
        if let Some(r) = julia::colon(v, -stepsize, vmin) {
            vals.extend(r.collect());
        }
    }

    for w in t.windows(2) {
        let stepsize = (w[1] - w[0]) / nf;
        let mut v = w[0];
        for _ in 1..n {
            v += stepsize;
            vals.push(v);
        }
    }

    if mirror {
        let stepsize = match base {
            None => (t[last] - t[last - 1]) / nf,
            Some(b) => {
                let last_scaled = b.forward(t[last]) - b.forward(t[last - 1]);
                let nexttick = b.inverse(b.forward(t[last]) + last_scaled);
                (nexttick - t[last]) / nf
            }
        };
        let v = t[last] + stepsize;
        if let Some(r) = julia::colon(v, stepsize, vmax) {
            vals.extend(r.collect());
        }
    }
    vals
}

/// Log-axis minor ticks (plan D2), for majors on whole powers of `base`:
/// - consecutive powers: `k·10ⁿ` for `k = 2..9` in every visible decade,
/// - majors that skip powers: the skipped `10ⁿ`.
///
/// Returns `None` when the majors are not whole powers (or the base is not 10 and the majors are
/// consecutive powers); the caller then uses Makie's `IntervalsBetween`.
pub(crate) fn log_decade_minors(major: &[f64], vmin: f64, vmax: f64, base: LogBase) -> Option<Vec<f64>> {
    let exps = decade_exponents(major, base)?;
    let step = exps.windows(2).map(|w| w[1] - w[0]).min().unwrap_or(1);
    let (lo_e, hi_e) = (base.forward(vmin).floor(), base.forward(vmax).ceil());
    if !(lo_e.is_finite() && hi_e.is_finite()) || hi_e - lo_e > 10_000.0 {
        return None;
    }
    let (lo_e, hi_e) = (lo_e as i64, hi_e as i64);
    let mut out = Vec::new();
    if step <= 1 {
        if base != LogBase::Ten {
            return None;
        }
        for d in lo_e..=hi_e {
            for k in 2..=9 {
                out.push(format!("{k}e{d}").parse().unwrap_or(f64::NAN));
            }
        }
    } else {
        out.extend((lo_e..=hi_e).filter(|d| !exps.contains(d)).map(|d| base.int_pow(d)));
    }
    Some(out)
}

/// The integer exponents of `major` if every value is a whole power of `base` (at least two).
pub(crate) fn decade_exponents(major: &[f64], base: LogBase) -> Option<Vec<i64>> {
    if major.len() < 2 {
        return None;
    }
    major
        .iter()
        .map(|&v| {
            let e = base.forward(v);
            let r = e.round();
            (e.is_finite() && (e - r).abs() < 1e-9 && r.abs() < 1e6).then_some(r as i64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_mirror() {
        let v = intervals_between(2, true, None, &[2.0, 4.0, 6.0, 8.0, 10.0], 0.55, 10.45);
        assert_eq!(v, vec![1.0, 3.0, 5.0, 7.0, 9.0]);
    }

    #[test]
    fn decades() {
        let m = log_decade_minors(&[1.0, 10.0, 100.0], 0.9, 120.0, LogBase::Ten).unwrap();
        assert!(m.contains(&2.0) && m.contains(&0.3) && m.contains(&90.0) && m.contains(&200.0));
        let m = log_decade_minors(&[1.0, 100.0, 1e4], 1.0, 1e4, LogBase::Ten).unwrap();
        assert_eq!(m, vec![10.0, 1000.0]);
        assert!(log_decade_minors(&[2.0, 4.0], 1.0, 5.0, LogBase::Ten).is_none());
    }
}
