//! Legends: axislegend (S2), a shared unique Legend over three axes (S3), a horizontal legend with
//! a title under an axis, and bar/band/marker entries. Writes out/legend_check_*.png (+ .svg);
//! tools/legend_check.jl renders the same figures with CairoMakie (out/legend_check_*_makie.png).
use ezviz::prelude::*;

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();
    let t = linspace(0.0, 10.0, 200);
    let td = linspace(0.25, 9.75, 20);
    let map = |v: &[f64], f: &dyn Fn(f64) -> f64| v.iter().map(|x| f(*x)).collect::<Vec<f64>>();

    // S2: sin/cos with axislegend (top right).
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); xlabel = "t", ylabel = "u");
    lines!(ax, &t, map(&t, &f64::sin); label = "sin");
    lines!(ax, &t, map(&t, &f64::cos); label = "cos");
    axislegend!(ax);
    save(&fig, "s2")?;

    // S3-like: three axes with lines + scatter, one shared legend (unique) spanning two rows.
    let fig = Figure!(size = (900, 650));
    let a = Axis!(fig.at(1, 1); title = "ω = 1", ylabel = "u (V)");
    let b = Axis!(fig.at(1, 2); title = "ω = 2");
    let c = Axis!(fig.at(2, 1..=2); xlabel = "t (s)");
    for (k, ax) in [&a, &b, &c].into_iter().enumerate() {
        let k = (k + 1) as f64;
        lines!(ax, &t, map(&t, &|x| (k * x).sin()); label = "model");
        scatter!(ax, &td, map(&td, &|x| (k * x).sin() + 0.1 * (7.0 * x).cos()); label = "measurement", markersize = 7);
    }
    lines!(c, &t, map(&t, &|x| 0.5 * x.cos()); label = "envelope", linestyle = Linestyle::Dash);
    Legend!(fig.at(1..=2, 3), &[&a, &b, &c]; unique = true);
    save(&fig, "s3")?;

    // Horizontal legend under an axis, with a title.
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    lines!(ax, &t, map(&t, &f64::sin); label = "solid");
    lines!(ax, &t, map(&t, &|x| (x - 1.0).sin()); label = "dash", linestyle = Linestyle::Dash);
    lines!(ax, &t, map(&t, &|x| (x - 2.0).sin()); label = "dot", linestyle = Linestyle::Dot, linewidth = 3);
    let k: Vec<f64> = (1..=9).map(f64::from).collect();
    scatterlines!(ax, &k, map(&k, &|x| 0.2 * x.cos()); label = "scatterlines");
    Legend!(fig.at(2, 1), &ax; title = "Styles", orientation = Orientation::Horizontal);
    save(&fig, "horizontal")?;

    // Bars, band and stroked markers with an axislegend at the left top, with a title.
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    band!(ax, &t, map(&t, &|x| x.sin() - 0.3 + 3.0), map(&t, &|x| x.sin() + 0.3 + 3.0); label = "band");
    barplot!(ax, &k, map(&k, &|x| 1.0 + 0.1 * x); label = "bars");
    barplot!(ax, &k, map(&k, &|x| 0.5 + 0.05 * x); label = "bars 2", strokewidth = 1, strokecolor = BLACK);
    scatter!(ax, &k, map(&k, &|x| 2.0 + 0.1 * x);
        label = "stroked", marker = Marker::Rect, markersize = 12, strokewidth = 1, color = ORANGE);
    axislegend!(ax; title = "Kinds", position = Pos::LT);
    save(&fig, "bars")
}

fn save(fig: &Figure, name: &str) -> ezviz::Result<()> {
    fig.save(format!("out/legend_check_{name}.png"))?;
    fig.save(format!("out/legend_check_{name}.svg"))?;
    println!("wrote out/legend_check_{name}.png");
    Ok(())
}
