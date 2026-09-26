//! The CairoMakie side-by-side gallery (plan §7).
//!
//! Renders the plan's scenarios (S1–S4, S6–S8) and stress pages to `target/gallery/<name>.png`
//! and `.svg`, and writes the exact input arrays to `target/gallery/data/<name>.json`, so
//! `tools/makie_gallery.jl` can draw the same figures with CairoMakie. Then
//! `examples/compare.rs` builds the ezviz | Makie | diff page.
//!
//! ```text
//! cargo run --release --example gallery
//! julia --project=tools tools/makie_gallery.jl
//! cargo run --release --example compare      # -> target/gallery/index.html
//! ```
//!
//! All data is deterministic: randomness comes from xorshift (normal samples are Irwin–Hall
//! sums, so no libm is involved), and the Julia side reads the dumped arrays instead of
//! recomputing them. `cargo run --example gallery -- s3 stress_text` renders only the pages
//! whose names contain one of the arguments.
use ezviz::prelude::*;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const OUT: &str = "target/gallery";

/// Deterministic xorshift64 stream.
struct Rng(u64);

impl Rng {
    /// Uniform in [0, 1).
    fn u(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Approximately standard normal (Irwin–Hall: sum of 12 uniforms minus 6).
    fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.u()).sum::<f64>() - 6.0
    }
}

/// The arrays one page is drawn from (dumped as JSON for the Makie side).
#[derive(Default)]
struct Data(Map<String, Value>);

impl Data {
    fn put(&mut self, k: &str, v: &[f64]) -> &mut Self {
        // Non-finite values become `null` (read back as NaN in Julia).
        self.0.insert(k.into(), Value::Array(v.iter().map(|x| json!(x)).collect()));
        self
    }
    fn num(&mut self, k: &str, v: f64) -> &mut Self {
        self.0.insert(k.into(), json!(v));
        self
    }
}

/// Saves `fig` as PNG (+ SVG) and the page data.
fn save(name: &str, fig: &Figure, data: &Data, png: Option<Save>) -> ezviz::Result<()> {
    let dir = PathBuf::from(OUT);
    std::fs::create_dir_all(dir.join("data"))?;
    let png_path = dir.join(format!("{name}.png"));
    match png {
        Some(opts) => fig.save_with(&png_path, opts)?,
        None => fig.save(&png_path)?,
    }
    fig.save(dir.join(format!("{name}.svg")))?;
    let json =
        serde_json::to_string(&Value::Object(data.0.clone())).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(dir.join("data").join(format!("{name}.json")), json)?;
    println!("wrote {}", png_path.display());
    Ok(())
}

fn linspace_v(a: f64, b: f64, n: usize) -> Vec<f64> {
    linspace(a, b, n)
}

// ---------------------------------------------------------------------------------------------
// Scenarios

/// S1: the one-liner scatter.
fn s1_scatter() -> ezviz::Result<()> {
    let mut r = Rng(0x2545F4914F6CDD1D);
    let x: Vec<f64> = (0..1000).map(|_| r.u()).collect();
    let y: Vec<f64> = (0..1000).map(|_| r.u()).collect();
    let fig = scatter(&x, &y).figure();
    save("s1_scatter", &fig, Data::default().put("x", &x).put("y", &y), None)
}

/// S1b: titled axis, translucent markers, stroked stars.
fn s1_axis() -> ezviz::Result<()> {
    let mut r = Rng(0x2545F4914F6CDD1D);
    let x: Vec<f64> = (0..1000).map(|_| r.u()).collect();
    let y: Vec<f64> = (0..1000).map(|_| r.u()).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("uniform samples").xlabel("x").ylabel("y");
    ax.scatter(&x, &y).markersize(6).color((WONG[1], 0.6));
    ax.scatter([0.25, 0.5, 0.75], [0.5, 0.5, 0.5])
        .marker(Marker::Star5)
        .markersize(30)
        .strokewidth(1.5)
        .strokecolor(BLACK);
    save("s1_axis", &fig, Data::default().put("x", &x).put("y", &y), None)
}

/// S2: two lines, one dashed.
fn s2_lines() -> ezviz::Result<()> {
    let t = linspace_v(0.0, 4.0 * std::f64::consts::PI, 200);
    let s: Vec<f64> = t.iter().map(|t| t.sin()).collect();
    let c: Vec<f64> = t.iter().map(|t| t.cos()).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("Harmonic oscillator").xlabel("time t (s)").ylabel("displacement x (mm)");
    ax.lines(&t, &s).label("sin");
    ax.lines(&t, &c).label("cos").linestyle(Linestyle::Dash);
    // TODO(legend): axislegend!(ax; position = Pos::RT) once Legend lands (and in makie_gallery.jl).
    save("s2_lines", &fig, Data::default().put("t", &t).put("s", &s).put("c", &c), None)
}

/// S3: 2×2 panels with a spanning bottom axis, linked axes, hidden inner decorations, panel
/// labels.
fn s3_panels() -> ezviz::Result<()> {
    let t = linspace_v(0.0, 10.0, 300);
    let td: Vec<f64> = t.iter().step_by(15).copied().collect();
    let model = |w: f64| -> Vec<f64> { t.iter().map(|t| (w * t).sin() * (-0.1 * t).exp()).collect() };
    let meas =
        |w: f64| -> Vec<f64> { td.iter().map(|t| (w * t).sin() * (-0.1 * t).exp() + 0.05 * (7.0 * t).cos()).collect() };
    let mut d = Data::default();
    d.put("t", &t).put("td", &td);

    let fig = Figure::new().size((900, 650));
    let a = Axis::new(fig.at(1, 1)).title("ω = 1").ylabel("u (V)");
    let b = Axis::new(fig.at(1, 2)).title("ω = 2");
    let c = Axis::new(fig.at(2, 1..=2)).title("ω = 0.5").xlabel("t (s)").ylabel("u (V)");
    for (ax, w, key) in [(&a, 1.0, "1"), (&b, 2.0, "2"), (&c, 0.5, "05")] {
        let (m, y) = (model(w), meas(w));
        ax.lines(&t, &m).label("model");
        ax.scatter(&td, &y).label("measurement").markersize(7);
        d.put(&format!("model_{key}"), &m).put(&format!("data_{key}"), &y);
    }
    linkxaxes(&[&a, &b]);
    linkyaxes(&[&a, &b]);
    b.hideydecorations(false);
    // TODO(legend): Legend!(fig.at(1..=2, 3), &[&a, &b, &c]; unique = true, framevisible = false).
    for (pos, s) in [(fig.at(1, 1), "A"), (fig.at(1, 2), "B"), (fig.at(2, 1..=2), "C")] {
        Label::new(pos.side(Side::TopLeft), s)
            .fontsize(20)
            .font(Font::Bold)
            .padding((0, 5, 5, 0))
            .halign(HAlign::Right);
    }
    save("s3_panels", &fig, &d, None)
}

fn density(x: f64, y: f64) -> f64 {
    1.2 * (-(x - 1.0).powi(2) / 0.08 - (y - 0.5).powi(2) / 0.02).exp()
}

/// S4: heatmaps (outer edges and cell centres) and a value-colored scatter.
fn s4_heatmap() -> ezviz::Result<()> {
    let (nx, ny) = (400usize, 200usize);
    let (lx, ly) = (2.0, 1.0);
    let xc: Vec<f64> = (0..nx).map(|i| (i as f64 + 0.5) * lx / nx as f64).collect();
    let yc: Vec<f64> = (0..ny).map(|j| (j as f64 + 0.5) * ly / ny as f64).collect();
    let n: Vec<f64> = (0..nx * ny).map(|k| density(xc[k % nx], yc[k / nx])).collect();
    let k: Vec<f64> = (0..300).map(|k| k as f64).collect();
    let px: Vec<f64> = k.iter().map(|k| 1.0 + 0.9 * (k / 300.0).sqrt() * (k * 2.39996).cos()).collect();
    let py: Vec<f64> = k.iter().map(|k| 0.5 + 0.45 * (k / 300.0).sqrt() * (k * 2.39996).sin()).collect();
    let temp: Vec<f64> = px.iter().zip(&py).map(|(x, y)| 10.0 + 40.0 * density(*x, *y)).collect();

    let fig = Figure::new().size((800, 950));
    let ax1 = Axis::new(fig.at(1, 1)).title("flat Vec<f64>").ylabel("y (mm)");
    ax1.heatmap_xy(0.0..=lx, 0.0..=ly, Field::new(&n, nx, ny)).colormap(Colormap::MAGMA).colorrange((0.0, 1.2));
    // TODO(colorbar): Colorbar!(fig.at(1, 2), &hm1; label = "n (a.u.)") once Colorbar lands.
    let ax2 = Axis::new(fig.at(2, 1)).title("cell centres").ylabel("y (mm)");
    ax2.heatmap_xy(&xc, &yc, Field::new(&n, nx, ny)).colormap(Colormap::MAGMA).colorrange((0.0, 1.2));
    let ax3 = Axis::new(fig.at(3, 1)).title("probes").xlabel("x (mm)").ylabel("y (mm)");
    ax3.scatter(&px, &py).color(&temp).colormap(Colormap::VIRIDIS).markersize(10);
    fig.colsize(1, GridSize::Aspect(1, lx / ly));
    linkaxes(&[&ax1, &ax2, &ax3]);
    ax1.hidexdecorations(false);
    ax2.hidexdecorations(false);
    let mut d = Data::default();
    d.num("nx", nx as f64).num("ny", ny as f64).num("lx", lx).num("ly", ly);
    d.put("n", &n).put("xc", &xc).put("yc", &yc).put("px", &px).put("py", &py).put("temp", &temp);
    save("s4_heatmap", &fig, &d, None)
}

/// S6: log-log with minor ticks and minor grid, and semilog-y.
fn s6_log() -> ezviz::Result<()> {
    let k = logspace(-1.0, 3.0, 81);
    let e: Vec<f64> = k.iter().map(|k| 2.0 * k.powf(-5.0 / 3.0)).collect();
    let t = linspace_v(0.0, 10.0, 60);
    let s: Vec<f64> = t.iter().map(|t| 1e3 * (-1.2 * t).exp() + 1e-2).collect();
    let fig = Figure::new().size((900, 400));
    let ax1 = Axis::new(fig.at(1, 1))
        .title("Kolmogorov spectrum")
        .xlabel("k (1/m)")
        .ylabel("E(k)")
        .xscale(Scale::Log10)
        .yscale(Scale::Log10)
        .xminorticksvisible(true)
        .yminorticksvisible(true)
        .xminorgridvisible(true)
        .yminorgridvisible(true);
    ax1.lines(&k, &e);
    let ax2 = Axis::new(fig.at(1, 2))
        .title("semilog-y")
        .xlabel("t (s)")
        .ylabel("signal")
        .yscale(Scale::Log10)
        .yminorticksvisible(true);
    ax2.scatterlines(&t, &s).markersize(5);
    save("s6_log", &fig, Data::default().put("k", &k).put("e", &e).put("t", &t).put("s", &s), None)
}

/// S7: density histogram with a pdf overlay, categorical barplot, confidence band.
fn s7_stats() -> ezviz::Result<()> {
    let mut r = Rng(0x9E3779B97F4A7C15);
    let samples: Vec<f64> = (0..10_000).map(|_| r.normal()).collect();
    let xs = linspace_v(-4.0, 4.0, 200);
    let pdf: Vec<f64> = xs.iter().map(|x| (-x * x / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt()).collect();
    let t = linspace_v(0.0, 10.0, 100);
    let mean: Vec<f64> = t.iter().map(|t| (0.6 * t).sin() * (-0.1 * t).exp()).collect();
    let sd: Vec<f64> = t.iter().map(|t| 0.05 + 0.02 * t).collect();
    let lo: Vec<f64> = mean.iter().zip(&sd).map(|(m, s)| m - 1.96 * s).collect();
    let hi: Vec<f64> = mean.iter().zip(&sd).map(|(m, s)| m + 1.96 * s).collect();
    let heights = [1.2, 2.1, 1.6, 7.9];

    let fig = Figure::new().size((1200, 400));
    let ax1 = Axis::new(fig.at(1, 1)).title("histogram").xlabel("x").ylabel("probability density");
    ax1.hist(&samples).bins(40).normalization(Normalization::Pdf).label("samples");
    ax1.lines(&xs, &pdf).color(BLACK).linewidth(2).label("N(0, 1)");
    let ax2 = Axis::new(fig.at(1, 2)).title("solver runtime").ylabel("time (s)");
    ax2.barplot(["CG", "GMRES", "BiCGStab", "Jacobi"], heights);
    let ax3 = Axis::new(fig.at(1, 3)).title("ensemble mean ± 95% CI").xlabel("t (s)").ylabel("u");
    ax3.band(&t, &lo, &hi).color((WONG[0], 0.3)).label("95% CI");
    ax3.lines(&t, &mean).color(WONG[0]).label("mean");
    // TODO(legend): axislegend!(ax1) and axislegend!(ax3; position = Pos::RB).
    let mut d = Data::default();
    d.put("samples", &samples).put("xs", &xs).put("pdf", &pdf).put("heights", &heights);
    d.put("t", &t).put("mean", &mean).put("lo", &lo).put("hi", &hi);
    save("s7_stats", &fig, &d, None)
}

/// S8: a paper figure with a scoped theme, 4 in × 3 in at 12 pt, 300 dpi.
fn s8_paper() -> ezviz::Result<()> {
    let t = linspace_v(0.0, 5.0, 200);
    let n: Vec<f64> = t.iter().map(|t| 1e19 * (1.0 + 0.5 * (-t).exp() * (6.0 * t).cos())).collect();
    let paper = theme_minimal().fontsize(12.0 * PT).figure_padding(4.0 * PT).linewidth(1.0 * PT).axis(|a| {
        a.xticksvisible(true)
            .yticksvisible(true)
            .spinewidth(0.75 * PT)
            .xtickwidth(0.75 * PT)
            .ytickwidth(0.75 * PT)
            .xticksize(3.0 * PT)
            .yticksize(3.0 * PT)
    });
    with_theme(paper, || {
        let fig = Figure::new().size((4.0 * INCH, 3.0 * INCH));
        let ax = Axis::new(fig.at(1, 1)).xlabel("time t (ms)").ylabel(rich!("n (m", superscript("−3"), ")"));
        ax.lines(&t, &n);
        save("s8_paper", &fig, Data::default().put("t", &t).put("n", &n), Some(Save::dpi(300)))
    })
}

/// Makie's `theme_minimal` on a simple figure.
fn theme_minimal_page() -> ezviz::Result<()> {
    let x = linspace_v(0.0, 10.0, 100);
    let y1: Vec<f64> = x.iter().map(|x| (x * 0.8).sin() + 0.1 * x).collect();
    let y2: Vec<f64> = x.iter().map(|x| (x * 0.8).cos() - 0.1 * x).collect();
    let sx: Vec<f64> = x.iter().step_by(5).copied().collect();
    let sy: Vec<f64> = y1.iter().step_by(5).map(|y| y + 0.3).collect();
    with_theme(theme_minimal(), || {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1)).title("theme_minimal").xlabel("x").ylabel("y");
        ax.lines(&x, &y1);
        ax.lines(&x, &y2);
        ax.scatter(&sx, &sy);
        let mut d = Data::default();
        d.put("x", &x).put("y1", &y1).put("y2", &y2).put("sx", &sx).put("sy", &sy);
        save("theme_minimal", &fig, &d, None)
    })
}

// ---------------------------------------------------------------------------------------------
// Stress pages

/// Every marker at three sizes; the largest row stroked.
fn stress_markers() -> ezviz::Result<()> {
    let markers = [
        Marker::Circle,
        Marker::Rect,
        Marker::Diamond,
        Marker::Cross,
        Marker::XCross,
        Marker::UTriangle,
        Marker::DTriangle,
        Marker::LTriangle,
        Marker::RTriangle,
        Marker::Pentagon,
        Marker::Hexagon,
        Marker::Star5,
        Marker::FullCircle,
        Marker::FullRect,
    ];
    let fig = Figure::new().size((900, 400));
    let ax = Axis::new(fig.at(1, 1)).title("markers (sizes 8, 16, 28; stroked)").limits(0.0, 15.0, 0.0, 4.0);
    for (i, m) in markers.iter().enumerate() {
        let x = (i + 1) as f64;
        ax.scatter([x], [1.0]).marker(*m).markersize(8).color(WONG[0]);
        ax.scatter([x], [2.0]).marker(*m).markersize(16).color(WONG[0]);
        ax.scatter([x], [3.0]).marker(*m).markersize(28).color(WONG[2]).strokewidth(2).strokecolor(BLACK);
    }
    save("stress_markers", &fig, &Data::default(), None)
}

/// NaN gaps in lines, scatter and scatterlines.
fn stress_nan() -> ezviz::Result<()> {
    let x = linspace_v(0.0, 10.0, 101);
    let mut y: Vec<f64> = x.iter().map(|x| x.sin()).collect();
    for i in [20, 21, 22, 50, 80] {
        y[i] = f64::NAN;
    }
    let mut y2: Vec<f64> = x.iter().map(|x| (0.5 * x).cos() - 2.0).collect();
    for v in y2.iter_mut().skip(40).take(15) {
        *v = f64::NAN;
    }
    let sx: Vec<f64> = x.iter().step_by(4).copied().collect();
    let mut sy: Vec<f64> = sx.iter().map(|x| 0.5 * (1.3 * x).sin() + 2.0).collect();
    sy[3] = f64::NAN;
    sy[10] = f64::NAN;
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("NaN gaps");
    ax.lines(&x, &y).linewidth(3);
    ax.scatterlines(&x, &y2);
    ax.scatter(&sx, &sy).markersize(10);
    let mut d = Data::default();
    d.put("x", &x).put("y", &y).put("y2", &y2).put("sx", &sx).put("sy", &sy);
    save("stress_nan", &fig, &d, None)
}

/// Huge offsets and tiny spans (Float32 rebasing).
fn stress_offset() -> ezviz::Result<()> {
    let x: Vec<f64> = (0..200).map(|i| 1e9 + i as f64 * 0.01).collect();
    let y: Vec<f64> = (0..200).map(|i| (i as f64 * 0.1).sin()).collect();
    let x2: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let y2: Vec<f64> = (0..100).map(|i| 5.0 + 1e-6 * (i as f64 * 0.2).cos()).collect();
    let fig = Figure::new().size((900, 400));
    let a = Axis::new(fig.at(1, 1)).title("x = 1e9 + small");
    a.lines(&x, &y);
    let b = Axis::new(fig.at(1, 2)).title("y = 5 + 1e-6 · cos");
    b.scatter(&x2, &y2).markersize(5);
    let mut d = Data::default();
    d.put("x", &x).put("y", &y).put("x2", &x2).put("y2", &y2);
    save("stress_offset", &fig, &d, None)
}

/// A heatmap on log-scaled axes with irregular (log-spaced) edges.
fn stress_logheatmap() -> ezviz::Result<()> {
    let (nx, ny) = (30usize, 20usize);
    let xe = logspace(0.0, 3.0, nx + 1);
    let ye = logspace(-2.0, 1.0, ny + 1);
    let z: Vec<f64> = (0..nx * ny)
        .map(|k| {
            let (i, j) = ((k % nx) as f64, (k / nx) as f64);
            (i * 0.4).sin() * (j * 0.3).cos() + 0.02 * i
        })
        .collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("log-log heatmap").xscale(Scale::Log10).yscale(Scale::Log10);
    ax.heatmap_xy(&xe, &ye, Field::new(&z, nx, ny));
    let mut d = Data::default();
    d.num("nx", nx as f64).num("ny", ny as f64).put("xe", &xe).put("ye", &ye).put("z", &z);
    save("stress_logheatmap", &fig, &d, None)
}

/// Line widths, dash styles, joins and caps, and a dense chirp.
fn stress_lines() -> ezviz::Result<()> {
    let fig = Figure::new().size((900, 700));
    let a = Axis::new(fig.at(1, 1)).title("linewidths 0.5 1 2 4 8");
    for (i, w) in [0.5, 1.0, 2.0, 4.0, 8.0].iter().enumerate() {
        a.lines([0.0, 1.0], [i as f64, i as f64 + 0.5]).linewidth(*w).color(BLACK);
    }
    let b = Axis::new(fig.at(1, 2)).title("linestyles");
    let styles = [Linestyle::Solid, Linestyle::Dash, Linestyle::Dot, Linestyle::DashDot, Linestyle::DashDotDot];
    let xs = linspace_v(0.0, 1.0, 50);
    for (i, s) in styles.iter().enumerate() {
        let ys: Vec<f64> = xs.iter().map(|x| i as f64 + 0.3 * (6.0 * x).sin()).collect();
        b.lines(&xs, &ys).linestyle(s.clone()).linewidth(2);
    }
    let c = Axis::new(fig.at(2, 1)).title("joins: miter, bevel, round; caps: butt, square, round");
    let zx = [0.0, 1.0, 2.0, 3.0, 4.0];
    let zy = [0.0, 1.0, 0.0, 1.0, 0.0];
    let joins =
        [(JoinStyle::Miter, LineCap::Butt), (JoinStyle::Bevel, LineCap::Square), (JoinStyle::Round, LineCap::Round)];
    for (i, (j, cap)) in joins.iter().enumerate() {
        let y: Vec<f64> = zy.iter().map(|y| y + 1.6 * i as f64).collect();
        c.lines(zx, &y).linewidth(12).joinstyle(*j).linecap(*cap);
    }
    let dx = linspace_v(0.0, 1.0, 4000);
    let dy: Vec<f64> = dx.iter().map(|x| (60.0 * x * x * std::f64::consts::TAU).sin() * (1.0 - x)).collect();
    let e = Axis::new(fig.at(2, 2)).title("chirp, 4000 points");
    e.lines(&dx, &dy).linewidth(1);
    save("stress_lines", &fig, Data::default().put("xs", &xs).put("dx", &dx).put("dy", &dy), None)
}

/// Text annotations: alignments, rotation, fonts and sizes.
fn stress_text() -> ezviz::Result<()> {
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("text").limits(0.0, 4.0, 0.0, 4.0);
    let aligns = [(HAlign::Left, VAlign::Bottom), (HAlign::Center, VAlign::Center), (HAlign::Right, VAlign::Top)];
    for (i, al) in aligns.iter().enumerate() {
        let x = 1.0 + i as f64;
        ax.scatter([x], [3.0]).color(RED).markersize(6);
        ax.text([x], [3.0], "Align").align(*al);
    }
    ax.scatter([1.0], [1.5]).color(RED).markersize(6);
    ax.text([1.0], [1.5], "rotated 45°").rotation(std::f64::consts::FRAC_PI_4).fontsize(18);
    let fonts = [(Font::Regular, 10.0), (Font::Bold, 14.0), (Font::Italic, 18.0), (Font::BoldItalic, 24.0)];
    for (i, (f, s)) in fonts.iter().enumerate() {
        ax.text([2.2], [0.4 + 0.5 * i as f64], "Font 0.5 − 1").font(*f).fontsize(*s);
    }
    save("stress_text", &fig, &Data::default(), None)
}

/// Dodged, stacked and horizontal bars; a stroked histogram.
fn stress_bars() -> ezviz::Result<()> {
    let mut r = Rng(0xD1B54A32D192ED03);
    let samples: Vec<f64> = (0..2000).map(|_| 2.0 * r.normal() + 5.0).collect();
    let x = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
    let h = [1.0, 2.0, 2.0, 1.5, 3.0, 2.5];
    let g = [1.0, 2.0, 1.0, 2.0, 1.0, 2.0];
    let colors: Vec<Color> = g.iter().map(|g| WONG[*g as usize - 1]).collect();
    let fig = Figure::new().size((900, 700));
    let a = Axis::new(fig.at(1, 1)).title("dodge");
    a.barplot(x, h).dodge(g).color(colors.clone());
    let b = Axis::new(fig.at(1, 2)).title("stack");
    b.barplot(x, h).stack(g).color(colors);
    let c = Axis::new(fig.at(2, 1)).title("direction = x");
    c.barplot([1.0, 2.0, 3.0, 4.0], [4.0, 3.0, 5.0, 1.0]).direction(Direction::X).color(WONG[3]);
    let e = Axis::new(fig.at(2, 2)).title("hist, 20 bins, stroked");
    e.hist(&samples).bins(20).strokewidth(1).strokecolor(BLACK).color((WONG[2], 0.7));
    save("stress_bars", &fig, Data::default().put("samples", &samples), None)
}

/// 50 000 translucent points (blending and marker AA at small sizes).
fn stress_dense() -> ezviz::Result<()> {
    let mut r = Rng(0x94D049BB133111EB);
    let n = 50_000;
    let x: Vec<f64> = (0..n).map(|_| r.normal()).collect();
    let y: Vec<f64> = x.iter().map(|x| 0.6 * x + 0.8 * r.normal()).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).title("50 000 points, alpha 0.1");
    ax.scatter(&x, &y).markersize(3).color((BLACK, 0.1));
    save("stress_dense", &fig, Data::default().put("x", &x).put("y", &y), None)
}

type PageFn = fn() -> ezviz::Result<()>;

const PAGES: &[(&str, PageFn)] = &[
    ("s1_scatter", s1_scatter),
    ("s1_axis", s1_axis),
    ("s2_lines", s2_lines),
    ("s3_panels", s3_panels),
    ("s4_heatmap", s4_heatmap),
    ("s6_log", s6_log),
    ("s7_stats", s7_stats),
    ("s8_paper", s8_paper),
    ("theme_minimal", theme_minimal_page),
    ("stress_markers", stress_markers),
    ("stress_nan", stress_nan),
    ("stress_offset", stress_offset),
    ("stress_logheatmap", stress_logheatmap),
    ("stress_lines", stress_lines),
    ("stress_text", stress_text),
    ("stress_bars", stress_bars),
    ("stress_dense", stress_dense),
];

fn main() -> ezviz::Result<()> {
    let filters: Vec<String> = std::env::args().skip(1).collect();
    std::fs::create_dir_all(Path::new(OUT).join("data"))?;
    let mut names = Vec::new();
    for (name, page) in PAGES {
        if filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str())) {
            page()?;
        }
        names.push(*name);
    }
    // The page list, in order, for makie_gallery.jl and compare.rs.
    std::fs::write(Path::new(OUT).join("data").join("pages.json"), serde_json::to_string(&names).unwrap_or_default())?;
    Ok(())
}
