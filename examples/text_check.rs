//! Text fidelity check: a default 600×450 figure with a title, axis labels and a few points,
//! rendered at px_per_unit 2 to `out/text_check.png`. `tools/text_check.jl` renders the same
//! figure with CairoMakie to `out/text_check_makie.png` for side-by-side comparison.
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let t: Vec<f64> = (0..11).map(|i| i as f64).collect();
    let x: Vec<f64> = t.iter().map(|t| 5.0 * (0.8 * t).cos() * (-0.1 * t).exp()).collect();

    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "Harmonic oscillator", xlabel = "time t (s)", ylabel = "displacement x (mm)");
    scatter!(ax, &t, &x);

    std::fs::create_dir_all("out").ok();
    fig.save("out/text_check.png")?;

    // Rich text and tex() markup on the same kind of figure.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1);
        title = rich!("E = mc", superscript("2"), colored(" (rest energy)", WONG[5])),
        xlabel = tex(r"wavenumber k (\mu m^{-1})"),
        ylabel = tex(r"E(k) = C k^{-5/3}, \alpha = 0.5 \pm 0.1"));
    scatter!(ax, &t, &x);
    fig.save("out/text_check_rich.png")?;
    println!("wrote out/text_check.png and out/text_check_rich.png");
    Ok(())
}
