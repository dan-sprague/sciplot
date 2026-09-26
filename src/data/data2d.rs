//! 2D data for heatmaps. The first index is x (Makie's `z[i, j]` convention).

use super::Scalar;

/// 2D numeric data with `dims() = (nx, ny)`; element `(i, j)` belongs to cell `(x_i, y_j)`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not plottable 2D data",
    label = "expected Field::new(&v, nx, ny), (&v, nx, ny), &Vec<Vec<T>> (v[ix][iy]) or an ndarray Array2",
    note = "flat data is x-fastest: v[j * nx + i] is the value at (x_i, y_j)"
)]
pub trait Data2D {
    /// `(nx, ny)`.
    fn dims(&self) -> (usize, usize);
    /// Writes values x-fastest (`out[j * nx + i]`) as f32 after the transform `(v - off) * k`.
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64);
    /// Finite extrema in f64.
    fn extrema(&self) -> Option<(f64, f64)>;
}

/// A borrowed flat field with explicit dimensions.
///
/// `Field::new(&v, nx, ny)` reads `v[j * nx + i]` as the value at `(x_i, y_j)` (x fastest);
/// `Field::y_fastest(&v, nx, ny)` reads `v[i * ny + j]`.
#[derive(Clone, Copy)]
pub struct Field<'a, T: Scalar> {
    data: &'a [T],
    nx: usize,
    ny: usize,
    x_fastest: bool,
}

impl<'a, T: Scalar> Field<'a, T> {
    #[track_caller]
    pub fn new(data: &'a [T], nx: usize, ny: usize) -> Self {
        assert!(
            data.len() == nx * ny,
            "Field::new: data has {} values but nx * ny = {nx} * {ny} = {}",
            data.len(),
            nx * ny
        );
        Field { data, nx, ny, x_fastest: true }
    }

    #[track_caller]
    pub fn y_fastest(data: &'a [T], nx: usize, ny: usize) -> Self {
        Field { x_fastest: false, ..Field::new(data, nx, ny) }
    }

    #[inline]
    fn at(&self, i: usize, j: usize) -> f64 {
        let idx = if self.x_fastest { j * self.nx + i } else { i * self.ny + j };
        self.data[idx].to_f64()
    }
}

fn conv(v: f64, off: f64, k: f64) -> f32 {
    let r = (v - off) * k;
    if r.is_nan() { f32::NAN } else { r.clamp(f32::MIN as f64, f32::MAX as f64) as f32 }
}

impl<T: Scalar> Data2D for Field<'_, T> {
    fn dims(&self) -> (usize, usize) {
        (self.nx, self.ny)
    }
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64) {
        out.clear();
        out.reserve(self.nx * self.ny);
        if self.x_fastest {
            out.extend(self.data.iter().map(|v| conv(v.to_f64(), off, k)));
        } else {
            for j in 0..self.ny {
                for i in 0..self.nx {
                    out.push(conv(self.at(i, j), off, k));
                }
            }
        }
    }
    fn extrema(&self) -> Option<(f64, f64)> {
        super::finite_extrema(self.data.iter().map(|v| v.to_f64()))
    }
}

impl<T: Scalar> Data2D for (&[T], usize, usize) {
    fn dims(&self) -> (usize, usize) {
        (self.1, self.2)
    }
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64) {
        Field::new(self.0, self.1, self.2).write_f32(out, off, k)
    }
    fn extrema(&self) -> Option<(f64, f64)> {
        super::finite_extrema(self.0.iter().map(|v| v.to_f64()))
    }
}

impl<T: Scalar> Data2D for (&Vec<T>, usize, usize) {
    fn dims(&self) -> (usize, usize) {
        (self.1, self.2)
    }
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64) {
        Field::new(self.0, self.1, self.2).write_f32(out, off, k)
    }
    fn extrema(&self) -> Option<(f64, f64)> {
        super::finite_extrema(self.0.iter().map(|v| v.to_f64()))
    }
}

/// `v[ix][iy]`, like a Julia matrix `z[i, j]`.
impl<T: Scalar> Data2D for &Vec<Vec<T>> {
    fn dims(&self) -> (usize, usize) {
        (self.len(), self.first().map_or(0, |c| c.len()))
    }
    #[track_caller]
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64) {
        let (nx, ny) = self.dims();
        if let Some(r) = self.iter().position(|c| c.len() != ny) {
            panic!("ragged 2D data: v[{r}] has {} values, v[0] has {ny}", self[r].len());
        }
        out.clear();
        out.reserve(nx * ny);
        for j in 0..ny {
            for col in self.iter() {
                out.push(conv(col[j].to_f64(), off, k));
            }
        }
    }
    fn extrema(&self) -> Option<(f64, f64)> {
        super::finite_extrema(self.iter().flat_map(|c| c.iter().map(|v| v.to_f64())))
    }
}

#[cfg(feature = "ndarray")]
impl<S, T> Data2D for &ndarray::ArrayBase<S, ndarray::Ix2>
where
    S: ndarray::Data<Elem = T>,
    T: Scalar,
{
    fn dims(&self) -> (usize, usize) {
        let s = self.shape();
        (s[0], s[1])
    }
    fn write_f32(&self, out: &mut Vec<f32>, off: f64, k: f64) {
        let (nx, ny) = self.dims();
        out.clear();
        out.reserve(nx * ny);
        for j in 0..ny {
            for i in 0..nx {
                out.push(conv(self[[i, j]].to_f64(), off, k));
            }
        }
    }
    fn extrema(&self) -> Option<(f64, f64)> {
        super::finite_extrema(self.iter().map(|v| v.to_f64()))
    }
}
