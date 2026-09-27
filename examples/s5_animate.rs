//! S5 with the portable animation API: dynamic systems advanced inside `fig.animate`.
//!
//! - Gray–Scott reaction–diffusion on a 256 × 256 periodic grid, shown as a heatmap (magma) of
//!   `v`; several simulation steps run per displayed frame and the title shows the step count.
//! - A growing diagnostic line: mean `v` against the step.
//! - The Lorenz attractor projected on (x, z), drawn as a growing line.
//!
//! Everything runs on the main thread in the per-frame callback (no worker thread), which is
//! the model that also works in the browser. The window stays interactive: scroll to zoom,
//! drag to pan or zoom into a rectangle, hover for values.
//!
//! `SCIPLOT_AUTOCLOSE=4 SCIPLOT_WINDOW_DUMP=out/s5_animate.png cargo run --release --example
//! s5_animate` closes after 4 s and writes the last frame.
use sciplot::prelude::*;

const N: usize = 256;
/// Simulation steps per displayed frame.
const STEPS_PER_FRAME: usize = 12;
/// Lorenz RK4 steps per displayed frame.
const LORENZ_PER_FRAME: usize = 20;

/// Gray–Scott model (Pearson 1993), unit grid spacing and time step, periodic boundaries.
struct GrayScott {
    u: Vec<f64>,
    v: Vec<f64>,
    u2: Vec<f64>,
    v2: Vec<f64>,
    step: u64,
}

impl GrayScott {
    const DU: f64 = 0.16;
    const DV: f64 = 0.08;
    const F: f64 = 0.035;
    const K: f64 = 0.065;

    fn new() -> GrayScott {
        let mut u = vec![1.0; N * N];
        let mut v = vec![0.0; N * N];
        // Deterministic xorshift seeding: a central square plus a few random patches.
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rnd = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut patch = |cx: usize, cy: usize, r: usize, rnd: &mut dyn FnMut() -> f64| {
            for j in cy - r..cy + r {
                for i in cx - r..cx + r {
                    let k = (j % N) * N + (i % N);
                    u[k] = 0.5 + 0.02 * rnd();
                    v[k] = 0.25 + 0.02 * rnd();
                }
            }
        };
        patch(N / 2, N / 2, 10, &mut rnd);
        for _ in 0..8 {
            let (cx, cy) = (16 + (rnd() * (N - 32) as f64) as usize, 16 + (rnd() * (N - 32) as f64) as usize);
            patch(cx, cy, 4, &mut rnd);
        }
        GrayScott { u2: u.clone(), v2: v.clone(), u, v, step: 0 }
    }

    fn step(&mut self) {
        let (u, v) = (&self.u, &self.v);
        for j in 0..N {
            let (jm, jp) = ((j + N - 1) % N * N, (j + 1) % N * N);
            let row = j * N;
            for i in 0..N {
                let (im, ip) = ((i + N - 1) % N, (i + 1) % N);
                let k = row + i;
                let lap_u = u[row + im] + u[row + ip] + u[jm + i] + u[jp + i] - 4.0 * u[k];
                let lap_v = v[row + im] + v[row + ip] + v[jm + i] + v[jp + i] - 4.0 * v[k];
                let uvv = u[k] * v[k] * v[k];
                self.u2[k] = u[k] + Self::DU * lap_u - uvv + Self::F * (1.0 - u[k]);
                self.v2[k] = v[k] + Self::DV * lap_v + uvv - (Self::F + Self::K) * v[k];
            }
        }
        std::mem::swap(&mut self.u, &mut self.u2);
        std::mem::swap(&mut self.v, &mut self.v2);
        self.step += 1;
    }

    fn mean_v(&self) -> f64 {
        self.v.iter().sum::<f64>() / self.v.len() as f64
    }
}

/// The Lorenz system (σ = 10, ρ = 28, β = 8/3), integrated with RK4.
struct Lorenz {
    p: [f64; 3],
}

impl Lorenz {
    const DT: f64 = 0.005;

    fn f(p: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = p;
        [10.0 * (y - x), x * (28.0 - z) - y, x * y - 8.0 / 3.0 * z]
    }

    fn step(&mut self) {
        let add = |a: [f64; 3], b: [f64; 3], h: f64| [a[0] + h * b[0], a[1] + h * b[1], a[2] + h * b[2]];
        let h = Self::DT;
        let k1 = Self::f(self.p);
        let k2 = Self::f(add(self.p, k1, h / 2.0));
        let k3 = Self::f(add(self.p, k2, h / 2.0));
        let k4 = Self::f(add(self.p, k3, h));
        for d in 0..3 {
            self.p[d] += h / 6.0 * (k1[d] + 2.0 * k2[d] + 2.0 * k3[d] + k4[d]);
        }
    }
}

fn main() -> sciplot::Result<()> {
    let fig = Figure::new().size((1300, 460)).window_title("sciplot: animate (Gray–Scott, Lorenz)");
    let ax_gs = Axis::new(fig.at(1, 1)).title("step 0").xlabel("x").ylabel("y");
    let mut gs = GrayScott::new();
    let hm = ax_gs.heatmap(Field::new(&gs.v, N, N)).colormap(Colormap::MAGMA).colorrange((0.0, 0.45));
    fig.colsize(1, GridSize::Aspect(1, 1.0));

    let ax_d = Axis::new(fig.at(1, 2)).title("diagnostic").xlabel("step").ylabel("mean v").yticklabelspace(50.0);
    let diag = ax_d.lines(Vec::<f64>::new(), Vec::<f64>::new());

    let ax_l = Axis::new(fig.at(1, 3)).title("Lorenz attractor").xlabel("x").ylabel("z");
    let mut lz = Lorenz { p: [1.0, 1.0, 1.0] };
    let traj = ax_l.lines([lz.p[0]], [lz.p[2]]).color(WONG[1]).linewidth(1.0);

    fig.animate(|frame| {
        for _ in 0..STEPS_PER_FRAME {
            gs.step();
        }
        for _ in 0..LORENZ_PER_FRAME {
            lz.step();
            traj.push(lz.p[0], lz.p[2]);
        }
        // One batch: the frame never shows half of the update.
        fig.batch(|| {
            hm.set_data(Field::new(&gs.v, N, N));
            diag.push(gs.step as f64, gs.mean_v());
            ax_gs.title(format!("step {}  (t = {:.1} s, frame {})", gs.step, frame.t, frame.count));
        });
    })?;
    println!("window closed after {} Gray–Scott steps", gs.step);
    Ok(())
}
