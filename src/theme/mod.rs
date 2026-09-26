//! Themes: figure-wide defaults (Makie's `Theme`), with process-global `set_theme` and
//! thread-scoped `with_theme`.

use crate::blocks::axis::{AxisAttrs, AxisTheme};
use crate::color::{Color, WONG};
use crate::plots::heatmap::{HeatmapAttrs, HeatmapTheme};
use crate::plots::scatter::{ScatterAttrs, ScatterTheme};
use parking_lot::RwLock;
use std::cell::RefCell;

/// Global values that attribute defaults are derived from (Makie's `@inherit`).
#[derive(Clone, Debug)]
pub struct Globals {
    pub fontsize: f64,
    pub textcolor: Color,
    pub backgroundcolor: Color,
    pub figure_padding: [f64; 4],
    pub rowgap: f64,
    pub colgap: f64,
    pub size: [f64; 2],
    pub linewidth: f64,
    pub markersize: f64,
    /// Categorical colors cycled by lines and scatter.
    pub palette: Vec<Color>,
    /// Categorical fill colors cycled by bars, hists and bands.
    pub patchpalette: Vec<Color>,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            fontsize: 14.0,
            textcolor: Color::rgb(0.0, 0.0, 0.0),
            backgroundcolor: Color::rgb(1.0, 1.0, 1.0),
            figure_padding: [16.0; 4],
            rowgap: 18.0,
            colgap: 18.0,
            size: [600.0, 450.0],
            linewidth: 1.5,
            markersize: 9.0,
            palette: WONG.to_vec(),
            // Makie's patchcolor palette: 0.2 * background + 0.8 * color.
            patchpalette: WONG.iter().map(|c| Color::rgb(1.0, 1.0, 1.0).lerp(*c, 0.8)).collect(),
        }
    }
}

/// A set of overrides. Unset values fall back to Makie's defaults.
#[derive(Clone, Default, Debug)]
pub struct Theme {
    pub(crate) fontsize: Option<f64>,
    pub(crate) textcolor: Option<Color>,
    pub(crate) backgroundcolor: Option<Color>,
    pub(crate) figure_padding: Option<[f64; 4]>,
    pub(crate) rowgap: Option<f64>,
    pub(crate) colgap: Option<f64>,
    pub(crate) size: Option<[f64; 2]>,
    pub(crate) linewidth: Option<f64>,
    pub(crate) markersize: Option<f64>,
    pub(crate) palette: Option<Vec<Color>>,
    pub(crate) patchpalette: Option<Vec<Color>>,
    pub(crate) axis: AxisAttrs,
    pub(crate) scatter: ScatterAttrs,
    pub(crate) heatmap: HeatmapAttrs,
}

macro_rules! theme_setters {
    ($($(#[$m:meta])* $f:ident : $ty:ty),* $(,)?) => {$(
        $(#[$m])*
        #[track_caller]
        pub fn $f(mut self, v: impl crate::attrs::Conv<$ty>) -> Self { self.$f = Some(v.conv()); self }
    )*};
}

impl Theme {
    /// An empty theme (everything inherits Makie's defaults).
    pub fn new() -> Theme {
        Theme::default()
    }

    theme_setters! {
        /// Base font size in units (1 unit = 1 CSS px = 0.75 pt).
        fontsize: f64,
        textcolor: Color,
        backgroundcolor: Color,
        /// One number or `(left, right, bottom, top)`.
        figure_padding: [f64; 4],
        rowgap: f64,
        colgap: f64,
        size: [f64; 2],
        linewidth: f64,
        markersize: f64,
    }

    /// Categorical palette for lines and scatter.
    pub fn palette(mut self, colors: &[Color]) -> Self {
        self.palette = Some(colors.to_vec());
        self
    }

    /// Axis defaults: `Theme::new().axis(|a| a.xgridvisible(false))`.
    pub fn axis(mut self, f: impl FnOnce(AxisTheme) -> AxisTheme) -> Self {
        self.axis = f(AxisTheme(self.axis)).0;
        self
    }

    /// Scatter defaults.
    pub fn scatter(mut self, f: impl FnOnce(ScatterTheme) -> ScatterTheme) -> Self {
        self.scatter = f(ScatterTheme(self.scatter)).0;
        self
    }

    /// Heatmap defaults: `Theme::new().heatmap(|h| h.colormap(Colormap::MAGMA))`.
    pub fn heatmap(mut self, f: impl FnOnce(HeatmapTheme) -> HeatmapTheme) -> Self {
        self.heatmap = f(HeatmapTheme(self.heatmap)).0;
        self
    }

    /// Makie's `merge(a, b)`: values set in `self` win over `other`.
    pub fn merge(self, other: Theme) -> Theme {
        let mut out = other;
        macro_rules! take {
            ($($f:ident),*) => {$( if self.$f.is_some() { out.$f = self.$f; } )*};
        }
        take!(
            fontsize,
            textcolor,
            backgroundcolor,
            figure_padding,
            rowgap,
            colgap,
            size,
            linewidth,
            markersize,
            palette,
            patchpalette
        );
        out.axis.merge_from(&self.axis);
        out.scatter.merge_from(&self.scatter);
        out.heatmap.merge_from(&self.heatmap);
        out
    }

    /// Globals with this theme's overrides applied.
    pub(crate) fn globals(&self) -> Globals {
        let mut g = Globals::default();
        macro_rules! set {
            ($($f:ident),*) => {$( if let Some(v) = &self.$f { g.$f = v.clone(); } )*};
        }
        set!(
            fontsize,
            textcolor,
            backgroundcolor,
            figure_padding,
            rowgap,
            colgap,
            size,
            linewidth,
            markersize,
            palette
        );
        match &self.patchpalette {
            Some(p) => g.patchpalette = p.clone(),
            None => {
                if self.palette.is_some() || self.backgroundcolor.is_some() {
                    g.patchpalette = g.palette.iter().map(|c| g.backgroundcolor.lerp(*c, 0.8)).collect();
                }
            }
        }
        g
    }
}

static GLOBAL_THEME: RwLock<Option<Theme>> = RwLock::new(None);

thread_local! {
    static SCOPED: RefCell<Vec<Theme>> = const { RefCell::new(Vec::new()) };
}

/// Sets the process-wide default theme (Makie's `set_theme!`).
pub fn set_theme(t: Theme) {
    *GLOBAL_THEME.write() = Some(t);
}

/// Resets the process-wide theme to Makie's defaults.
pub fn reset_theme() {
    *GLOBAL_THEME.write() = None;
}

/// Runs `f` with `t` layered over the current theme, on this thread only. Restored on exit or panic.
pub fn with_theme<R>(t: Theme, f: impl FnOnce() -> R) -> R {
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            SCOPED.with_borrow_mut(|s| {
                s.pop();
            });
        }
    }
    SCOPED.with_borrow_mut(|s| s.push(t));
    let _pop = Pop;
    f()
}

/// The theme a new `Figure` would capture right now (scoped over global).
pub fn current_theme() -> Theme {
    let base = GLOBAL_THEME.read().clone().unwrap_or_default();
    SCOPED.with_borrow(|s| s.iter().fold(base, |acc, t| t.clone().merge(acc)))
}

/// Makie's `theme_minimal()`: no grid, only left and bottom spines.
pub fn theme_minimal() -> Theme {
    Theme::new().axis(|a| a.xgridvisible(false).ygridvisible(false).topspinevisible(false).rightspinevisible(false))
}
