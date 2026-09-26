//! Makie-style keyword sugar. Every macro expands to the builder chain, so a misspelled keyword is
//! a normal "no method named ..." error pointing at it.
//!
//! ```no_run
//! use ezviz::prelude::*;
//! let fig = Figure!(size = (800, 400));
//! let ax = Axis!(fig.at(1, 1); title = "demo", xlabel = "x");
//! scatter!(ax, [1.0, 2.0], [3.0, 4.0]; color = RED, markersize = 12);
//! ```

/// Applies `key = value` pairs as chained setters: `kw!(scatter(&x, &y); markersize = 4)`.
#[macro_export]
macro_rules! kw {
    ($base:expr; $($k:ident = $v:expr),* $(,)?) => { $base $(.$k($v))* };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kw {
    ($base:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $base $($(.$k($v))*)? };
}

/// `Figure!(size = (800, 600), fontsize = 12)` = `Figure::new().size((800, 600)).fontsize(12)`.
#[macro_export]
macro_rules! Figure {
    ($($k:ident = $v:expr),* $(,)?) => { $crate::Figure::new() $(.$k($v))* };
}

/// `Axis!(fig.at(1, 1); title = "t", xlabel = "x")` = `Axis::new(fig.at(1, 1)).title("t").xlabel("x")`.
#[macro_export]
macro_rules! Axis {
    ($pos:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Axis::new($pos) $(; $($k = $v),*)?)
    };
    ($pos:expr, $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: Axis!(fig.at(1, 1); title = \"...\")")
    };
}

/// Makie's `scatter!(ax, x, y; kw...)`: draws into `ax` (an `Axis` or `&Axis`).
#[macro_export]
macro_rules! scatter {
    ($ax:expr, $x:expr, $y:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).scatter($x, $y) $(; $($k = $v),*)?)
    };
    ($ax:expr, $x:expr, $y:expr, $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: scatter!(ax, x, y; color = RED)")
    };
}

/// Makie's `lines!(ax, x, y; kw...)` or `lines!(ax, points_or_y; kw...)`: draws into `ax` (an
/// `Axis` or `&Axis`).
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// lines!(ax, [0.0, 1.0, 2.0], [1.0, 0.0, 1.0]; color = RED, linewidth = 3);
/// lines!(ax, &[[0.0, 0.5], [2.0, 0.5]]; linestyle = Linestyle::Dash);
/// ```
#[macro_export]
macro_rules! lines {
    ($ax:expr, $x:expr, $k:ident = $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: lines!(ax, x, y; color = RED)")
    };
    ($ax:expr, $x:expr, $y:expr, $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: lines!(ax, x, y; color = RED)")
    };
    ($ax:expr, $x:expr, $y:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).lines($x, $y) $(; $($k = $v),*)?)
    };
    ($ax:expr, $p:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).lines_points($p) $(; $($k = $v),*)?)
    };
}

/// Makie's `scatterlines!(ax, x, y; kw...)` or `scatterlines!(ax, points_or_y; kw...)`.
#[macro_export]
macro_rules! scatterlines {
    ($ax:expr, $x:expr, $k:ident = $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: scatterlines!(ax, x, y; color = RED)")
    };
    ($ax:expr, $x:expr, $y:expr, $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: scatterlines!(ax, x, y; color = RED)")
    };
    ($ax:expr, $x:expr, $y:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).scatterlines($x, $y) $(; $($k = $v),*)?)
    };
    ($ax:expr, $p:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).scatterlines_points($p) $(; $($k = $v),*)?)
    };
}

/// Accepts `Axis` or `&Axis` in the plotting macros.
#[doc(hidden)]
pub trait AsAxis {
    fn __axis(&self) -> &crate::Axis;
}
impl AsAxis for crate::Axis {
    fn __axis(&self) -> &crate::Axis {
        self
    }
}
impl AsAxis for &crate::Axis {
    fn __axis(&self) -> &crate::Axis {
        self
    }
}
impl AsAxis for &&crate::Axis {
    fn __axis(&self) -> &crate::Axis {
        self
    }
}

#[doc(hidden)]
pub fn __as_axis<T: AsAxis + ?Sized>(t: &T) -> &crate::Axis {
    t.__axis()
}
