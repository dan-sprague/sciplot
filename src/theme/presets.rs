//! Makie's built-in themes (`Makie/src/themes/theme_*.jl`), restricted to the blocks ezviz has.

use super::Theme;
use crate::color::Color;

/// X11 grays as Colors.jl defines them (`gray50` = #7F7F7F).
const GRAY10: Color = Color::hex(0x1A1A1A);
const GRAY45: Color = Color::hex(0x737373);
const GRAY50: Color = Color::hex(0x7F7F7F);

/// Makie's `theme_minimal()`: transparent axis background, no grids, only the left and bottom
/// spines, no ticks, label padding 3.
///
/// ```
/// use ezviz::prelude::*;
/// with_theme(theme_minimal(), || Figure::new().at(1, 1).lines([1.0, 2.0], [3.0, 1.0]));
/// ```
pub fn theme_minimal() -> Theme {
    Theme::new().axis(|a| {
        a.backgroundcolor(Color::TRANSPARENT)
            .xgridvisible(false)
            .ygridvisible(false)
            .xminorgridvisible(false)
            .yminorgridvisible(false)
            .leftspinevisible(true)
            .rightspinevisible(false)
            .bottomspinevisible(true)
            .topspinevisible(false)
            .xminorticksvisible(false)
            .yminorticksvisible(false)
            .xticksvisible(false)
            .yticksvisible(false)
            .xlabelpadding(3)
            .ylabelpadding(3)
    })
    .legend(|l| l.framevisible(false).padding(0))
}

/// Makie's `theme_light()`: gray text, faint grid, no spines, no ticks, label padding 3.
pub fn theme_light() -> Theme {
    Theme::new().textcolor(GRAY50).axis(|a| {
        a.backgroundcolor(Color::TRANSPARENT)
            .xgridcolor((Color::rgb(0.0, 0.0, 0.0), 0.07))
            .ygridcolor((Color::rgb(0.0, 0.0, 0.0), 0.07))
            .leftspinevisible(false)
            .rightspinevisible(false)
            .bottomspinevisible(false)
            .topspinevisible(false)
            .xminorticksvisible(false)
            .yminorticksvisible(false)
            .xticksvisible(false)
            .yticksvisible(false)
            .xlabelpadding(3)
            .ylabelpadding(3)
    })
    .legend(|l| l.framevisible(false).padding(0))
}

/// Makie's `theme_dark()`: `gray10` background, `gray45` text, faint white grid, no spines, no
/// ticks, label padding 3. The fill palette is regenerated against the dark background.
pub fn theme_dark() -> Theme {
    Theme::new().backgroundcolor(GRAY10).textcolor(GRAY45).axis(|a| {
        a.backgroundcolor(Color::TRANSPARENT)
            .xgridcolor((Color::rgb(1.0, 1.0, 1.0), 0.09))
            .ygridcolor((Color::rgb(1.0, 1.0, 1.0), 0.09))
            .leftspinevisible(false)
            .rightspinevisible(false)
            .bottomspinevisible(false)
            .topspinevisible(false)
            .xminorticksvisible(false)
            .yminorticksvisible(false)
            .xticksvisible(false)
            .yticksvisible(false)
            .xlabelpadding(3)
            .ylabelpadding(3)
    })
    .legend(|l| l.framevisible(false).padding(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makie_theme_values() {
        let t = theme_dark();
        let g = t.globals();
        assert_eq!(g.backgroundcolor.to_rgba8(), [26, 26, 26, 255]);
        assert_eq!(g.textcolor.to_rgba8(), [115, 115, 115, 255]);
        // patchcolor = 0.2 · gray10 + 0.8 · wong
        let p = g.patchpalette[0];
        assert!((p.r - (0.2 * GRAY10.r + 0.8 * crate::WONG[0].r)).abs() < 1e-6);
        let a = t.axis.resolve(&Default::default(), &g);
        assert!(!a.leftspinevisible && !a.xticksvisible && a.xgridvisible);
        assert_eq!(a.xgridcolor, Color::rgba(1.0, 1.0, 1.0, 0.09));
        assert_eq!(a.ylabelpadding, 3.0);

        let m = theme_minimal().axis.resolve(&Default::default(), &g);
        assert!(m.leftspinevisible && m.bottomspinevisible && !m.topspinevisible && !m.rightspinevisible);
        assert!(!m.xgridvisible && !m.yticksvisible);
        assert_eq!(m.backgroundcolor, Color::TRANSPARENT);

        let l = theme_light();
        assert_eq!(l.globals().textcolor.to_rgba8(), [127, 127, 127, 255]);
        assert_eq!(l.axis.resolve(&Default::default(), &g).ygridcolor, Color::rgba(0.0, 0.0, 0.0, 0.07));
    }
}
