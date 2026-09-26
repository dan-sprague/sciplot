//! Histogram, categorical barplot, dodged/stacked bars and a band.
use ezviz::prelude::*;

fn main() -> ezviz::Result<()> {
    // Deterministic normal samples (Box-Muller on a xorshift stream).
    let mut s: u64 = 0x9E3779B97F4A7C15;
    let mut u = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    let samples: Vec<f64> =
        (0..10_000).map(|_| (-2.0 * u().ln()).sqrt() * (2.0 * std::f64::consts::PI * u()).cos()).collect();

    let fig = Figure!(size = (1200, 800));
    let a1 = Axis!(fig.at(1, 1); title = "histogram (pdf)");
    hist!(a1, &samples; bins = 40, normalization = Normalization::Pdf);
    let a2 = Axis!(fig.at(1, 2); title = "solver runtime");
    barplot!(a2, ["CG", "GMRES", "BiCGStab", "Jacobi"], [1.2, 2.1, 1.6, 7.9]);
    let a3 = Axis!(fig.at(2, 1); title = "dodge");
    barplot!(a3, [1, 1, 2, 2, 3, 3], [1.0, 2.0, 2.0, 1.5, 3.0, 2.5]).dodge([1, 2, 1, 2, 1, 2]).color(Cycled(1));
    let a4 = Axis!(fig.at(2, 2); title = "band");
    let t = linspace(0.0, 10.0, 100);
    let m: Vec<f64> = t.iter().map(|t| (0.6 * t).sin() * (-0.1 * t).exp()).collect();
    let lo: Vec<f64> = m.iter().zip(&t).map(|(m, t)| m - 0.1 - 0.04 * t).collect();
    let hi: Vec<f64> = m.iter().zip(&t).map(|(m, t)| m + 0.1 + 0.04 * t).collect();
    band!(a4, &t, &lo, &hi; alpha = 0.5);
    scatter!(a4, &t, &m; markersize = 4, color = BLACK);
    std::fs::create_dir_all("out").ok();
    fig.save("out/stats_check.png")
}
