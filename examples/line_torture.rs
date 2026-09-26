//! Line pipeline stress page: widths, translucent joints, acute angles, degenerate input, caps,
//! joins, dash styles and color modes. Writes `out/line_torture.png`, plus `out/lines_ezviz.png`
//! (the data of `tools/lines_check.jl`, for comparison with CairoMakie's `out/lines_makie.png`).
use ezviz::prelude::*;

const NAN: f64 = f64::NAN;

/// A chain of V shapes with the given apex angles (degrees), starting at `(x, y)`, height `h`.
fn vees(x: f64, y: f64, h: f64, angles: &[f64]) -> Vec<[f64; 2]> {
    let mut pts = vec![[x, y]];
    let mut cx = x;
    for a in angles {
        let dx = h * (a.to_radians() / 2.0).tan();
        pts.push([cx + dx, y + h]);
        cx += 2.0 * dx;
        pts.push([cx, y]);
    }
    pts
}

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();
    let fig = Figure!(size = (1200, 1000));
    let half = (BLACK, 0.5);

    // Widths 0.5..20 units, translucent zigzags: joints must not be darker than the segments.
    let a = Axis!(fig.at(1, 1); title = "widths 0.5–20, alpha 0.5");
    for (k, w) in [0.5, 1.0, 2.0, 4.0, 8.0, 12.0, 20.0].into_iter().enumerate() {
        let y = 1.0 + 1.3 * k as f64;
        let x: Vec<f64> = (0..8).map(|i| 0.5 + 1.3 * i as f64).collect();
        let yy: Vec<f64> = (0..8).map(|i| y + if i % 2 == 0 { 0.0 } else { 0.7 }).collect();
        lines!(a, &x, &yy; linewidth = w, color = half);
    }
    a.limits(0.0, 10.0, 0.0, 10.0);

    // Every join style against increasingly acute angles (120° .. 5°).
    let b = Axis!(fig.at(1, 2); title = "miter / bevel / round joins");
    let angles = [120.0, 90.0, 60.0, 45.0, 30.0, 15.0, 5.0];
    for (k, join) in [JoinStyle::Miter, JoinStyle::Bevel, JoinStyle::Round].into_iter().enumerate() {
        let p = vees(0.5, 0.5 + 3.3 * k as f64, 2.0, &angles);
        lines!(b, &p; linewidth = 12, joinstyle = join, color = (WONG[k], 0.5));
    }
    b.limits(0.0, 18.0, 0.0, 10.0);

    // Caps on single segments and on open polylines, with a thin center line for reference.
    let c = Axis!(fig.at(2, 1); title = "butt / square / round caps");
    for (k, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round].into_iter().enumerate() {
        let y = 1.5 + 3.0 * k as f64;
        lines!(c, [1.0, 4.0], [y, y]; linewidth = 20, linecap = cap, color = (WONG[k], 0.5));
        lines!(c, [6.0, 7.5, 9.0], [y - 0.8, y + 0.8, y - 0.8]; linewidth = 20, linecap = cap, joinstyle = JoinStyle::Round, color = (WONG[k], 0.5));
        lines!(c, [1.0, 4.0], [y, y]; linewidth = 1, color = BLACK);
    }
    c.limits(0.0, 10.0, 0.0, 9.0);

    // Degenerate input: duplicate points, NaN gaps, 180° reversals, tiny segments.
    let d = Axis!(fig.at(2, 2); title = "zero-length, NaN gaps, reversals");
    let w = 10.0;
    lines!(d, [1.0, 2.5, 2.5, 2.5, 4.0], [1.0, 3.0, 3.0, 3.0, 1.0]; linewidth = w, color = half);
    lines!(d, [5.0, 6.0, NAN, 7.0, 8.0, NAN, 9.0, NAN, 9.5, 10.5], [1.0, 3.0, NAN, 1.0, 3.0, NAN, 2.0, NAN, 1.0, 3.0]; linewidth = w, color = (WONG[5], 0.5));
    lines!(d, [1.0, 4.0, 2.0], [5.0, 5.0, 5.0]; linewidth = w, color = (WONG[0], 0.5));
    lines!(d, [5.0, 9.0, 5.0], [5.0, 5.0, 5.4]; linewidth = w, color = (WONG[2], 0.5));
    let spiral: Vec<[f64; 2]> = (0..400)
        .map(|i| {
            let t = i as f64 * 0.05;
            [2.5 + 0.08 * t * t.cos(), 8.0 + 0.08 * t * t.sin()]
        })
        .collect();
    lines!(d, &spiral; linewidth = 6, color = (WONG[3], 0.5));
    lines!(d, [6.0, 6.0, 8.0, 8.0, 6.0], [7.0, 9.0, 9.0, 7.0, 7.0]; linewidth = w, color = (WONG[4], 0.5));
    d.limits(0.0, 11.0, 0.0, 10.0);

    // Dash styles at two widths, phase continuous around corners.
    let e = Axis!(fig.at(3, 1); title = "linestyles");
    let x = linspace(0.0, 10.0, 200);
    let styles = [
        Linestyle::Solid,
        Linestyle::Dash,
        Linestyle::Dot,
        Linestyle::DashDot,
        Linestyle::DashDotDot,
        Linestyle::Custom(vec![0.0, 4.0, 5.0, 6.0, 7.0]),
    ];
    for (k, s) in styles.into_iter().enumerate() {
        let y0 = 1.0 + 1.5 * k as f64;
        lines!(e, &x, x.iter().map(|x| y0 + 0.4 * (2.0 * x).sin()); linestyle = s.clone(), linewidth = 2, color = BLACK);
        lines!(e, [10.5, 12.0, 13.5, 12.0], [y0 - 0.4, y0 + 0.5, y0 - 0.4, y0 - 0.2]; linestyle = s, linewidth = 5, color = WONG[k % 7]);
    }
    e.limits(0.0, 14.0, 0.0, 10.0);

    // Color modes: per-point colors, values through the colormap, translucent gradients, scatterlines.
    let f = Axis!(fig.at(3, 2); title = "per-point colors / values / scatterlines");
    let n = 60;
    let xs = linspace(0.5, 9.5, n);
    let rainbow: Vec<Color> = (0..n).map(|i| WONG[i * 7 / n]).collect();
    lines!(f, &xs, xs.iter().map(|x| 8.0 + 0.5 * x.sin()); color = rainbow, linewidth = 8);
    let v: Vec<f64> = xs.iter().map(|x| x.cos()).collect();
    lines!(f, &xs, xs.iter().map(|x| 5.5 + 0.8 * x.cos()); color = &v, linewidth = 8);
    let fade: Vec<Color> = (0..n).map(|i| WONG[5].with_alpha(i as f32 / n as f32)).collect();
    lines!(f, &xs, xs.iter().map(|x| 3.5 + 0.3 * (3.0 * x).sin()); color = fade, linewidth = 12);
    scatterlines!(f, [1.0, 3.0, 5.0, 7.0, 9.0], [1.0, 2.0, 0.8, 1.8, 1.2]; markersize = 14, linewidth = 3);
    f.limits(0.0, 10.0, 0.0, 9.5);

    fig.save("out/line_torture.png")?;

    // Same data as tools/lines_check.jl.
    let x = linspace(0.0, 10.0, 100);
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    lines!(ax, &x, x.iter().map(|x| x.sin()));
    lines!(ax, &x, x.iter().map(|x| x.cos()); linewidth = 4);
    lines!(ax, &x, x.iter().map(|x| 0.5 * (2.0 * x).sin()); linestyle = Linestyle::Dash, linewidth = 2);
    scatterlines!(ax, [1.0, 3.0, 5.0, 7.0, 9.0], [-0.8, 0.6, -0.4, 0.9, -0.9]);
    fig.save("out/lines_ezviz.png")?;
    println!("wrote out/line_torture.png and out/lines_ezviz.png");
    Ok(())
}
