//! Heatmap cell coordinates (Makie's `CellGrid` conversions, `conversions.jl:321-342, 419-438`).

use super::{Data1D, Scalar};
use std::sync::Arc;

/// Outer edges of a regular grid: `Edges(a, b)` spreads the cells evenly from edge `a` to edge `b`
/// (Makie's `EndPoints`), which is what finite-volume grids want.
///
/// Compare `a..=b`, which gives the centres of the first and last cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges<T: Scalar = f64>(pub T, pub T);

/// Heatmap cell coordinates along one axis, for `n` cells:
///
/// | input | meaning |
/// |---|---|
/// | `a..=b` (floats) | centres of the first and last cell (Makie's `a..b` interval): edges `a − Δ/2 ..= b + Δ/2` |
/// | [`Edges(a, b)`](Edges) | the outer edges (Makie's `EndPoints`) |
/// | a vector, slice, array, integer range or iterator of `n` values | cell centres (edges at midpoints, mirrored at the ends) |
/// | the same with `n + 1` values | cell edges |
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be used as heatmap cell coordinates",
    label = "expected a..=b (first/last centre), Edges(a, b) (outer edges), or n centres / n + 1 edges"
)]
pub trait CellCoords {
    #[doc(hidden)]
    fn cell_spec(self) -> CellSpec;
}

/// Opaque coordinate specification (see [`CellCoords`]).
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct CellSpec(pub(crate) Spec);

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Spec {
    /// Heatmap without coordinates: centres `1..=n`.
    Index,
    /// Centres of the first and last cell.
    Centres(f64, f64),
    /// Outer edges.
    Outer(f64, f64),
    /// `n` centres or `n + 1` edges.
    Values(Arc<Vec<f64>>),
}

/// Cell boundaries along one axis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CellEdges {
    /// `n` equal cells from edge `e0` to edge `e1` (possibly `e0 > e1`).
    Regular { e0: f64, e1: f64, n: usize },
    /// `n + 1` edges.
    Irregular(Arc<Vec<f64>>),
}

impl CellEdges {
    pub fn n(&self) -> usize {
        match self {
            CellEdges::Regular { n, .. } => *n,
            CellEdges::Irregular(e) => e.len().saturating_sub(1),
        }
    }

    pub fn first(&self) -> f64 {
        match self {
            CellEdges::Regular { e0, .. } => *e0,
            CellEdges::Irregular(e) => e.first().copied().unwrap_or(f64::NAN),
        }
    }

    pub fn last(&self) -> f64 {
        match self {
            CellEdges::Regular { e1, .. } => *e1,
            CellEdges::Irregular(e) => e.last().copied().unwrap_or(f64::NAN),
        }
    }

    /// Edge `i` in `0..=n`.
    pub fn edge(&self, i: usize) -> f64 {
        match self {
            CellEdges::Regular { e0, e1, n } => {
                if i == *n {
                    *e1
                } else {
                    e0 + (e1 - e0) * (i as f64 / *n as f64)
                }
            }
            CellEdges::Irregular(e) => e[i],
        }
    }
}

impl Spec {
    /// Edges for `n` cells, or a message explaining the mismatch.
    pub(crate) fn edges(&self, n: usize) -> Result<CellEdges, String> {
        let half_step = |a: f64, b: f64| {
            if n <= 1 { if a == b { 0.5 } else { 0.5 * (b - a) } } else { 0.5 * (b - a) / (n - 1) as f64 }
        };
        Ok(match self {
            Spec::Index => CellEdges::Regular { e0: 0.5, e1: n as f64 + 0.5, n },
            Spec::Centres(a, b) => {
                let h = half_step(*a, *b);
                CellEdges::Regular { e0: a - h, e1: b + h, n }
            }
            Spec::Outer(a, b) => CellEdges::Regular { e0: *a, e1: *b, n },
            Spec::Values(v) => {
                let e = if v.len() == n {
                    centres_to_edges(v)
                } else if v.len() == n + 1 {
                    v.as_ref().clone()
                } else {
                    return Err(format!(
                        "{} coordinates for {n} cells; expected {n} centres or {} edges",
                        v.len(),
                        n + 1
                    ));
                };
                regular(&e).unwrap_or(CellEdges::Irregular(Arc::new(e)))
            }
        })
    }
}

/// Makie's `edges(v)`: midpoints between centres, mirrored at both ends; one centre `v` gives
/// `[v - 0.5, v + 0.5]`.
pub(crate) fn centres_to_edges(v: &[f64]) -> Vec<f64> {
    match v.len() {
        0 => vec![],
        1 => vec![v[0] - 0.5, v[0] + 0.5],
        l => {
            let mut e: Vec<f64> = (0..=l).map(|i| 0.5 * (v[i.max(1) - 1] + v[i.min(l - 1)])).collect();
            e[0] = 2.0 * e[0] - e[1];
            e[l] = 2.0 * e[l] - e[l - 1];
            e
        }
    }
}

/// Detects evenly spaced edges (to within float rounding) so the GPU can use an affine lookup.
fn regular(e: &[f64]) -> Option<CellEdges> {
    let n = e.len().checked_sub(1).filter(|n| *n > 0)?;
    let (e0, e1) = (e[0], e[n]);
    let d = (e1 - e0) / n as f64;
    let tol = 1e-9 * d.abs().max(e0.abs().max(e1.abs()) * 1e-6);
    (d != 0.0 && d.is_finite() && e.iter().enumerate().all(|(i, x)| (x - (e0 + d * i as f64)).abs() <= tol))
        .then_some(CellEdges::Regular { e0, e1, n })
}

impl CellCoords for std::ops::RangeInclusive<f64> {
    fn cell_spec(self) -> CellSpec {
        CellSpec(Spec::Centres(*self.start(), *self.end()))
    }
}
impl CellCoords for std::ops::RangeInclusive<f32> {
    fn cell_spec(self) -> CellSpec {
        CellSpec(Spec::Centres(*self.start() as f64, *self.end() as f64))
    }
}
impl<T: Scalar> CellCoords for Edges<T> {
    fn cell_spec(self) -> CellSpec {
        CellSpec(Spec::Outer(self.0.to_f64(), self.1.to_f64()))
    }
}
/// Any 1D data: `n` centres or `n + 1` edges.
impl<D: Data1D> CellCoords for D {
    fn cell_spec(self) -> CellSpec {
        CellSpec(Spec::Values(Arc::new(self.to_vec_f64())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(c: impl CellCoords, n: usize) -> Vec<f64> {
        let e = c.cell_spec().0.edges(n).unwrap();
        (0..=e.n()).map(|i| e.edge(i)).collect()
    }

    #[test]
    fn makie_semantics() {
        // heatmap(0..1, 0..1, 4x3 matrix): x edges -1/6..7/6, y edges -0.25..1.25.
        let x = edges(0.0..=1.0, 4);
        assert!((x[0] + 1.0 / 6.0).abs() < 1e-12 && (x[4] - 7.0 / 6.0).abs() < 1e-12);
        assert_eq!(edges(0.0..=1.0, 3), [-0.25, 0.25, 0.75, 1.25]);
        assert_eq!(edges(Edges(0, 4), 4), [0.0, 1.0, 2.0, 3.0, 4.0]);
        assert_eq!(edges(1..=3, 3), [0.5, 1.5, 2.5, 3.5]);
        assert_eq!(edges(1..=4, 3), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(edges(vec![1.0, 2.0, 4.0], 3), [0.5, 1.5, 3.0, 5.0]);
        assert_eq!(edges(&[5.0][..], 1), [4.5, 5.5]);
        assert_eq!(edges(2.0..=2.0, 1), [1.5, 2.5]);
        assert_eq!(Spec::Index.edges(2).unwrap(), CellEdges::Regular { e0: 0.5, e1: 2.5, n: 2 });
        assert!(matches!(Spec::Values(Arc::new(vec![1.0, 2.0, 4.0])).edges(3), Ok(CellEdges::Irregular(_))));
        assert!(matches!(Spec::Values(Arc::new(vec![0.0, 0.1, 0.2, 0.3])).edges(3), Ok(CellEdges::Regular { .. })));
        assert!(Spec::Values(Arc::new(vec![1.0, 2.0])).edges(5).is_err());
    }
}
