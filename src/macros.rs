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

/// `Label!(fig.at(Prepend, ..), "Title"; fontsize = 20)` = `Label::new(pos, text).fontsize(20)`.
#[macro_export]
macro_rules! Label {
    ($pos:expr, $text:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Label::new($pos, $text) $(; $($k = $v),*)?)
    };
}

/// Makie's `Legend(fig[r, c], axes; kw...)`: `Legend!(fig.at(1, 2), &ax; title = "T")` or
/// `Legend!(fig.at(1..=2, 3), &[&a, &b]; unique = true)` = `Legend::new(pos, src).unique(true)`.
#[macro_export]
macro_rules! Legend {
    ($pos:expr, $src:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Legend::new($pos, $src) $(; $($k = $v),*)?)
    };
}

/// Makie's `axislegend(ax; position = :rt, kw...)`: `axislegend!(ax; position = Pos::LT)` =
/// `axislegend(&ax).position(Pos::LT)`.
#[macro_export]
macro_rules! axislegend {
    ($ax:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::axislegend($crate::__as_axis(&$ax)) $(; $($k = $v),*)?)
    };
}

/// Makie's `Colorbar(fig[r, c], plot; kw...)`: `Colorbar!(fig.at(1, 2), &hm; label = "z")`, or
/// without a plot `Colorbar!(fig.at(1, 2); colormap = Colormap::MAGMA, limits = (0, 10))`.
#[macro_export]
macro_rules! Colorbar {
    ($pos:expr, $plot:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Colorbar::new($pos, $plot) $(; $($k = $v),*)?)
    };
    ($pos:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Colorbar::standalone($pos) $(; $($k = $v),*)?)
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

/// Makie's `barplot!(ax, x, heights; kw...)` or `barplot!(ax, heights; kw...)`.
#[macro_export]
macro_rules! barplot {
    ($ax:expr, $x:expr, $h:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).barplot($x, $h) $(; $($k = $v),*)?)
    };
    ($ax:expr, $h:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).barplot_heights($h) $(; $($k = $v),*)?)
    };
}

/// Makie's `hist!(ax, values; kw...)`.
#[macro_export]
macro_rules! hist {
    ($ax:expr, $v:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).hist($v) $(; $($k = $val),*)?)
    };
}

/// Makie's `band!(ax, x, lower, upper; kw...)`.
#[macro_export]
macro_rules! band {
    ($ax:expr, $x:expr, $lo:expr, $hi:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).band($x, $lo, $hi) $(; $($k = $v),*)?)
    };
}

/// Makie's `text!(ax, x, y, texts; kw...)`.
#[macro_export]
macro_rules! text {
    ($ax:expr, $x:expr, $y:expr, $t:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).text($x, $y, $t) $(; $($k = $v),*)?)
    };
}

/// Makie's `heatmap!(ax, z)` / `heatmap!(ax, x, y, z)` with keywords after `;`:
/// `heatmap!(ax, Edges(0.0, 1.0), Edges(0.0, 1.0), Field::new(&v, nx, ny); colormap = Colormap::MAGMA)`.
#[macro_export]
macro_rules! heatmap {
    ($ax:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).heatmap($z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).heatmap_xy($x, $y, $z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $($rest:tt)*) => {
        compile_error!("heatmap! takes (ax, z) or (ax, x, y, z), then `;` before keyword arguments: heatmap!(ax, z; colormap = Colormap::MAGMA)")
    };
}

/// Makie's `contour!(ax, z)` / `contour!(ax, x, y, z)` with keywords after `;`:
/// `contour!(ax, &xs, &ys, Field::new(&v, nx, ny); levels = [0.0], color = RED)`.
#[macro_export]
macro_rules! contour {
    ($ax:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).contour($z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).contour_xy($x, $y, $z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $($rest:tt)*) => {
        compile_error!("contour! takes (ax, z) or (ax, x, y, z), then `;` before keyword arguments: contour!(ax, z; levels = 8)")
    };
}

/// Makie's `contourf!(ax, z)` / `contourf!(ax, x, y, z)` with keywords after `;`:
/// `contourf!(ax, &xs, &ys, Field::new(&v, nx, ny); levels = 8, extendhigh = Extend::Auto)`.
#[macro_export]
macro_rules! contourf {
    ($ax:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).contourf($z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).contourf_xy($x, $y, $z) $(; $($k = $v),*)?)
    };
    ($ax:expr, $($rest:tt)*) => {
        compile_error!("contourf! takes (ax, z) or (ax, x, y, z), then `;` before keyword arguments: contourf!(ax, z; levels = 8)")
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

/// Makie's `hlines!(ax, ys; kw...)`: horizontal lines spanning `xmin..xmax` of the axis width.
#[macro_export]
macro_rules! hlines {
    ($ax:expr, $v:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).hlines($v) $(; $($k = $val),*)?)
    };
}

/// Makie's `vlines!(ax, xs; kw...)`: vertical lines spanning `ymin..ymax` of the axis height.
#[macro_export]
macro_rules! vlines {
    ($ax:expr, $v:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).vlines($v) $(; $($k = $val),*)?)
    };
}

/// Makie's `ablines!(ax, intercepts, slopes; kw...)`: lines `y = a + b·x` across the axis.
#[macro_export]
macro_rules! ablines {
    ($ax:expr, $a:expr, $b:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).ablines($a, $b) $(; $($k = $val),*)?)
    };
}

/// Makie's `arrows2d!(ax, x, y, u, v; kw...)` (quiver), `arrows2d!(ax, xs, ys, f; kw...)` on the
/// grid `xs × ys`, or `arrows2d!(ax, points, directions; kw...)`.
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// let g = linspace(-2.0, 2.0, 15);
/// arrows!(ax, &g, &g, |x, y| (-y, x); color = Magnitude, normalize = true, lengthscale = 0.2);
/// ```
#[macro_export]
macro_rules! arrows {
    ($ax:expr, $x:expr, $y:expr, $u:expr, $v:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).arrows($x, $y, $u, $v) $(; $($k = $val),*)?)
    };
    ($ax:expr, $x:expr, $y:expr, $f:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).arrows_fn($x, $y, $f) $(; $($k = $val),*)?)
    };
    ($ax:expr, $p:expr, $d:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).arrows_points($p, $d) $(; $($k = $val),*)?)
    };
}

/// Makie's `streamplot!(ax, f, xrange, yrange; kw...)`.
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// streamplot!(ax, |x, y| (y, -x.sin()), -3.0..=3.0, -2.0..=2.0; colormap = Colormap::MAGMA);
/// ```
#[macro_export]
macro_rules! streamplot {
    ($ax:expr, $f:expr, $x:expr, $y:expr $(; $($k:ident = $val:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__as_axis(&$ax).streamplot($f, $x, $y) $(; $($k = $val),*)?)
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

/// Makie's `rich(...)`: builds a [`RichText`](crate::RichText) from strings, spans
/// ([`superscript`](crate::superscript), [`subscript`](crate::subscript), `.color(..)`,
/// [`colored`](crate::text::colored)) and other rich texts (e.g. [`tex`](crate::text::tex)).
///
/// ```
/// use ezviz::prelude::*;
/// let label = rich!("k", superscript("\u{2212}5/3"), colored(" (fit)", RED));
/// assert_eq!(label.plain_text(), "k\u{2212}5/3 (fit)");
/// ```
#[macro_export]
macro_rules! rich {
    ($($s:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut spans = ::std::vec::Vec::new();
        $($crate::text::IntoSpans::push_into($s, &mut spans);)*
        $crate::RichText::from_spans(spans)
    }};
}

/// `Axis3!(fig.at(1, 1); azimuth = 0.3, title = "t")` = `Axis3::new(fig.at(1, 1)).azimuth(0.3).title("t")`.
#[macro_export]
macro_rules! Axis3 {
    ($pos:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::Axis3::new($pos) $(; $($k = $v),*)?)
    };
    ($pos:expr, $($rest:tt)*) => {
        compile_error!("use `;` before keyword arguments: Axis3!(fig.at(1, 1); title = \"...\")")
    };
}

/// Makie's `lines!(ax3, x, y, z; kw...)` into an [`Axis3`](crate::Axis3).
#[macro_export]
macro_rules! lines3d {
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!(($ax).lines($x, $y, $z) $(; $($k = $v),*)?)
    };
}

/// Makie's `scatter!(ax3, x, y, z; kw...)` into an [`Axis3`](crate::Axis3).
#[macro_export]
macro_rules! scatter3d {
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!(($ax).scatter($x, $y, $z) $(; $($k = $v),*)?)
    };
}

/// Makie's `surface!(ax3, x, y, z; kw...)` into an [`Axis3`](crate::Axis3).
#[macro_export]
macro_rules! surface {
    ($ax:expr, $x:expr, $y:expr, $z:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!(($ax).surface($x, $y, $z) $(; $($k = $v),*)?)
    };
}
