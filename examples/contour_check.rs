//! Visual check of `contour` / `contourf` against CairoMakie (`tools/contour_check.jl` renders the
//! same data to `out/contour_check_makie.png`):
//! (a) contour lines of a Gaussian mixture, 8 levels; (b) contourf of the same with a Colorbar;
//! (c) labelled contour lines, dashed; (d) FitzHugh–Nagumo nullclines (`levels = [0]`) with
//! labels and a trajectory. Also writes `out/contour_check.svg` and
//! `out/contour_check_extend.png` (explicit levels with `extendlow`/`extendhigh`).
use ezviz::prelude::*;

/// Three Gaussian bumps, shared with tools/contour_check.jl.
fn mixture(x: f64, y: f64) -> f64 {
    let g = |x0: f64, y0: f64, s: f64, a: f64| a * (-((x - x0).powi(2) + (y - y0).powi(2)) / (2.0 * s * s)).exp();
    g(-1.0, -0.5, 0.6, 1.0) + g(1.2, 0.8, 0.8, 0.8) + g(0.8, -1.2, 0.4, 0.6)
}

/// FitzHugh–Nagumo parameters (shared with the Julia script).
const I_EXT: f64 = 0.5;
const A: f64 = 0.7;
const B: f64 = 0.8;
const EPS: f64 = 0.08;

fn fhn(v: f64, w: f64) -> [f64; 2] {
    [v - v.powi(3) / 3.0 - w + I_EXT, EPS * (v + A - B * w)]
}

/// RK4 trajectory from `(v, w)`.
fn trajectory(mut v: f64, mut w: f64, dt: f64, n: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut vs, mut ws) = (vec![v], vec![w]);
    for _ in 0..n {
        let k1 = fhn(v, w);
        let k2 = fhn(v + 0.5 * dt * k1[0], w + 0.5 * dt * k1[1]);
        let k3 = fhn(v + 0.5 * dt * k2[0], w + 0.5 * dt * k2[1]);
        let k4 = fhn(v + dt * k3[0], w + dt * k3[1]);
        v += dt / 6.0 * (k1[0] + 2.0 * k2[0] + 2.0 * k3[0] + k4[0]);
        w += dt / 6.0 * (k1[1] + 2.0 * k2[1] + 2.0 * k3[1] + k4[1]);
        vs.push(v);
        ws.push(w);
    }
    (vs, ws)
}

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();

    let (nx, ny) = (120, 100);
    let xs = linspace(-3.0, 3.0, nx);
    let ys = linspace(-2.5, 2.5, ny);
    let z: Vec<f64> = (0..nx * ny).map(|k| mixture(xs[k % nx], ys[k / nx])).collect();
    let field = Field::new(&z, nx, ny);

    let fig = Figure!(size = (1000, 800));
    // (a) 8 automatic levels, colored by level.
    let a = Axis!(fig.at(1, 1); title = "contour, levels = 8");
    contour!(a, &xs, &ys, field; levels = 8);
    // (b) filled bands with a colorbar.
    let b = Axis!(fig.at(1, 2); title = "contourf, levels = 8");
    let cf = contourf!(b, &xs, &ys, field; levels = 8);
    Colorbar::new(fig.at(1, 3), &cf);
    // (c) labels on dashed lines.
    let c = Axis!(fig.at(2, 1); title = "labels = true, dashed");
    contour!(c, &xs, &ys, field; levels = 6, labels = true, linestyle = Linestyle::Dash, colormap = Colormap::MAGMA);
    // (d) nullclines of FitzHugh–Nagumo and a trajectory onto the limit cycle.
    let (n, m) = (200, 150);
    let vs = linspace(-2.5, 2.5, n);
    let ws = linspace(-1.0, 2.0, m);
    let f: Vec<f64> = (0..n * m).map(|k| fhn(vs[k % n], ws[k / n])[0]).collect();
    let g: Vec<f64> = (0..n * m).map(|k| fhn(vs[k % n], ws[k / n])[1]).collect();
    let d = Axis!(fig.at(2, 2..=3); title = "FitzHugh–Nagumo nullclines", xlabel = "v", ylabel = "w");
    let (tv, tw) = trajectory(-2.0, -0.5, 0.05, 2000);
    lines!(d, &tv, &tw; color = (GRAY, 0.8), linewidth = 1);
    contour!(d, &vs, &ws, Field::new(&f, n, m); levels = [0.0], color = RED, linewidth = 2, labels = true,
        labelformatter = |_: f64| "v' = 0", labelsize = 12);
    contour!(d, &vs, &ws, Field::new(&g, n, m); levels = [0.0], color = BLUE, linewidth = 2, labels = true,
        labelformatter = |_: f64| "w' = 0", labelsize = 12);
    fig.save("out/contour_check.png")?;
    fig.save("out/contour_check.svg")?;

    // Explicit levels with extensions, relative mode, and lines over bands.
    let fig2 = Figure!(size = (900, 400));
    let e = Axis!(fig2.at(1, 1); title = "levels = 0.1:0.1:0.6, extend auto");
    let cf2 = contourf!(e, &xs, &ys, field; levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6], extendlow = Extend::Auto,
        extendhigh = Extend::Auto, colormap = Colormap::PLASMA);
    contour!(e, &xs, &ys, field; levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6], color = BLACK, linewidth = 0.75);
    Colorbar::new(fig2.at(1, 2), &cf2);
    let h = Axis!(fig2.at(1, 3); title = "mode = relative, extendhigh = red");
    let cf3 = contourf!(h, &xs, &ys, field; levels = [0.1, 0.3, 0.5, 0.7], mode = ContourfMode::Relative,
        extendhigh = RED);
    Colorbar::new(fig2.at(1, 4), &cf3);
    fig2.save("out/contour_check_extend.png")?;

    println!("wrote out/contour_check.png, out/contour_check.svg and out/contour_check_extend.png");
    Ok(())
}
