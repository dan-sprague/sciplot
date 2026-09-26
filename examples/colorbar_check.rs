//! Visual check of the Colorbar block against CairoMakie (`tools/colorbar_check.jl` renders the
//! same figures to `out/colorbar_check_makie.png` and `out/colorbar_check_clips_makie.png`).
//!
//! S4: a magma heatmap with a labeled colorbar, a value-colored scatter with a colorbar, a
//! horizontal colorbar below an axis; then lowclip/highclip triangles, `flipaxis = false` and a
//! horizontal colorbar on top. Pass `--svg` to also write SVGs and `--cpu` to also render with the
//! CPU fallback.
use ezviz::prelude::*;

/// Two Gaussian bumps, shared with tools/colorbar_check.jl.
fn bumps(x: f64, y: f64) -> f64 {
    (-2.0 * ((x - 0.6).powi(2) + 4.0 * (y - 0.1).powi(2))).exp()
        + 0.6 * (-(3.0 * (x + 0.9).powi(2) + 10.0 * (y + 0.3).powi(2))).exp()
}

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();
    let svg = std::env::args().any(|a| a == "--svg");
    let cpu = std::env::args().any(|a| a == "--cpu");

    // 1. S4.
    let (nx, ny) = (200, 100);
    let xs = linspace(-2.0, 2.0, nx);
    let ys = linspace(-1.0, 1.0, ny);
    let z: Vec<f64> = (0..nx * ny).map(|k| bumps(xs[k % nx], ys[k / nx])).collect();
    let fig = Figure!(size = (900, 700));
    let ax1 = Axis!(fig.at(1, 1); title = "heatmap, magma");
    let hm = heatmap!(ax1, -2.0..=2.0, -1.0..=1.0, Field::new(&z, nx, ny); colormap = Colormap::MAGMA);
    Colorbar!(fig.at(1, 2), &hm; label = "amplitude");

    let n = 200;
    let t: Vec<f64> = (0..n).map(|i| 4.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).collect();
    let ax2 = Axis!(fig.at(1, 3); title = "scatter, color = values");
    let tc: Vec<f64> = t.iter().map(|t| t * 10.0).collect();
    let sc = scatter!(ax2, t.iter().map(|t| t * t.cos()), t.iter().map(|t| t * t.sin()); color = &tc, markersize = 8);
    Colorbar::new(fig.at(1, 4), &sc);

    let ax3 = Axis!(fig.at(2, 1..=3); title = "horizontal colorbar below");
    let x: Vec<f64> = (0..=100).map(|i| i as f64 * 0.1).collect();
    lines!(ax3, &x, x.iter().map(|x| x.sin()));
    Colorbar!(fig.at(3, 1..=3); colormap = Colormap::VIRIDIS, limits = (-1, 1), vertical = false, label = "horizontal");
    fig.save("out/colorbar_check.png")?;
    if svg {
        fig.save("out/colorbar_check.svg")?;
    }

    // 2. Clip triangles, flipaxis = false, horizontal on top (a new row above: row 0 in Makie).
    let fig2 = Figure!(size = (700, 400));
    let a = Axis!(fig2.at(1, 2); title = "clips");
    let (mx, my) = (40, 30);
    let w: Vec<f64> =
        (0..mx * my).map(|k| ((k % mx + 1) as f64 / 3.0).sin() * ((k / mx + 1) as f64 / 4.0).cos() * 1.4).collect();
    let hm2 = heatmap!(a, Field::new(&w, mx, my); colorrange = (-1, 1), lowclip = "cyan", highclip = "red", colormap = "RdBu");
    Colorbar!(fig2.at(1, 3), &hm2; label = "clipped");
    Colorbar!(fig2.at(1, 1), &hm2; flipaxis = false, label = "left side");
    Colorbar!(fig2.at(Prepend, 2); colormap = Colormap::PLASMA, limits = (0, 1000), vertical = false, highclip = "black", label = "top");
    fig2.save("out/colorbar_check_clips.png")?;
    if svg {
        fig2.save("out/colorbar_check_clips.svg")?;
    }
    if cpu {
        fig.save_with("out/colorbar_check_cpu.png", Save::new().cpu(true))?;
        fig2.save_with("out/colorbar_check_clips_cpu.png", Save::new().cpu(true))?;
    }

    println!("wrote out/colorbar_check.png and out/colorbar_check_clips.png");
    Ok(())
}
