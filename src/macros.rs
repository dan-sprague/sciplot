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
