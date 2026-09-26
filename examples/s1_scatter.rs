//! S1: one-liner scatter to PNG.
use ezviz::prelude::*;

fn main() -> ezviz::Result<()> {
    // Deterministic pseudo-random points (xorshift) so the example needs no extra crates.
    let mut s: u64 = 0x2545F4914F6CDD1D;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let x: Vec<f64> = (0..1000).map(|_| rnd()).collect();
    let y: Vec<f64> = (0..1000).map(|_| rnd()).collect();

    std::fs::create_dir_all("out").ok();
    scatter(&x, &y).save("out/s1_scatter.png")?;

    // Keyword form and an explicit axis.
    let fig = Figure!(size = (600, 450));
    let ax = Axis!(fig.at(1, 1); title = "uniform samples", xlabel = "x", ylabel = "y");
    scatter!(ax, &x, &y; markersize = 6, color = (WONG[1], 0.6));
    scatter!(ax, [0.25, 0.5, 0.75], [0.5, 0.5, 0.5]; marker = Marker::Star5, markersize = 30, strokewidth = 1.5, strokecolor = BLACK);
    fig.save("out/s1_axis.png")?;
    println!("wrote out/s1_scatter.png and out/s1_axis.png");
    Ok(())
}
