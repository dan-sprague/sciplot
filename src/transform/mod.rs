//! Axis scale transforms (applied on the CPU in f64, as in Makie).

/// An axis scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Scale {
    #[default]
    Identity,
    Log10,
    Log2,
    Ln,
    Sqrt,
}

crate::attrs::conv_identity!(Scale);

impl Scale {
    /// Data -> scaled space. Values outside the domain become NaN.
    #[inline]
    pub fn forward(self, v: f64) -> f64 {
        match self {
            Scale::Identity => v,
            Scale::Log10 => {
                if v > 0.0 {
                    v.log10()
                } else {
                    f64::NAN
                }
            }
            Scale::Log2 => {
                if v > 0.0 {
                    v.log2()
                } else {
                    f64::NAN
                }
            }
            Scale::Ln => {
                if v > 0.0 {
                    v.ln()
                } else {
                    f64::NAN
                }
            }
            Scale::Sqrt => {
                if v >= 0.0 {
                    v.sqrt()
                } else {
                    f64::NAN
                }
            }
        }
    }

    /// Scaled space -> data.
    #[inline]
    pub fn inverse(self, v: f64) -> f64 {
        match self {
            Scale::Identity => v,
            Scale::Log10 => 10f64.powf(v),
            Scale::Log2 => v.exp2(),
            Scale::Ln => v.exp(),
            Scale::Sqrt => v * v,
        }
    }

    pub fn is_log(self) -> bool {
        matches!(self, Scale::Log10 | Scale::Log2 | Scale::Ln)
    }

    /// Whether `v` is inside the scale's domain.
    pub fn valid(self, v: f64) -> bool {
        match self {
            Scale::Identity => v.is_finite(),
            Scale::Log10 | Scale::Log2 | Scale::Ln => v.is_finite() && v > 0.0,
            Scale::Sqrt => v.is_finite() && v >= 0.0,
        }
    }
}

/// Maps scaled coordinates to f32 "local" GPU coordinates: `local = (s - origin) * k`.
/// Keeping `origin` near the view and `k ~ 1/extent` preserves precision for data like
/// `1e9 ± 2` (Makie's Float32Convert).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rebase {
    pub origin: [f64; 2],
    pub k: [f64; 2],
    pub epoch: u64,
}

impl Rebase {
    /// A rebase centred on the view `[x0, x1, y0, y1]` (scaled space).
    pub fn for_view(view: [f64; 4], epoch: u64) -> Rebase {
        let k = |a: f64, b: f64| {
            let w = (b - a).abs();
            if w.is_finite() && w > 0.0 {
                2.0 / w
            } else {
                1.0
            }
        };
        Rebase {
            origin: [0.5 * (view[0] + view[1]), 0.5 * (view[2] + view[3])],
            k: [k(view[0], view[1]), k(view[2], view[3])],
            epoch,
        }
    }

    #[inline]
    pub fn to_local(&self, sx: f64, sy: f64) -> [f32; 2] {
        [
            ((sx - self.origin[0]) * self.k[0]) as f32,
            ((sy - self.origin[1]) * self.k[1]) as f32,
        ]
    }

    /// The view in local coordinates.
    pub fn local_view(&self, view: [f64; 4]) -> [f64; 4] {
        [
            (view[0] - self.origin[0]) * self.k[0],
            (view[1] - self.origin[0]) * self.k[0],
            (view[2] - self.origin[1]) * self.k[1],
            (view[3] - self.origin[1]) * self.k[1],
        ]
    }

    /// Whether f32 local coordinates can still resolve the view (Makie's criterion: at least
    /// 1e4 distinct f32 values across the visible range, and no overflow).
    pub fn adequate(&self, view: [f64; 4]) -> bool {
        let l = self.local_view(view);
        let ok = |a: f64, b: f64| {
            let w = (b - a).abs();
            let m = a.abs().max(b.abs());
            w.is_finite() && w > 1e4 * f32::EPSILON as f64 * m && w > 1e-30 && m < 1e30
        };
        ok(l[0], l[1]) && ok(l[2], l[3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_precision() {
        let view = [1e9 - 2.0, 1e9 + 2.0, 0.0, 1.0];
        let r = Rebase::for_view(view, 0);
        assert!(r.adequate(view));
        let a = r.to_local(1e9 + 1.0, 0.5);
        let b = r.to_local(1e9 + 1.001, 0.5);
        assert!(b[0] > a[0]);
        // A naive f32 conversion could not tell these apart.
        assert_eq!((1e9f64 + 1.0) as f32, (1e9f64 + 1.001) as f32);
        // Zooming far in on the old rebase triggers a rebase.
        assert!(r.adequate([1e9, 1e9 + 1e-6, 0.0, 1.0])); // near the origin f32 is dense
        assert!(!r.adequate([1e9 + 1.0, 1e9 + 1.0 + 1e-6, 0.0, 1.0]));
    }
}
