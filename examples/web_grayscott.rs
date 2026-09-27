//! Gray–Scott reaction–diffusion on a 256 × 256 periodic grid, simulated live inside
//! `fig.animate` — the same code natively and in the browser.
//!
//! Left: `v` as a heatmap (magma) with a colorbar; the title shows the step count. Right: the
//! mean of `v` against the step, growing as the pattern spreads. Several simulation steps run per
//! displayed frame. Scroll to zoom, drag to pan or zoom into a rectangle, hover for values.
//!
//! Native: `cargo run --release --example web_grayscott` (`SCIPLOT_AUTOCLOSE=4
//! SCIPLOT_WINDOW_DUMP=out/web_grayscott_native.png` closes after 4 s and writes the last frame).
//! Browser: `tools/web/build.sh web_grayscott`, then serve `examples/web/` and open
//! `web_grayscott.html` (add `?backend=gl` to force WebGL2).
use sciplot::prelude::*;

const N: usize = 256;
/// Simulation steps per displayed frame.
const STEPS_PER_FRAME: usize = 10;

/// A deterministic xorshift64 generator in [0, 1) (no `rand` dependency on the web).
fn xorshift(seed: u64) -> impl FnMut() -> f64 {
    let mut s = seed.max(1);
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

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

    /// A central square and a few random patches of `v` in a `u = 1` background.
    fn new(seed: u64) -> GrayScott {
        let mut u = vec![1.0; N * N];
        let mut v = vec![0.0; N * N];
        let mut rnd = xorshift(seed);
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

fn main() -> sciplot::Result<()> {
    let fig = Figure::new().size((980, 440)).window_title("sciplot: Gray–Scott");
    let ax = Axis::new(fig.at(1, 1)).title("step 0").xlabel("x").ylabel("y");
    let mut gs = GrayScott::new(0x9E37_79B9_7F4A_7C15);
    let hm = ax.heatmap(Field::new(&gs.v, N, N)).colormap(Colormap::MAGMA).colorrange((0.0, 0.45));
    Colorbar::new(fig.at(1, 2), &hm).label("v");
    fig.colsize(1, GridSize::Aspect(1, 1.0));
    let ax_m = Axis::new(fig.at(1, 3)).title("pattern growth").xlabel("step").ylabel("mean v").yticklabelspace(50.0);
    let mean = ax_m.lines([0.0], [gs.mean_v()]).color(WONG[0]);

    let fig2 = fig.clone();
    fig.animate(move |_frame| {
        for _ in 0..STEPS_PER_FRAME {
            gs.step();
        }
        // One batch: a frame never shows half of the update.
        fig2.batch(|| {
            hm.set_data(Field::new(&gs.v, N, N));
            mean.push(gs.step as f64, gs.mean_v());
            ax.title(format!("step {}", gs.step));
        });
    })
}
