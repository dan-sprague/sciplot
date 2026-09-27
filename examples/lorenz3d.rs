//! A live 3D trajectory: the Lorenz attractor grows inside `fig.animate` (appended with
//! `Lines3d::push`, so only new points are converted and uploaded) while the camera turns
//! slowly around it (`Axis3::azimuth`).
//!
//! `SCIPLOT_AUTOCLOSE=4 SCIPLOT_WINDOW_DUMP=out/lorenz3d.png cargo run --example lorenz3d` closes
//! after 4 s and writes the first frame.
use sciplot::prelude::*;

/// Lorenz RK4 steps per displayed frame.
const STEPS_PER_FRAME: usize = 8;

fn lorenz(p: [f64; 3]) -> [f64; 3] {
    [10.0 * (p[1] - p[0]), p[0] * (28.0 - p[2]) - p[1], p[0] * p[1] - 8.0 / 3.0 * p[2]]
}

fn rk4(p: [f64; 3], dt: f64) -> [f64; 3] {
    let k1 = lorenz(p);
    let k2 = lorenz(std::array::from_fn(|i| p[i] + 0.5 * dt * k1[i]));
    let k3 = lorenz(std::array::from_fn(|i| p[i] + 0.5 * dt * k2[i]));
    let k4 = lorenz(std::array::from_fn(|i| p[i] + dt * k3[i]));
    std::array::from_fn(|i| p[i] + dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
}

fn main() -> sciplot::Result<()> {
    let fig = Figure::new().size((800, 700)).window_title("sciplot: Lorenz attractor (Axis3)");
    let ax = Axis3!(fig.at(1, 1); title = "Lorenz attractor", perspectiveness = 0.3);
    ax.limits(-25.0, 25.0, -30.0, 30.0, 0.0, 55.0);
    let mut p = [1.0, 1.0, 1.0];
    let traj = ax.lines([p[0]], [p[1]], [p[2]]).color(WONG[0]).linewidth(1.2);
    let head = ax.scatter([p[0]], [p[1]], [p[2]]).color(WONG[5]).markersize(10);
    let az0 = 1.275 * std::f64::consts::PI;
    fig.animate(|frame| {
        for _ in 0..STEPS_PER_FRAME {
            p = rk4(p, 0.005);
            traj.push(p[0], p[1], p[2]);
        }
        fig.batch(|| {
            head.set_data([p[0]], [p[1]], [p[2]]);
            ax.azimuth(az0 + 0.1 * frame.t);
        });
    })?;
    println!("window closed after {} points", traj.len());
    Ok(())
}
