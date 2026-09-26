//! Visual check of the `field` pipeline against CairoMakie (`tools/heatmap_check.jl` renders the
//! same data to `out/heatmap_check_makie.png` and `out/heatmap_check_irregular_makie.png`).
use ezviz::prelude::*;

/// Two Gaussian bumps, shared with tools/heatmap_check.jl.
fn bumps(x: f64, y: f64) -> f64 {
    (-2.0 * ((x - 0.6).powi(2) + 4.0 * (y - 0.1).powi(2))).exp()
        + 0.6 * (-(3.0 * (x + 0.9).powi(2) + 10.0 * (y + 0.3).powi(2))).exp()
}

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();

    // 1. A 400x200 field (x fastest), magma with an explicit colorrange, next to a value-colored
    //    scatter.
    let (nx, ny) = (400, 200);
    let xs = linspace(-2.0, 2.0, nx);
    let ys = linspace(-1.0, 1.0, ny);
    let z: Vec<f64> = (0..nx * ny).map(|k| bumps(xs[k % nx], ys[k / nx])).collect();

    let fig = Figure!(size = (900, 400));
    let ax1 = Axis!(fig.at(1, 1); title = "heatmap, magma, colorrange (0, 0.8)");
    heatmap!(ax1, -2.0..=2.0, -1.0..=1.0, Field::new(&z, nx, ny); colormap = Colormap::MAGMA, colorrange = (0.0, 0.8));

    let n = 300;
    let t: Vec<f64> = (0..n).map(|i| 4.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).collect();
    let ax2 = Axis!(fig.at(1, 2); title = "scatter, color = values");
    scatter!(ax2, t.iter().map(|t| t * t.cos()), t.iter().map(|t| t * t.sin()); color = &t, markersize = 10);
    fig.save("out/heatmap_check.png")?;

    // 2. Irregular edges, NaN cells, interpolation, clip colors and a log-scaled y axis.
    let xe = [0.0, 1.0, 1.5, 3.0, 3.2, 5.0];
    let yc = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
    let (nx, ny) = (xe.len() - 1, yc.len());
    let mut w: Vec<f64> = (0..nx * ny).map(|k| (k % nx) as f64 + 0.5 * (k / nx) as f64).collect();
    w[2 * nx + 1] = f64::NAN;
    let fig2 = Figure!(size = (900, 400));
    let a = Axis!(fig2.at(1, 1); title = "irregular, NaN, clips", yscale = Scale::Log10);
    heatmap!(a, &xe, &yc, Field::new(&w, nx, ny); colorrange = (1.0, 6.0), lowclip = "cyan", highclip = "red");
    let b = Axis!(fig2.at(1, 2); title = "interpolate = true");
    let (mx, my) = (8, 6);
    let v: Vec<f64> = (0..mx * my).map(|k| ((k % mx) as f64 * 0.8).sin() * ((k / mx) as f64 * 0.9).cos()).collect();
    heatmap!(b, Field::new(&v, mx, my); interpolate = true, colormap = "RdBu");
    fig2.save("out/heatmap_check_irregular.png")?;

    println!("wrote out/heatmap_check.png and out/heatmap_check_irregular.png");
    Ok(())
}
