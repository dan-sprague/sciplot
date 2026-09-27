//! Makie-fidelity check: renders reference figures (default axis with title and labels, log-log
//! with minor ticks, theme_minimal, a DataAspect heatmap, hlines/vlines/ablines, autolimitaspect,
//! theme_light / theme_dark) to `out/fidelity_<name>.png`.
//!
//! `julia --project=tools tools/fidelity_check.jl` renders the same figures with CairoMakie to
//! `out/fidelity_<name>_makie.png` and dumps Makie's axis geometry (viewport, limits, tick label /
//! axis label / title boxes) to `out/fidelity_makie.json` and `tests/fixtures/fidelity_makie.json`.
//! This example compares against that dump and prints the largest differences in figure units
//! (`tests/fidelity.rs` asserts them).
//!
//! Run: `cargo run --example fidelity_check`
#![allow(dead_code)]
use sciplot::prelude::*;
use sciplot::{AxisGeometry, theme_dark, theme_light};
use serde_json::Value;

/// The reference figures: `(name, figure, axis)`.
pub fn figures() -> Vec<(&'static str, Figure, Axis)> {
    let t: Vec<f64> = (0..11).map(|i| i as f64).collect();
    let xs: Vec<f64> = t.iter().map(|t| 5.0 * (0.8 * t).cos() * (-0.1 * t).exp()).collect();
    let mut out = Vec::new();

    // 1. Default axis with title and labels.
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "Harmonic oscillator", xlabel = "time t (s)", ylabel = "displacement x (mm)");
    scatter!(ax, &t, &xs);
    out.push(("default", fig, ax));

    // 2. Log-log with minor ticks and minor grid (Makie-exact log ticks and minors for the comparison).
    let lx: Vec<f64> = (0..50).map(|i| 10f64.powf(-1.0 + 4.0 * i as f64 / 49.0)).collect();
    let ly: Vec<f64> = lx.iter().map(|x| 100.0 * x.powf(-1.5)).collect();
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "power law", xlabel = "k", ylabel = "E(k)",
        xscale = Scale::Log10, yscale = Scale::Log10, xticks = TickSpec::LogMakie, yticks = TickSpec::LogMakie,
        // Makie's default minors (sciplot's own default on log axes is 2..9·10ⁿ).
        xminorticks = MinorSpec::IntervalsBetween(2), yminorticks = MinorSpec::IntervalsBetween(2),
        xminorticksvisible = true, yminorticksvisible = true, xminorgridvisible = true, yminorgridvisible = true);
    lines!(ax, &lx, &ly);
    out.push(("loglog", fig, ax));

    // 3. theme_minimal.
    let (fig, ax) = with_theme(theme_minimal(), || {
        let fig = Figure::new();
        let ax = Axis!(fig.at(1, 1); title = "minimal", xlabel = "time t (s)", ylabel = "x");
        lines!(ax, &t, &xs);
        scatter!(ax, &t, &xs);
        (fig, ax)
    });
    out.push(("minimal", fig, ax));

    // 4. DataAspect heatmap (40 × 20 cells centred on 1..=40, 1..=20).
    let z: Vec<f64> = (0..20 * 40)
        .map(|k| {
            let (i, j) = ((k % 40 + 1) as f64, (k / 40 + 1) as f64);
            (0.3 * i).sin() * (0.4 * j).cos()
        })
        .collect();
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "DataAspect", aspect = DataAspect);
    heatmap!(ax, Field::new(&z, 40, 20));
    out.push(("dataaspect", fig, ax));

    // 5. hlines / vlines / ablines.
    let tt: Vec<f64> = (0..100).map(|i| 10.0 * i as f64 / 99.0).collect();
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "reference lines");
    lines!(ax, &tt, tt.iter().map(|t| t.sin()));
    hlines!(ax, [0.5, -0.5]; xmin = 0.1, xmax = 0.9);
    vlines!(ax, [2.0, 4.0]);
    ablines!(ax, -1.0, 0.2);
    out.push(("reflines", fig, ax));

    // 6. autolimitaspect = 1 on a unit circle.
    let th: Vec<f64> = (0..100).map(|i| std::f64::consts::TAU * i as f64 / 99.0).collect();
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "autolimitaspect", autolimitaspect = 1);
    lines!(ax, th.iter().map(|t| t.cos()), th.iter().map(|t| t.sin()));
    out.push(("autolimitaspect", fig, ax));

    // 7. theme_light and theme_dark.
    for (name, th) in [("light", theme_light()), ("dark", theme_dark())] {
        let (fig, ax) = with_theme(th, || {
            let fig = Figure::new();
            let ax = Axis!(fig.at(1, 1); title = name, xlabel = "time t (s)", ylabel = "x");
            lines!(ax, &t, &xs);
            lines!(ax, &t, xs.iter().map(|v| -v));
            (fig, ax)
        });
        out.push((name, fig, ax));
    }
    out
}

/// A Makie box (y up) as `[x, y, w, h]` with y down, or None for an empty text.
fn makie_box(v: &Value, fig_h: f64) -> Option<[f64; 4]> {
    let f = |k: &str| v[k].as_f64().unwrap_or(f64::NAN);
    let (x, y, w, h) = (f("x"), f("y"), f("w"), f("h"));
    (w > 0.0 || h > 0.0).then_some([x, fig_h - (y + h), w, h])
}

fn diff(a: [f64; 4], b: [f64; 4]) -> f64 {
    a.iter().zip(b).map(|(p, q)| (p - q).abs()).fold(0.0, f64::max)
}

/// Compares one axis with Makie's dump: prints the largest difference per element and returns
/// the overall maximum in units (infinite for a structural mismatch or limits off by more than
/// 1e-6 of their range).
pub fn compare(name: &str, g: &AxisGeometry, m: &Value, fig_h: f64, verbose: bool) -> f64 {
    let lims: Vec<f64> =
        m["limits"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let rel = if lims.len() == 4 {
        (0..4).map(|i| (g.limits[i] - lims[i]).abs() / (lims[i | 1] - lims[i & 2]).abs()).fold(0.0, f64::max)
    } else {
        f64::INFINITY
    };
    if verbose || rel > 1e-6 {
        println!("  {name:16} {:12} max |Δ|/range = {rel:.2e}", "limits");
    }
    let mut worst = if rel <= 1e-6 { 0.0f64 } else { f64::INFINITY };
    let mut report = |what: &str, d: Option<f64>| {
        let d = d.unwrap_or(f64::INFINITY);
        worst = worst.max(d);
        if verbose || d > 1e-3 {
            println!("  {name:16} {what:12} max |Δ| = {d:.2e}");
        }
    };
    let vp = makie_box(&m["viewport"], fig_h).unwrap_or([f64::NAN; 4]);
    report("viewport", Some(diff(g.viewport, vp)).filter(|d| d.is_finite()));
    for (what, ours) in [("xticklabels", &g.xticklabels), ("yticklabels", &g.yticklabels)] {
        let theirs: Vec<[f64; 4]> =
            m[what].as_array().map(|a| a.iter().filter_map(|b| makie_box(b, fig_h)).collect()).unwrap_or_default();
        let d = (ours.len() == theirs.len())
            .then(|| ours.iter().zip(&theirs).map(|(a, b)| diff(*a, *b)).fold(0.0, f64::max));
        report(what, d);
    }
    for (what, ours) in [("xlabel", g.xlabel), ("ylabel", g.ylabel), ("title", g.title)] {
        let theirs = m[what].as_array().and_then(|a| a.first()).and_then(|b| makie_box(b, fig_h));
        match (ours, theirs) {
            (None, None) => {}
            (Some(a), Some(b)) => report(what, Some(diff(a, b))),
            _ => report(what, None),
        }
    }
    worst
}

/// Compares every reference figure with Makie's dump; returns the largest difference in units.
pub fn check(makie: &Value, verbose: bool) -> f64 {
    let mut worst = 0.0f64;
    for (name, _fig, ax) in figures() {
        let m = &makie[name];
        let Some(g) = ax.geometry() else { return f64::INFINITY };
        if m.is_null() {
            println!("  {name}: not in the Makie dump");
            return f64::INFINITY;
        }
        worst = worst.max(compare(name, &g, m, 450.0, verbose));
    }
    worst
}

fn main() -> sciplot::Result<()> {
    std::fs::create_dir_all("out").ok();
    for (name, fig, _) in figures() {
        fig.save(format!("out/fidelity_{name}.png"))?;
    }
    println!("wrote out/fidelity_*.png");
    let makie: Option<Value> = ["out/fidelity_makie.json", "tests/fixtures/fidelity_makie.json"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());
    match makie {
        Some(m) => println!("largest position difference vs CairoMakie: {:.2e} units", check(&m, true)),
        None => println!("no Makie dump: run `julia --project=tools tools/fidelity_check.jl` to compare"),
    }
    Ok(())
}
