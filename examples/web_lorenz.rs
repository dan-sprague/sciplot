//! The Lorenz attractor, integrated live inside `fig.animate` — the same code natively and in
//! the browser.
//!
//! Left: the (x, z) projection of the trajectory as a growing line; the last few states are
//! marked. Right: x(t) and z(t) as growing time series.
//! Scroll to zoom, drag to pan or zoom into a rectangle, hover for values; on touch screens pan
//! with one finger and pinch with two.
//!
//! Native: `cargo run --release --example web_lorenz` (`SCIPLOT_AUTOCLOSE=4
//! SCIPLOT_WINDOW_DUMP=out/web_lorenz_native.png` closes after 4 s and writes the last frame).
//! Browser: `tools/web/build.sh web_lorenz`, then serve `examples/web/` and open
//! `web_lorenz.html` (add `?backend=gl` to force WebGL2).
use sciplot::prelude::*;

/// RK4 steps per displayed frame.
const STEPS_PER_FRAME: usize = 8;

/// The Lorenz system (σ = 10, ρ = 28, β = 8/3), integrated with RK4.
struct Lorenz {
    p: [f64; 3],
    t: f64,
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
        self.t += h;
    }
}

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

fn main() -> sciplot::Result<()> {
    let fig = Figure::new().size((960, 440)).window_title("sciplot: Lorenz attractor");
    let ax = Axis::new(fig.at(1, 1)).title("Lorenz attractor").xlabel("x").ylabel("z");
    let ts = Axis::new(fig.at(1, 2)).title("time series").xlabel("t").ylabel("x, z");
    fig.colsize(1, GridSize::Aspect(1, 1.0));

    // Start near the origin with a small random offset.
    let mut rnd = xorshift(0x2545_F491_4F6C_DD1D);
    let mut lz = Lorenz { p: [1.0 + 0.1 * rnd(), 1.0 + 0.1 * rnd(), 1.0 + 0.1 * rnd()], t: 0.0 };
    let traj = ax.lines([lz.p[0]], [lz.p[2]]).color(WONG[0]).linewidth(1.0);
    let head = ax.scatterlines([lz.p[0]], [lz.p[2]]).color(WONG[5]).markersize(9).linewidth(2.5);
    let xt = ts.lines([0.0], [lz.p[0]]).color(WONG[0]).label("x");
    let zt = ts.lines([0.0], [lz.p[2]]).color(WONG[1]).label("z");
    axislegend(&ts);

    let fig2 = fig.clone();
    let mut tail: Vec<[f64; 2]> = Vec::new();
    fig.animate(move |frame| {
        for _ in 0..STEPS_PER_FRAME {
            lz.step();
            traj.push(lz.p[0], lz.p[2]);
            xt.push(lz.t, lz.p[0]);
            zt.push(lz.t, lz.p[2]);
        }
        // The head: the state at the last 6 frames, marked.
        tail.push([lz.p[0], lz.p[2]]);
        let keep = tail.len().saturating_sub(6);
        tail.drain(..keep);
        fig2.batch(|| {
            head.set_points(&tail[..]);
            ts.title(format!("time series  (t = {:.1}, frame {})", lz.t, frame.count));
        });
    })
}
