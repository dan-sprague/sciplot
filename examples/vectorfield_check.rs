//! Visual check of `arrows` and `streamplot` against CairoMakie (`tools/vectorfield_check.jl`
//! renders the same figures to `out/vectorfield_*_makie.png`).
//!
//! (a) a quiver of the damped pendulum θ' = ω, ω' = −sin θ − 0.2ω on a 20 × 20 grid, colored by
//! magnitude, with a Colorbar; (b) a streamplot of the same field; (c) a streamplot of the Van der
//! Pol oscillator; (d) arrow metrics (long, short and styled arrows). Pass `--svg` to also write
//! SVGs and `--cpu` to also render with the CPU fallback.
use ezviz::prelude::*;

/// Damped pendulum.
fn pendulum(th: f64, om: f64) -> (f64, f64) {
    (om, -th.sin() - 0.2 * om)
}

/// Van der Pol oscillator, μ = 1.
fn vdp(x: f64, y: f64) -> (f64, f64) {
    (y, (1.0 - x * x) * y - x)
}

fn save(fig: &Figure, name: &str, svg: bool, cpu: bool) -> ezviz::Result<()> {
    fig.save_with(format!("out/{name}.png"), Save::new().px_per_unit(2.0))?;
    if svg {
        fig.save(format!("out/{name}.svg"))?;
    }
    if cpu {
        fig.save_with(format!("out/{name}_cpu.png"), Save::new().px_per_unit(2.0).cpu(true))?;
    }
    Ok(())
}

fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out").ok();
    let svg = std::env::args().any(|a| a == "--svg");
    let cpu = std::env::args().any(|a| a == "--cpu");
    let pi = std::f64::consts::PI;

    // (a) Quiver: normalized arrows colored by the field's magnitude, with a Colorbar.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "damped pendulum: arrows", xlabel = "θ", ylabel = "ω");
    let ar = arrows!(ax, linspace(-pi, pi, 20), linspace(-3.0, 3.0, 20), pendulum;
        color = Magnitude, normalize = true, lengthscale = 0.25, align = ArrowAlign::Center);
    Colorbar!(fig.at(1, 2), &ar; label = "|f|");
    save(&fig, "vectorfield_quiver", svg, cpu)?;

    // (b) Streamplot of the same field.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "damped pendulum: streamplot", xlabel = "θ", ylabel = "ω");
    let sp = streamplot!(ax, pendulum, -pi..=pi, -3.0..=3.0);
    save(&fig, "vectorfield_pendulum", svg, cpu)?;
    println!("pendulum    seeds = {}", sp.seeds());

    // (c) Van der Pol: magma, thinner lines, smaller arrowheads, lower density.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "Van der Pol (μ = 1)", xlabel = "x", ylabel = "y");
    let sp = streamplot!(ax, vdp, -3.0..=3.0, -4.0..=4.0;
        colormap = Colormap::MAGMA, linewidth = 1, arrow_size = 10, density = 0.8, gridsize = (24, 24));
    save(&fig, "vectorfield_vdp", svg, cpu)?;
    println!("vdp         seeds = {}", sp.seeds());

    // (d) Arrow metrics: default shape, arrows too short for the minimum shaft, styled arrows.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "arrows2d metrics").limits(0.0, 10.0, 0.0, 6.0);
    ax.arrows(
        [1.0, 1.0, 1.0, 1.0, 1.0],
        [1.0, 2.0, 3.0, 4.0, 5.0],
        [8.0, 0.3, 0.15, 4.0, 0.05],
        [0.0, 0.0, 0.0, 0.5, 0.0],
    );
    arrows!(ax, [6.0], [2.0], [2.0], [2.0];
        color = RED, shaftwidth = 6, tipwidth = 20, tiplength = 14, align = ArrowAlign::Center);
    arrows!(ax, [6.0], [5.0], [3.0], [0.0]; color = BLUE, taillength = 8, tailwidth = 12);
    save(&fig, "vectorfield_metrics", svg, cpu)?;

    // (e) Legend entries (ezviz draws a line with an arrowhead; Makie shows a gray patch for
    // arrows and an upward triangle for streamplot) and a solid-colored streamplot.
    let fig = Figure!(size = (600, 450));
    let ax = Axis!(fig.at(1, 1); title = "legend");
    let g = linspace(-2.0, 2.0, 9);
    arrows!(ax, &g, &g, |x, y| (-y, x); lengthscale = 0.2, color = Cycled(2), label = "rotation");
    streamplot!(ax, |x, y| (x, y), -2.0..=2.0, -2.0..=2.0; color = Cycled(1), density = 0.3, label = "source");
    axislegend(&ax);
    save(&fig, "vectorfield_legend", svg, cpu)?;

    println!("wrote out/vectorfield_{{quiver,pendulum,vdp,metrics}}.png");
    Ok(())
}
