//! What counts as plottable data.
//!
//! 1D data ([`Data1D`]) is accepted from slices, `Vec`s, arrays, integer ranges, common iterator
//! adapters (so `t.iter().map(|t| t.sin())` needs no `.collect()`), and anything wrapped in
//! [`iter`]. Every numeric type converts through [`Scalar`].

mod data2d;
pub(crate) mod points;

pub use data2d::{Data2D, Field};
pub use points::PointData;

mod sealed {
    pub trait Sealed {}
}

/// A number that can be plotted: every built-in integer and float type (and references to them).
pub trait Scalar: Copy + sealed::Sealed {
    fn to_f64(self) -> f64;
}

macro_rules! scalar {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl Scalar for $t { #[inline] fn to_f64(self) -> f64 { self as f64 } }
        impl sealed::Sealed for &$t {}
        impl Scalar for &$t { #[inline] fn to_f64(self) -> f64 { *self as f64 } }
        impl sealed::Sealed for &&$t {}
        impl Scalar for &&$t { #[inline] fn to_f64(self) -> f64 { **self as f64 } }
    )*};
}
scalar!(f64, f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// A number usable as a numeric attribute value (`linewidth = 2`, `markersize = 7.5`).
pub trait Num: Scalar {}
impl Num for f64 {}
impl Num for f32 {}
impl Num for i32 {}
impl Num for i64 {}
impl Num for u32 {}
impl Num for u64 {}
impl Num for usize {}
impl Num for isize {}

/// 1D numeric data.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not plottable 1D data",
    label = "expected a slice, Vec, array, integer range or iterator of numbers",
    note = "wrap any other iterator of numbers in `ezviz::iter(..)`"
)]
pub trait Data1D {
    /// Appends the values (as f64) to `out`.
    fn write_f64(self, out: &mut Vec<f64>);

    /// Collects into a new vector.
    fn to_vec_f64(self) -> Vec<f64>
    where
        Self: Sized,
    {
        let mut v = Vec::new();
        self.write_f64(&mut v);
        v
    }
}

impl<T: Scalar> Data1D for &[T] {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.iter().map(|v| v.to_f64()));
    }
}
impl<T: Scalar> Data1D for &mut [T] {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.iter().map(|v| v.to_f64()));
    }
}
impl<T: Scalar> Data1D for &Vec<T> {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.iter().map(|v| v.to_f64()));
    }
}
impl<T: Scalar> Data1D for Vec<T> {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.into_iter().map(|v| v.to_f64()));
    }
}
impl<T: Scalar, const N: usize> Data1D for [T; N] {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.into_iter().map(|v| v.to_f64()));
    }
}
impl<T: Scalar, const N: usize> Data1D for &[T; N] {
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.iter().map(|v| v.to_f64()));
    }
}

macro_rules! int_ranges {
    ($($t:ty),*) => {$(
        impl Data1D for std::ops::Range<$t> {
            fn write_f64(self, out: &mut Vec<f64>) { out.extend(self.map(|v| v as f64)); }
        }
        impl Data1D for std::ops::RangeInclusive<$t> {
            fn write_f64(self, out: &mut Vec<f64>) { out.extend(self.map(|v| v as f64)); }
        }
    )*};
}
int_ranges!(i32, i64, u32, u64, usize);

macro_rules! iter_adapters {
    ($([$($g:tt)*] $t:ty),* $(,)?) => {$(
        impl<$($g)*> Data1D for $t
        where
            Self: Iterator,
            <Self as Iterator>::Item: Scalar,
        {
            fn write_f64(self, out: &mut Vec<f64>) { out.extend(self.map(|v| v.to_f64())); }
        }
    )*};
}
iter_adapters!(
    [I, F] std::iter::Map<I, F>,
    [I] std::iter::Copied<I>,
    [I] std::iter::Cloned<I>,
    [I] std::iter::StepBy<I>,
    [I] std::iter::Take<I>,
    [I] std::iter::Skip<I>,
    [I] std::iter::Rev<I>,
    [A, B] std::iter::Chain<A, B>,
    [I, P] std::iter::Filter<I, P>,
    ['a, T] std::slice::Iter<'a, T>,
    [T] std::vec::IntoIter<T>,
);

/// Wraps any iterator of numbers as plottable data: `ezviz::iter(my_iter)`.
pub struct Iter<I>(pub I);

/// Wraps any iterator of numbers as plottable data.
pub fn iter<I>(it: I) -> Iter<I::IntoIter>
where
    I: IntoIterator,
    I::Item: Scalar,
{
    Iter(it.into_iter())
}

impl<I> Data1D for Iter<I>
where
    I: Iterator,
    I::Item: Scalar,
{
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.0.map(|v| v.to_f64()));
    }
}

#[cfg(feature = "ndarray")]
impl<S, T> Data1D for &ndarray::ArrayBase<S, ndarray::Ix1>
where
    S: ndarray::Data<Elem = T>,
    T: Scalar,
{
    fn write_f64(self, out: &mut Vec<f64>) {
        out.extend(self.iter().map(|v| v.to_f64()));
    }
}

/// `n` evenly spaced values from `a` to `b` inclusive (Julia's `range(a, b, length = n)`).
pub fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    match n {
        0 => vec![],
        1 => vec![a],
        _ => {
            let d = (b - a) / (n - 1) as f64;
            (0..n).map(|i| if i == n - 1 { b } else { a + d * i as f64 }).collect()
        }
    }
}

/// `n` log-spaced values from `10^a` to `10^b`.
pub fn logspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    linspace(a, b, n).into_iter().map(|e| 10f64.powf(e)).collect()
}

/// Minimum and maximum of the finite values, or `None` if there are none.
pub(crate) fn finite_extrema(v: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in v {
        if x.is_finite() {
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(d: impl Data1D) -> Vec<f64> {
        d.to_vec_f64()
    }

    #[test]
    fn inputs() {
        let v = vec![1.0f64, 2.0];
        assert_eq!(take(&v), [1.0, 2.0]);
        assert_eq!(take(&v[..]), [1.0, 2.0]);
        assert_eq!(take([1, 2, 3]), [1.0, 2.0, 3.0]);
        assert_eq!(take(0..3), [0.0, 1.0, 2.0]);
        assert_eq!(take(v.iter().map(|x| x * 2.0)), [2.0, 4.0]);
        assert_eq!(take(v.iter()), [1.0, 2.0]);
        assert_eq!(take(v.iter().rev()), [2.0, 1.0]);
        assert_eq!(take(vec![1f32, 2.0]), [1.0, 2.0]);
        assert_eq!(take(iter(std::iter::repeat(1u8).take(2))), [1.0, 1.0]);
        assert_eq!(linspace(0.0, 1.0, 3), [0.0, 0.5, 1.0]);
    }
}
