//! S5: a live simulation in a window.
//!
//! A 2D heat equation (explicit finite differences on a 64 × 64 grid, zero boundary temperature)
//! driven by a hot spot circling the domain. The left axis shows the field as isotherm bands
//! (cells above 0.05, 0.2 and 0.5), the right axis the peak temperature over time. The
//! simulation runs on a worker thread; the window stays interactive (scroll to zoom, drag to
//! pan or zoom into a rectangle, hover a cell for its coordinates).
//!
//! `EZVIZ_AUTOCLOSE=3 cargo run --example s5_live` closes the window after 3 s.
use ezviz::prelude::*;

const N: usize = 64;

struct Heat {
    u: Vec<f64>,
    next: Vec<f64>,
    t: f64,
}

impl Heat {
    const DX: f64 = 1.0 / N as f64;
    /// Stable explicit step: dt <= dx² / 4.
    const DT: f64 = 0.2 * Self::DX * Self::DX;

    fn new() -> Heat {
        Heat { u: vec![0.0; N * N], next: vec![0.0; N * N], t: 0.0 }
    }

    fn center(i: usize) -> f64 {
        (i as f64 + 0.5) * Self::DX
    }

    fn step(&mut self) {
        let (sx, sy) = (0.5 + 0.3 * (30.0 * self.t).cos(), 0.5 + 0.3 * (30.0 * self.t).sin());
        let r = Self::DT / (Self::DX * Self::DX);
        for j in 0..N {
            for i in 0..N {
                let at = |i: isize, j: isize| {
                    if i < 0 || j < 0 || i >= N as isize || j >= N as isize {
                        0.0
                    } else {
                        self.u[j as usize * N + i as usize]
                    }
                };
                let (ii, jj) = (i as isize, j as isize);
                let lap = at(ii - 1, jj) + at(ii + 1, jj) + at(ii, jj - 1) + at(ii, jj + 1) - 4.0 * at(ii, jj);
                let (x, y) = (Self::center(i), Self::center(j));
                let source = 150.0 * (-((x - sx).powi(2) + (y - sy).powi(2)) / 0.004).exp();
                self.next[j * N + i] = at(ii, jj) + r * lap + Self::DT * source;
            }
        }
        std::mem::swap(&mut self.u, &mut self.next);
        self.t += Self::DT;
    }

    /// Cell centers where the temperature exceeds `level`.
    fn above(&self, level: f64) -> (Vec<f64>, Vec<f64>) {
        let mut x = Vec::new();
        let mut y = Vec::new();
        for (k, &v) in self.u.iter().enumerate() {
            if v > level {
                x.push(Self::center(k % N));
                y.push(Self::center(k / N));
            }
        }
        (x, y)
    }

    fn peak(&self) -> f64 {
        self.u.iter().copied().fold(0.0, f64::max)
    }
}

fn main() -> ezviz::Result<()> {
    let fig = Figure::new().size((900, 450)).window_title("ezviz: live 2D heat equation");
    let field = Axis::new(fig.at(1, 1)).title("step 0").xlabel("x").ylabel("y").limits(0.0, 1.0, 0.0, 1.0);
    let levels = [(0.05, Color::hex(0x56B4E9)), (0.2, Color::hex(0xE69F00)), (0.5, Color::hex(0xD55E00))];
    let bands: Vec<Scatter> = levels
        .iter()
        .map(|(_, c)| {
            field.scatter(Vec::<f64>::new(), Vec::<f64>::new()).marker(Marker::FullRect).markersize(6).color(*c)
        })
        .collect();
    let diag = Axis::new(fig.at(1, 2)).title("peak temperature").xlabel("t").ylabel("max u");
    let peak = diag.scatter(Vec::<f64>::new(), Vec::<f64>::new()).markersize(4);

    let mut sim = Heat::new();
    let (mut ts, mut peaks) = (Vec::new(), Vec::new());
    let steps = fig.show_live(|live| {
        let mut step = 0u64;
        while live.is_open() {
            for _ in 0..20 {
                sim.step();
                step += 1;
            }
            ts.push(sim.t);
            peaks.push(sim.peak());
            live.batch(|| {
                for ((level, _), band) in levels.iter().zip(&bands) {
                    let (x, y) = sim.above(*level);
                    band.set_data(&x, &y);
                }
                peak.set_data(&ts, &peaks);
                field.title(format!("step {step}"));
            });
            // Publish at display rate; returns at once if the window is hidden or closed.
            live.wait_frame_timeout(std::time::Duration::from_millis(250));
        }
        step
    })?;
    println!("window closed; the simulation stopped after {steps} steps");
    Ok(())
}
