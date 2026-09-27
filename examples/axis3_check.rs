//! Visual check of `Axis3` against CairoMakie (`tools/axis3_check.jl` renders the same figures to
//! `out/axis3_<name>_makie.png`):
//! (a) the Lorenz attractor (`lines`, colored by time), (b) a tilted double-well potential
//! (`surface` with a Colorbar) with a trajectory on it, (c) a point cloud (`scatter`, colored by
//! z), each at Makie's default view and at a second azimuth / elevation. Writes
//! `out/axis3_<name>.png`, and for the first view of each `.svg` and `_cpu.png` (the SVG backend's
//! painter's-order 3D, rasterized on the CPU).
//!
//! Run: `cargo run --example axis3_check`
use sciplot::prelude::*;
use std::f64::consts::PI;

/// Lorenz system (σ = 10, ρ = 28, β = 8/3), RK4 with a fixed step (shared with the Julia script).
fn lorenz(n: usize, dt: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let f = |p: [f64; 3]| [10.0 * (p[1] - p[0]), p[0] * (28.0 - p[2]) - p[1], p[0] * p[1] - 8.0 / 3.0 * p[2]];
    let mut p = [1.0, 1.0, 1.0];
    let (mut xs, mut ys, mut zs) = (vec![p[0]], vec![p[1]], vec![p[2]]);
    for _ in 0..n {
        let k1 = f(p);
        let k2 = f(std::array::from_fn(|i| p[i] + 0.5 * dt * k1[i]));
        let k3 = f(std::array::from_fn(|i| p[i] + 0.5 * dt * k2[i]));
        let k4 = f(std::array::from_fn(|i| p[i] + dt * k3[i]));
        p = std::array::from_fn(|i| p[i] + dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]));
        xs.push(p[0]);
        ys.push(p[1]);
        zs.push(p[2]);
    }
    (xs, ys, zs)
}

/// A tilted double well.
fn potential(x: f64, y: f64) -> f64 {
    (x * x - 1.0).powi(2) + 0.8 * y * y - 0.3 * x
}

/// A wobbly sphere of points (Fibonacci lattice), shared with the Julia script.
fn cloud(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let (mut xs, mut ys, mut zs) = (vec![], vec![], vec![]);
    for k in 0..n {
        let z = 1.0 - 2.0 * (k as f64 + 0.5) / n as f64;
        let th = 2.399963229728653 * k as f64;
        let r = (1.0 - z * z).sqrt() * (1.0 + 0.25 * (5.0 * th).sin());
        xs.push(r * th.cos());
        ys.push(r * th.sin());
        zs.push(z);
    }
    (xs, ys, zs)
}

/// The two views: `(suffix, azimuth, elevation, perspectiveness)`.
const VIEWS: [(&str, f64, f64, f64); 2] = [("a", 1.275 * PI, PI / 8.0, 0.0), ("b", 0.3 * PI, 0.45, 0.5)];

fn main() -> sciplot::Result<()> {
    std::fs::create_dir_all("out").ok();
    let (lx, ly, lz) = lorenz(4000, 0.01);
    let t: Vec<f64> = (0..lx.len()).map(|i| i as f64 * 0.01).collect();
    let (nx, ny) = (60, 50);
    let gx = linspace(-1.8, 1.8, nx);
    let gy = linspace(-1.5, 1.5, ny);
    let v: Vec<f64> = (0..nx * ny).map(|k| potential(gx[k % nx], gy[k / nx])).collect();
    // A path along the valley floor, slightly above the surface.
    let px = linspace(-1.6, 1.6, 200);
    let py: Vec<f64> = px.iter().map(|x| 0.4 * (2.0 * x).sin()).collect();
    let pz: Vec<f64> = px.iter().zip(&py).map(|(x, y)| potential(*x, *y) + 0.05).collect();
    let (cx, cy, cz) = cloud(600);

    for (i, (suffix, az, el, persp)) in VIEWS.into_iter().enumerate() {
        let fig = Figure::new();
        let ax =
            Axis3!(fig.at(1, 1); title = "Lorenz attractor", azimuth = az, elevation = el, perspectiveness = persp);
        ax.lines(&lx, &ly, &lz).color(&t).linewidth(1.0);
        fig.save(format!("out/axis3_lorenz_{suffix}.png"))?;
        if i == 0 {
            fig.save("out/axis3_lorenz_a.svg")?;
            fig.save_with("out/axis3_lorenz_a_cpu.png", Save::new().cpu(true))?;
        }

        let fig = Figure::new();
        let ax = Axis3!(fig.at(1, 1); title = "double-well potential", xlabel = "x", ylabel = "y", zlabel = "V",
            azimuth = az, elevation = el, perspectiveness = persp);
        let s = ax.surface(&gx, &gy, Field::new(&v, nx, ny));
        ax.lines(&px, &py, &pz).color(RED).linewidth(2.0);
        Colorbar::new(fig.at(1, 2), &s).label("V");
        fig.save(format!("out/axis3_surface_{suffix}.png"))?;
        if i == 0 {
            fig.save("out/axis3_surface_a.svg")?;
            fig.save_with("out/axis3_surface_a_cpu.png", Save::new().cpu(true))?;
        }

        let fig = Figure::new();
        let ax = Axis3!(fig.at(1, 1); title = "point cloud", aspect = Aspect3::Data,
            azimuth = az, elevation = el, perspectiveness = persp);
        ax.scatter(&cx, &cy, &cz).color(&cz).markersize(8);
        fig.save(format!("out/axis3_cloud_{suffix}.png"))?;
        if i == 0 {
            fig.save("out/axis3_cloud_a.svg")?;
            fig.save_with("out/axis3_cloud_a_cpu.png", Save::new().cpu(true))?;
        }
    }
    println!("wrote out/axis3_{{lorenz,surface,cloud}}_{{a,b}}.png");
    Ok(())
}
