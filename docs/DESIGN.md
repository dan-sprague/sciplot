# sciplot: final design (v1)

This is the design to hand to implementation planning. I made no files. I did two web checks: the wgpu 30 `SurfaceColorSpace` docs and the winit 0.30.13 `EventLoopProxy` docs.

---

## 0. Basis and conflict resolutions

**Base.** Proposals A and B tied on total score (21.5 each). C scored 18.
- The public API follows **A**, the Makie-faithful one. The user is a Makie power user, and A won on ergonomics.
- The internals follow **B**: the snapshot-then-compute concurrency, the scoped live runner, the error and semver hygiene, and the main-thread check. B won on soundness and on GPU feasibility.
- From **C**: the end-to-end first milestone, handles that carry `.save()`, `.show()`, `.figure()`, `.axis()` and `.unpack()`, a sprite pipeline shared by markers and glyphs, decorations drawn as pixel-snapped rectangles, and tuple/`..=` sugar for heatmap input.

| Conflict | Resolution |
|---|---|
| Grid index base: 1-based (A/C) or 0-based (B) | **1-based, inclusive, as in Makie.** `Span` does **not** implement half-open `Range`, so `fig.at(2, 1..2)` fails to compile with a custom `on_unimplemented` hint. `usize` is accepted alongside `i32`, which fixes the soundness complaint. Anything that indexes the user's own data (the inspector's `[i, j]`, `Field`) is 0-based. That rule is stated once, in the docs and in error messages. |
| `Data1D` as a blanket over `IntoIterator` (C, ergonomics judge) or explicit impls only (B, soundness judge) | **Explicit impls, with the common std iterator adapters included:** `Map`, `Copied`, `Cloned`, `StepBy`, `Take`, `Skip`, `Rev`, `Chain`, `slice::Iter`, `vec::IntoIter`, each bounded `where Self: Iterator, Self::Item: Scalar`. So `t.iter().map(\|t\| t.sin())` works with no `.collect()`. There is no blanket impl, so future impls stay possible. Anything else goes through `sciplot::iter(it)`. |
| Setter shape | `fn attr(&self, v) -> Self` everywhere (A/C). One name serves construction, keyword macros and live updates. No `#[must_use]`. B's `x(self)` + `set_x(&self)` split is rejected. |
| Data errors: panic (A), deferred (B), or panic (C) | Programmer errors at a call site (length or shape mismatch, unknown colormap name) **panic with `#[track_caller]`**, pointing at the user's line. Every live data setter has a `try_*` variant that returns `Result<(), DataError>`. A setter on a stale handle is a warn-once no-op, and its `try_*` returns `Err(Stale)`. Environment failures (I/O, GPU, window, thread) return `sciplot::Result`. Nothing is deferred to `save`. |
| Live runner: A's `'static` `show_live`, B's scoped `run`, or C's `show_with` | **`fig.show_live(\|live\| …)` runs on a scoped thread (B's semantics under A's name).** The closure may borrow locals. A drop guard clears `open` before the join. A worker panic becomes `Err(WorkerPanicked)`. The window stays open after the simulation returns. The pump mode (`fig.display()` + `screen.pump()`) is kept as a secondary option (A/B keep it, C cut it). |
| `batch`: A held a reentrant lock across the user closure | **`batch` is a gate, not a lock.** It raises `batch_depth` under a brief lock. Setters inside it each convert outside the lock and then lock briefly. While `batch_depth > 0` the renderer re-presents its last snapshot and does not take a new one. Batch end wakes it. There is no reentrant mutex and no `RefCell`. |
| Lock scope | One non-reentrant `parking_lot::Mutex<FigState>` per figure. Only O(1) swaps and attribute writes happen under it. Rendering snapshots the state (Arc clones) and then computes limits, ticks, text, layout and the DrawList outside the lock. No user closure ever runs under the lock (B). |
| Colormapping on CPU (C) or GPU (A/B) | **GPU**, for heatmap, scatter and lines: a value buffer plus a LUT texture, with range, scale and clip colours as uniforms. Irregular or log-axis heatmaps use a binary search over an edges buffer on the GPU. There is no per-cell mesh fallback. |
| Pipeline count | **Four:** `line`, `sprite` (markers and glyphs), `field` (heatmap and colorbar gradient), `mesh` (fills and decoration rectangles). |
| Decoration crispness | Spines, ticks and grid are **rectangles in `mesh`**. Their **positions** are snapped to device pixels. Widths are never changed, so fractional ppu such as 3.125 stays width-exact and PNG and SVG agree. In SVG they are `<rect>` elements. |
| `Aspect::Data`: layout-aware (C) or Makie centred shrink (A/B) | Makie semantics. Flush colorbars use Makie's own idiom, `layout.colsize(c, Aspect(r, k))`, shown in S4 and S5. |
| Theme | Private fields with chainable setters (A/B). C's public struct is rejected because adding a field would break semver. |
| Non-mutating one-liners | They return the **plot handle** (C). Every handle has `.figure()`, `.axis()`, `.unpack() -> (Figure, Axis, Self)`, `.save()`, `.show()`, `.with_axis(\|a\| …)` and `.with_figure(\|f\| …)`. There is no `FigureAxisPlot` type. |
| Macro meaning | A `!` macro **always** draws into an explicit target. Positional arity only picks between Makie's overloads (`lines!(ax, y)` or `lines!(ax, x, y)`). The non-mutating keyword form is `kw!(scatter(&x, &y); …)`. |
| Rich text | `rich!(…)` and `superscript()` / `subscript()`, plus the opt-in `tex("k^{-5/3}")`. Plain strings are never parsed as markup. |
| Name collisions (A) | Grid span is `Span`, the rich-text run is `TextSpan`, the rich string is `RichText`, the text plot is `TextPlot`, and the grid size is `Aspect(i, r)`. Axis aspect uses `DataAspect` / `AxisAspect(r)`. |
| Conversion traits | Only sciplot-local `Into*` traits with explicit impls (`IntoColorSpec`, `IntoPadding`, `IntoSize2`, `IntoSpan`, …). There is no blanket `From`, which avoids overlap with core's `impl<T> From<T> for T`. |
| macOS P3 oversaturation | Configure the surface with `SurfaceColorSpace::Srgb` explicitly (wgpu 30 has the variant; the docs do not say what it does on Metal). Check with Digital Color Meter in M1. The objc2 `CAMetalLayer.colorspace` shim is an **in-scope item** of the window milestone and is used if the layer's colorspace is still nil. |
| `EventLoopProxy: Sync?` | The docs.rs page is platform-generic and did not settle this, so each proxy is stored in a `parking_lot::Mutex`. It is only touched on the false→true edge of `wake_pending`, so the cost is negligible. |
| v1 scope | A/B's scope, not C's cut list. Kept: `colsize`, `rowsize`, `colgap`, `rowgap`, nested GridLayout, `hide*decorations`, `hlines`, `vlines`, `ablines`, text, tick-format closures, reversed axes, `lowclip`, `highclip`, `nan_color`, legend title, horizontal legend, `nbanks`, italic fonts. Explicit non-goals are in §8. |

---

## 1. User code for S1–S8

User `Cargo.toml` for all scenarios:

```toml
[dependencies]
sciplot   = { version = "0.1", features = ["ndarray"] }   # "window" is a default feature
ndarray = "0.17"
rand    = "0.9"
```

### S1: one-liners

```rust
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let x: Vec<f64> = (0..1000).map(|_| rand::random()).collect();
    let y: Vec<f64> = (0..1000).map(|_| rand::random()).collect();

    scatter(&x, &y).save("scatter.png")?;   // new Figure+Axis+Scatter; 600×450 units -> 1200×900 px (px_per_unit 2)
    scatter(&x, &y).show()?;                // native window; blocks until closed; pan/zoom/rect-zoom/hover built in

    // keyword form of the non-mutating call, or plain chaining:
    kw!(scatter(&x, &y); markersize = 4, color = (BLUE, 0.5)).save("scatter_small.png")?;
    scatter(&x, &y).markersize(4).with_axis(|a| a.title("uniform")).save("titled.png")?;
    let (_fig, _ax, _plot) = scatter(&x, &y).unpack();   // Makie: fig, ax, p = scatter(x, y)
    Ok(())
}
```

### S2: two labelled lines, axislegend, SVG

```rust
use sciplot::prelude::*;
use std::f64::consts::PI;

fn main() -> sciplot::Result<()> {
    let t = linspace(0.0, 4.0 * PI, 200);                       // Vec<f64>

    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1);
        title  = "Harmonic oscillator",
        xlabel = "time t (s)",
        ylabel = "displacement x (mm)");
    lines!(ax, &t, t.iter().map(|t| t.sin()); label = "sin");     // iterator adapters accepted directly
    lines!(ax, &t, t.iter().map(|t| t.cos()); label = "cos", linestyle = Linestyle::Dash);
    axislegend!(ax; position = Pos::RT);                          // :rt is also the default

    fig.save("oscillator.svg")
}
// Builder equivalent: ax.lines(&t, t.iter().map(|t| t.cos())).label("cos").linestyle(Linestyle::Dash);
```

### S3: 2×2 panels with a spanning bottom axis, linked x, shared legend, hidden inner decorations, panel labels

```rust
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let t = linspace(0.0, 10.0, 300);
    let model = |w: f64| -> Vec<f64> { t.iter().map(|t| (w * t).sin() * (-0.1 * t).exp()).collect() };
    let td: Vec<f64> = t.iter().step_by(15).copied().collect();
    let data  = |w: f64| -> Vec<f64> { td.iter().map(|t| (w * t).sin() * (-0.1 * t).exp() + 0.05 * (7.0 * t).cos()).collect() };

    let fig  = Figure!(size = (900, 650));
    let ax_a = Axis!(fig.at(1, 1);      title = "ω = 1", ylabel = "u (V)");
    let ax_b = Axis!(fig.at(1, 2);      title = "ω = 2");
    let ax_c = Axis!(fig.at(2, 1..=2);  title = "ω = 0.5", xlabel = "t (s)", ylabel = "u (V)");

    for (ax, w) in [(&ax_a, 1.0), (&ax_b, 2.0), (&ax_c, 0.5)] {
        lines!(ax, &t, model(w); label = "model");
        scatter!(ax, &td, data(w); label = "measurement", markersize = 7);
    }

    linkxaxes!(ax_a, ax_b);
    linkyaxes!(ax_a, ax_b);
    hideydecorations!(ax_b; grid = false);                       // B's y ticks/labels are redundant; keep its grid

    Legend!(fig.at(1..=2, 3), &[&ax_a, &ax_b, &ax_c]; unique = true, framevisible = false);

    for (pos, s) in [(fig.at(1, 1), "A"), (fig.at(1, 2), "B"), (fig.at(2, 1..=2), "C")] {
        Label!(pos.side(Side::TopLeft), s;
            fontsize = 20, font = Font::Bold, padding = (0, 5, 5, 0), halign = HAlign::Right);
    }

    fig.save("panels.png")?;
    fig.save("panels.svg")
}
```

### S4: heatmap from a flat Vec and from ndarray, value-coloured scatter, each with a Colorbar

```rust
use sciplot::prelude::*;
use ndarray::Array2;

fn density(x: f64, y: f64) -> f64 { 1.2 * (-(x - 1.0).powi(2) / 0.08 - (y - 0.5).powi(2) / 0.02).exp() }

fn main() -> sciplot::Result<()> {
    let (nx, ny) = (400usize, 200usize);
    let (lx, ly) = (2.0, 1.0);                                    // mm
    let xc: Vec<f64> = (0..nx).map(|i| (i as f64 + 0.5) * lx / nx as f64).collect();
    let yc: Vec<f64> = (0..ny).map(|j| (j as f64 + 0.5) * ly / ny as f64).collect();

    // (a) flat Vec<f64>, x fastest: n[j*nx + i] = n(x_i, y_j)
    let mut n = vec![0.0; nx * ny];
    for j in 0..ny { for i in 0..nx { n[j * nx + i] = density(xc[i], yc[j]); } }
    // (b) ndarray, first index is x (Makie z[i, j]), shape (nx, ny)
    let a = Array2::from_shape_fn((nx, ny), |(i, j)| density(xc[i], yc[j]));

    let fig = Figure!(size = (800, 950));

    let ax1 = Axis!(fig.at(1, 1); title = "flat Vec<f64>", ylabel = "y (mm)");
    // f64 `a..=b` = closed interval of OUTER CELL EDGES (Makie `0..1` interval)
    let hm1 = heatmap!(ax1, 0.0..=lx, 0.0..=ly, Field::new(&n, nx, ny);   // or the tuple (&n, nx, ny)
        colormap = Colormap::MAGMA, colorrange = (0.0, 1.2));
    Colorbar!(fig.at(1, 2), &hm1; label = tex("n (10^{19} m^{-3})"));

    let ax2 = Axis!(fig.at(2, 1); title = "ndarray::Array2", ylabel = "y (mm)");
    let hm2 = heatmap!(ax2, &xc, &yc, &a;                          // len n = centres, len n+1 = edges
        colormap = Colormap::MAGMA, colorrange = (0.0, 1.2));
    Colorbar!(fig.at(2, 2), &hm2; label = "n (a.u.)");

    // scatter coloured by a value vector through a colormap
    let k: Vec<f64> = (0..300).map(|k| k as f64).collect();
    let px: Vec<f64> = k.iter().map(|k| 1.0 + 0.9 * (k / 300.0).sqrt() * (k * 2.39996).cos()).collect();
    let py: Vec<f64> = k.iter().map(|k| 0.5 + 0.45 * (k / 300.0).sqrt() * (k * 2.39996).sin()).collect();
    let temp: Vec<f64> = px.iter().zip(&py).map(|(x, y)| 10.0 + 40.0 * density(*x, *y)).collect();

    let ax3 = Axis!(fig.at(3, 1); title = "probes", xlabel = "x (mm)", ylabel = "y (mm)");
    let sc = scatter!(ax3, &px, &py; color = &temp, colormap = Colormap::VIRIDIS, markersize = 10);
    Colorbar!(fig.at(3, 2), &sc; label = "T (eV)");

    fig.layout().colsize(1, Aspect(1, lx / ly));                   // layout-aware aspect: colorbars sit flush
    linkaxes!(ax1, ax2, ax3);
    hidexdecorations!(ax1; grid = false);
    hidexdecorations!(ax2; grid = false);
    fig.resize_to_layout();                                        // trim leftover whitespace (Makie resize_to_layout!)
    fig.save("fields.png")
}
```

### S5: live Gray–Scott 256² with a growing diagnostic (macOS-correct)

**Recommended form.** The simulation runs on a scoped worker thread and may borrow locals. The event loop runs on the main thread.

```rust
use sciplot::prelude::*;

struct GrayScott { n: usize, u: Vec<f64>, v: Vec<f64>, u2: Vec<f64>, v2: Vec<f64> }

impl GrayScott {
    fn new(n: usize) -> Self {
        let mut s = Self { n, u: vec![1.0; n * n], v: vec![0.0; n * n], u2: vec![0.0; n * n], v2: vec![0.0; n * n] };
        for j in n / 2 - 10..n / 2 + 10 { for i in n / 2 - 10..n / 2 + 10 { s.u[j * n + i] = 0.5; s.v[j * n + i] = 0.25; } }
        s
    }
    fn step(&mut self) {
        let (n, du, dv, f, k) = (self.n, 0.16, 0.08, 0.035, 0.065);
        let idx = |i: usize, j: usize| (j % n) * n + (i % n);
        for j in 0..n { for i in 0..n {
            let c = idx(i, j);
            let lap = |a: &[f64]| a[idx(i + 1, j)] + a[idx(i + n - 1, j)] + a[idx(i, j + 1)] + a[idx(i, j + n - 1)] - 4.0 * a[c];
            let (u, v) = (self.u[c], self.v[c]);
            let uvv = u * v * v;
            self.u2[c] = u + du * lap(&self.u) - uvv + f * (1.0 - u);
            self.v2[c] = v + dv * lap(&self.v) + uvv - (f + k) * v;
        }}
        std::mem::swap(&mut self.u, &mut self.u2);
        std::mem::swap(&mut self.v, &mut self.v2);
    }
    fn mean_v(&self) -> f64 { self.v.iter().sum::<f64>() / self.v.len() as f64 }
}

fn main() -> sciplot::Result<()> {
    const N: usize = 256;
    let mut sim = GrayScott::new(N);

    let fig = Figure!(size = (1100, 520));
    let ax = Axis!(fig.at(1, 1); title = "step 0", xlabel = "x", ylabel = "y");
    let hm = heatmap!(ax, 0.0..=1.0, 0.0..=1.0, Field::new(&sim.v, N, N);
        colormap = Colormap::MAGMA, colorrange = (0.0, 0.45));
    Colorbar!(fig.at(1, 2), &hm; label = "v");
    let ax_d = Axis!(fig.at(1, 3); title = "diagnostic", xlabel = "step", ylabel = "mean v");
    let diag = lines!(ax_d, [0.0], [sim.mean_v()]);
    ax_d.follow(true);                                   // re-autoscale on new data; paused by user pan/zoom, resumed by Ctrl-click
    fig.layout().colsize(1, Aspect(1, 1.0)).colsize(3, Relative(0.35));

    // Main thread: event loop. Closure: scoped worker thread (borrows `sim`, `hm`, `ax`, `diag`; no `move` needed).
    fig.show_live(|live| {
        let mut step = 0usize;
        while live.is_open() {
            sim.step();
            step += 1;
            if step % 20 == 0 {                          // or: `if live.frame_due()` for render-paced publishing
                live.batch(|| {                          // frame never shows the new title with the old field
                    hm.set_data(Field::new(&sim.v, N, N));   // f64->f32 + extrema on THIS thread, then an O(1) swap
                    ax.title(format!("step {step}"));
                    diag.push(step as f64, sim.mean_v());    // tail-only GPU upload
                });
            }
        }
    })
}
```

**Alternative (pump).** The simulation stays on the main thread. The setup code is unchanged.

```rust
    let screen = fig.display()?;                          // opens the window, returns immediately; main thread only; Screen: !Send
    let mut step = 0usize;
    while screen.is_open() {
        sim.step(); step += 1;
        if step % 20 == 0 {
            fig.batch(|| {
                hm.set_data(Field::new(&sim.v, N, N));
                ax.title(format!("step {step}"));
                diag.push(step as f64, sim.mean_v());
            });
        }
        screen.pump()?;          // ≤ ~120 Hz internally; handles input, hover, redraw. Caveat: stalls during macOS live-resize.
    }
    Ok(())
```

### S6: log-log with minor ticks, and semilog-y

```rust
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let k = logspace(-1.0, 3.0, 81);                                        // 0.1 .. 1e3
    let t = linspace(0.0, 10.0, 60);

    let fig = Figure!(size = (900, 400));
    let ax1 = Axis!(fig.at(1, 1);
        title = "Kolmogorov spectrum", xlabel = "k (1/m)", ylabel = "E(k)",
        xscale = Scale::Log10, yscale = Scale::Log10,
        xminorticksvisible = true, yminorticksvisible = true,
        xminorgridvisible = true, yminorgridvisible = true);
    lines!(ax1, &k, k.iter().map(|k| 2.0 * k.powf(-5.0 / 3.0)); label = tex("k^{-5/3}"));
    axislegend!(ax1; position = Pos::LB);

    let ax2 = Axis!(fig.at(1, 2);
        title = "semilog-y", xlabel = "t (s)", ylabel = "signal",
        yscale = Scale::Log10, yminorticksvisible = true);
    scatterlines!(ax2, &t, t.iter().map(|t| 1e3 * (-1.2 * t).exp() + 1e-2); markersize = 5);

    fig.save("scaling.png")?;
    fig.save("scaling.svg")
}
```

On log axes the majors fall on integer decades, labelled "10" with a superscript exponent. Minors fall at k·10ⁿ for k = 2..9 (deviations D1 and D2). For Makie's exact behaviour: `xticks = LogTicks::makie(), xminorticks = IntervalsBetween(2)`.

### S7: density histogram with pdf overlay, categorical barplot, confidence band

```rust
use sciplot::prelude::*;
use std::f64::consts::PI;

fn randn() -> f64 {                                         // Box–Muller
    let (u1, u2): (f64, f64) = (rand::random(), rand::random());
    (-2.0 * (1.0 - u1).ln()).sqrt() * (2.0 * PI * u2).cos()
}

fn main() -> sciplot::Result<()> {
    let samples: Vec<f64> = (0..10_000).map(|_| randn()).collect();
    let xs = linspace(-4.0, 4.0, 200);
    let fig = Figure!(size = (1200, 400));

    let ax1 = Axis!(fig.at(1, 1); title = "histogram", xlabel = "x", ylabel = "probability density");
    hist!(ax1, &samples; bins = 40, normalization = Normalization::Pdf, label = "samples");
    lines!(ax1, &xs, xs.iter().map(|x| (-x * x / 2.0).exp() / (2.0 * PI).sqrt());
        color = BLACK, linewidth = 2, label = "N(0, 1)");
    axislegend!(ax1);

    let ax2 = Axis!(fig.at(1, 2); title = "solver runtime", ylabel = "time (s)");
    barplot!(ax2, ["CG", "GMRES", "BiCGStab", "Jacobi"], [1.2, 2.1, 1.6, 7.9]);  // x ticks = names, order of appearance

    let t = linspace(0.0, 10.0, 100);
    let mean: Vec<f64> = t.iter().map(|t| (0.6 * t).sin() * (-0.1 * t).exp()).collect();
    let sd:   Vec<f64> = t.iter().map(|t| 0.05 + 0.02 * t).collect();
    let lo: Vec<f64> = mean.iter().zip(&sd).map(|(m, s)| m - 1.96 * s).collect();
    let hi: Vec<f64> = mean.iter().zip(&sd).map(|(m, s)| m + 1.96 * s).collect();

    let ax3 = Axis!(fig.at(1, 3); title = "ensemble mean ± 95% CI", xlabel = "t (s)", ylabel = "u");
    band!(ax3, &t, &lo, &hi; color = (WONG[0], 0.3), label = "95% CI");
    lines!(ax3, &t, &mean; color = WONG[0], label = "mean");
    axislegend!(ax3; position = Pos::RB);

    fig.save("stats.png")
}
```

### S8: paper figure with a scoped theme, 4 in × 3 in, 12 pt, 300-dpi PNG and physically sized SVG

```rust
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let t = linspace(0.0, 5.0, 200);
    let n: Vec<f64> = t.iter().map(|t| 1e19 * (1.0 + 0.5 * (-t).exp() * (6.0 * t).cos())).collect();

    // 1 unit = 1 CSS px = 1/96 in = 0.75 pt.  PT = 96/72 units, INCH = 96 units.
    let paper = theme_minimal()
        .fontsize(12.0 * PT)                                  // 16 units -> 12 pt in the SVG
        .figure_padding(4.0 * PT)
        .linewidth(1.0 * PT)
        .axis(|a| a.xticksvisible(true).yticksvisible(true)
                   .spinewidth(0.75 * PT).xtickwidth(0.75 * PT).ytickwidth(0.75 * PT)
                   .xticksize(3.0 * PT).yticksize(3.0 * PT));

    with_theme(paper, || -> sciplot::Result<()> {               // thread-scoped; restored on exit or panic
        let fig = Figure!(size = (4.0 * INCH, 3.0 * INCH));   // 384 × 288 units
        let ax = Axis!(fig.at(1, 1); xlabel = "time t (ms)", ylabel = tex("n (m^{-3})"));
        lines!(ax, &t, &n);
        fig.save_with("fig.png", Save::dpi(300))?;            // px_per_unit = 300/96 = 3.125 -> 1200×900 px, pHYs 11811 px/m
        fig.save("fig.svg")                                   // width="288pt" height="216pt" viewBox="0 0 384 288"
    })
}
```

---

## 2. Public API reference

### 2.1 Ownership and threading model

```rust
pub struct Figure { sh: Arc<FigShared> }                    // Clone + Send + Sync + 'static
pub struct Axis   { sh: Arc<FigShared>, id: BlockId }       // same shape for every block and plot handle; PartialEq by id
pub struct Lines  { sh: Arc<FigShared>, id: PlotId }

struct FigShared {
    state: parking_lot::Mutex<FigState>,        // the ONLY lock; non-reentrant; never held across user code
    wake: Waker,                                // AtomicBool pending + Mutex<Vec<(ScreenId, EventLoopProxy<UserEvent>)>>
    published_rev: AtomicU64,                   // bumped by every setter
    rendered_rev: AtomicU64,                    // set by the renderer after present (frame_due / wait_frame)
}
struct FigState { arena: Arena /* (index, generation) ids */, theme: Theme, root: LayoutId, batch_depth: u32, /* revs, dirty */ }
```

**Rules**
1. **One method serves construction and update.** Every handle setter is `fn x(&self, v) -> Self`, which returns an Arc clone. The sequence is: convert the input *off-lock*, lock, write the attribute, bump the revision, OR in a dirty class (`DATA | STYLE | LIMITS | LAYOUT`), unlock, then `notify()` unless a batch is open.
2. **`notify()`** fires only on the false→true edge of `wake.pending`. At that edge it calls `proxy.send_event(Wake(fig_id))` for each attached screen. The proxies live in their own `Mutex`, which is never taken while `state` is held.
3. **Bulk payloads.**
   - `set_data`, `push` and `extend` convert f64→f32 into a *pooled* buffer (returned by the renderer after upload) and compute NaN-aware extrema on the caller's thread.
   - Under the lock they only swap in an `Arc<[f32]>` (or append for `push`).
4. **`batch(f)`.**
   - Opening: lock, `batch_depth += 1`, unlock.
   - Then `f()` runs, with setters behaving normally.
   - Closing (a drop guard, so it also runs on panic): lock, `batch_depth -= 1`, unlock, `notify()`.
   - The renderer checks `batch_depth` **under the lock at snapshot time**. If it is above 0, it re-presents the previous snapshot, so interaction still works, and waits for the batch-end wake.
   - The result: no torn frames, and the lock is never held during conversion.
5. **Rendering.**
   - Clear `wake.pending`, then lock and snapshot in O(#plots) (attribute struct clones and `Arc` clones), then unlock.
   - Everything else — limits, ticks, tick-format closures, text measure, layout, DrawList, uploads — runs outside the lock.
   - Autolimits results are written back with a compare-and-set on each axis's `limits_rev`.
6. **Stale handles.** Setters on a deleted handle warn once and do nothing. `try_*` returns `Err(DataError { kind: Stale, .. })`.
7. **Compile-time checks.**

   ```rust
   const _: () = { const fn ok<T: Send + Sync + Clone + 'static>() {}
       ok::<Figure>(); ok::<Axis>(); ok::<Heatmap>(); ok::<Lines>(); ok::<Live>(); /* every handle */ };
   ```

   `Screen` is `!Send + !Sync` via `PhantomData<*const ()>`, and a `compile_fail` doctest proves it.
8. **Unsafe code.** `#![deny(unsafe_code)]` applies everywhere except `window/macos.rs` (the colorspace shim) and `window/mainthread.rs` (`pthread_main_np`).

### 2.2 Figure, positions, layout

```rust
impl Figure {
    pub fn new() -> Figure;                                    // snapshots current_theme(); 600×450 units
    pub fn size(&self, wh: impl IntoSize2) -> Self;            // (900, 650), (4.0*INCH, 3.0*INCH)
    pub fn fontsize(&self, s: impl Num) -> Self;
    pub fn backgroundcolor(&self, c: impl IntoColor) -> Self;
    pub fn figure_padding(&self, p: impl IntoPadding) -> Self; // Num or (l, r, b, t)
    pub fn theme(&self, t: Theme) -> Self;                     // restyle an existing figure (attributes resolve at snapshot)
    pub fn datainspector(&self, on: bool) -> Self;             // hover readout in windows; default on
    pub fn window_title(&self, s: impl Into<String>) -> Self;
    pub fn at(&self, row: impl IntoSpan, col: impl IntoSpan) -> GridPosition;   // f[r, c]
    pub fn layout(&self) -> GridLayout;                        // f.layout
    pub fn content(&self, row: impl IntoSpan, col: impl IntoSpan) -> Option<Block>;
    pub fn axes(&self) -> Vec<Axis>;
    pub fn resize_to_layout(&self) -> Self;
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R;
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()>;         // .png | .svg by extension
    pub fn save_with(&self, path: impl AsRef<Path>, o: Save) -> Result<()>;
    pub fn to_svg_string(&self, o: &Save) -> Result<String>;
    pub fn render_rgba(&self, o: &Save) -> Result<RgbaImage>;        // any thread (movie frames from a sim)
    pub fn debug_layout(&self) -> LayoutDump;                        // #[doc(hidden)] for golden tests
    #[cfg(feature = "window")] pub fn show(&self) -> Result<()>;     // blocking; main thread
    #[cfg(feature = "window")] pub fn display(&self) -> Result<Screen>;
    #[cfg(feature = "window")]
    pub fn show_live<R, F>(&self, sim: F) -> Result<R> where F: FnOnce(&Live) -> R + Send, R: Send;
}
#[cfg(feature = "window")] pub fn show_all(figs: &[&Figure]) -> Result<()>;

#[diagnostic::on_unimplemented(message = "grid positions are 1-based and inclusive like Makie: use `2`, `1..=2`, `..`, or `2..`; `1..2` is not accepted")]
pub trait IntoSpan { fn into_span(self) -> Span; }
// impls: i32, usize, RangeInclusive<i32>, RangeInclusive<usize>, RangeFull, RangeFrom<i32>, RangeFrom<usize>
// (integer literals fall back to i32). 0 or negative prepends a row/col (f[0, :]); the grid grows automatically.
pub enum Side { Inner, Left, Right, Top, Bottom, TopLeft, TopRight, BottomLeft, BottomRight, Outer }

impl GridPosition {                                            // Clone
    pub fn side(&self, s: Side) -> Self;                       // f[1, 1, TopLeft()]
    pub fn layout(&self) -> GridLayout;                        // f[1, 2] = GridLayout() (created lazily, nested)
    pub fn at(&self, r: impl IntoSpan, c: impl IntoSpan) -> GridPosition;   // f[1, 2][1, 1]
    pub fn content(&self) -> Option<Block>;
}
impl GridLayout {
    pub fn at(&self, r: impl IntoSpan, c: impl IntoSpan) -> GridPosition;
    pub fn colsize(&self, col: i32, s: impl IntoGridSize) -> Self;          // Auto | Fixed(px) | Relative(f) | Aspect(i, r)
    pub fn rowsize(&self, row: i32, s: impl IntoGridSize) -> Self;
    pub fn colgap(&self, g: impl IntoGap) -> Self;  pub fn colgap_at(&self, i: i32, g: impl IntoGap) -> Self;
    pub fn rowgap(&self, g: impl IntoGap) -> Self;  pub fn rowgap_at(&self, i: i32, g: impl IntoGap) -> Self;
    pub fn alignmode(&self, m: AlignMode) -> Self;  // Inside | Outside(pad) | Mixed { left, right, bottom, top: Option<f64> }
    pub fn halign(&self, a: HAlign) -> Self;  pub fn valign(&self, a: VAlign) -> Self;
    pub fn tellwidth(&self, b: bool) -> Self; pub fn tellheight(&self, b: bool) -> Self;
    pub fn trim(&self) -> Self;  pub fn nrows(&self) -> i32;  pub fn ncols(&self) -> i32;
}
pub struct Auto; pub struct Fixed(pub f64); pub struct Relative(pub f64); pub struct Aspect(pub i32, pub f64);
// IntoGridSize: Auto, Fixed, Relative, Aspect, and bare Num (= Fixed). IntoGap: Num (= Fixed), Relative.
```

### 2.3 Axis

Setters are generated from one attribute table that mirrors `makielayout/types.jl:305-760`: Makie names, Makie defaults, and a dirty class per attribute. The same table generates `AxisTheme`.

```rust
impl Axis {
    pub fn new(pos: GridPosition) -> Axis;
    // text: title, subtitle, xlabel, ylabel (impl Into<RichText>); titlesize, titlefont(Font), titlegap, titlealign(HAlign),
    //       titlecolor, titlevisible, subtitlegap, xlabelsize, xlabelpadding, ylabelpadding, xlabelvisible, ylabelvisible, ...
    // ticks: xticks(impl IntoTicks) // Automatic | &[f64] | Vec<f64> | (vals, labels) | WilkinsonTicks(k) | LinearTicks(n)
    //        | MultiplesTicks(n, PI, "π") | LogTicks::integer() | LogTicks::makie() | Ticks::func(|lo, hi| ..)
    //        xtickformat(impl IntoTickFormat) // |v: f64| String | |vs: &[f64]| Vec<String> | "{:.2}"
    //        xminorticks(impl IntoMinorTicks) // Auto | IntervalsBetween(n) | LogMinor | Vec<f64>
    //        x/y: ticksvisible, ticksize, tickwidth, tickalign, tickcolor, ticklabelsvisible, ticklabelsize,
    //        ticklabelrotation, ticklabelpad, ticklabelcolor, minorticksvisible, minorticksize, minortickwidth, ticksmirrored
    // grid/spines: x/ygridvisible, gridcolor, gridwidth, gridstyle, minorgridvisible, minorgridcolor, minorgridwidth,
    //        spinewidth, left/right/bottom/topspinevisible, *spinecolor, backgroundcolor
    // scale/limits: xscale(Scale), yscale, limits(impl IntoLimits), x/yautolimitmargin((f64, f64)),
    //        x/yreversed(bool), aspect(impl IntoAxisAspect /* DataAspect | AxisAspect(r) | None */), autolimitaspect(Option<f64>),
    //        xaxisposition(Bottom|Top), yaxisposition(Left|Right), flip_ylabel
    // interaction: x/ypanlock, x/yzoomlock, x/yrectzoom
    // block-common: width, height, tellwidth, tellheight, halign, valign, alignmode
    pub fn xlims(&self, lo: impl LimVal, hi: impl LimVal) -> Self;   // LimVal: Num | Option<f64>; lo > hi => reversed
    pub fn ylims(&self, lo: impl LimVal, hi: impl LimVal) -> Self;
    pub fn set_limits(&self, x1: f64, x2: f64, y1: f64, y2: f64) -> Self;
    pub fn autolimits(&self) -> Self;  pub fn reset_limits(&self) -> Self;  pub fn tightlimits(&self) -> Self;
    pub fn follow(&self, on: bool) -> Self;                          // sciplot extension (D5)
    pub fn finallimits(&self) -> Rect64;
    pub fn hidexdecorations(&self, o: HideDecorations) -> Self;  hideydecorations, hidedecorations
    pub fn hidespines(&self, which: &[Spine]) -> Self;               // Spine::{Left, Right, Bottom, Top}
    pub fn axislegend(&self) -> Legend;
    // mutating plot constructors (what the ! macros call)
    pub fn lines(&self, x: impl Data1D, y: impl Data1D) -> Lines;
    pub fn lines_points(&self, p: impl PointData) -> Lines;          // y-only (x = 1..=n) or &[[T; 2]] / &[(T, T)]
    pub fn scatter(..) -> Scatter;  scatter_points(..);  scatterlines(..) -> ScatterLines;  scatterlines_points(..)
    pub fn heatmap(&self, z: impl Data2D) -> Heatmap;                // centres 1..=nx, 1..=ny
    pub fn heatmap_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Heatmap;
    pub fn hist(&self, v: impl Data1D) -> Hist;
    pub fn barplot(&self, x: impl BarX, h: impl Data1D) -> BarPlot;  barplot_heights(&self, h: impl Data1D) -> BarPlot;
    pub fn band(&self, x: impl Data1D, lo: impl Data1D, hi: impl Data1D) -> Band;
    pub fn hlines(&self, y: impl Data1D) -> HLines;  vlines(x) -> VLines;
    pub fn ablines(&self, intercept: impl Data1D, slope: impl Data1D) -> ABLines;
    pub fn text(&self, pos: impl PointData, s: impl IntoTexts) -> TextPlot;
    pub fn figure(&self) -> Figure;  pub fn plots(&self) -> Vec<AnyPlot>;
}
pub fn linkxaxes(axes: &[&Axis]); pub fn linkyaxes(axes: &[&Axis]); pub fn linkaxes(axes: &[&Axis]);
pub fn axislegend(ax: &Axis) -> Legend;
#[derive(Clone, Copy)] pub struct HideDecorations { /* label, ticklabels, ticks, grid, minorgrid, minorticks: all true = hide */ }
impl HideDecorations { pub const fn new() -> Self; pub const fn label(self, b: bool) -> Self; /* one per field */ }
pub struct DataAspect; pub struct AxisAspect(pub f64);
pub enum Scale { Identity, Log10, Log2, Ln, Sqrt }
```

### 2.4 Plot handles

**Common inherent methods on every plot type.** These are generated by a macro and are *inherent*, so no trait import is needed:

- `label(impl Into<RichText>)`, `visible(bool)`, `alpha(f32)`, `inspectable(bool)`
- `inspector_label(impl Fn(&HoverInfo) -> String + Send + Sync + 'static)`, which runs on the snapshot, never under the lock
- `xautolimits(bool)`, `yautolimits(bool)`, `z(f32)` (Makie `translate!` z), `rasterize(bool)` (SVG only)
- `delete(&self)`, `any() -> AnyPlot`, `figure() -> Figure`, `axis() -> Axis`, `unpack() -> (Figure, Axis, Self)`
- `with_axis<R>(&self, f: impl FnOnce(Axis) -> R) -> Self`, `with_figure<R>(..) -> Self`
- `save(path) -> Result<()>`, `save_with(path, Save)`, `show() -> Result<()>`

**Colormap mixin** on `ColorMapped` types (`Lines`, `Scatter`, `ScatterLines`, `Heatmap`, `Band`): `colormap(impl IntoColormap)`, `colorrange((f64, f64))`, `colorscale(Scale)`, `lowclip(impl IntoColor)`, `highclip`, `nan_color`.

| Handle | Attributes (Makie names, one argument each) | Data methods (each also has a `try_*` returning `Result<(), DataError>`) |
|---|---|---|
| `Lines` | `color(impl IntoColorSpec)`, `linewidth`, `linestyle(Linestyle)`, `linecap(LineCap)`, `joinstyle(JoinStyle)`, `miter_limit`, colormap mixin | `set_data(x, y)`, `set_points(p)`, `set_y(y)`, `push(x: f64, y: f64)`, `extend(xs, ys)`, `clear()`, `len()` |
| `Scatter` | `color`, `marker(Marker)`, `markersize(impl IntoSizes)` (scalar or per-point), `strokecolor`, `strokewidth`, `rotation(impl IntoAngles)`, `markerspace(Space)`, colormap mixin | same as `Lines` |
| `ScatterLines` | Lines attributes, plus `marker`, `markersize`, `markercolor`, `markercolormap`, `markercolorrange`, `strokecolor`, `strokewidth` | same |
| `Heatmap` | `interpolate(bool)`, colormap mixin | `set_data(z: impl Data2D)` (same dims recycles the buffer), `set_coords(x, y)`, `resolved_colorrange()` |
| `Hist` | `bins(impl IntoBins)` (usize or edges), `normalization(Normalization)`, `weights(impl Data1D)`, `scale_to(f64)`, `gap`, `offset`, `fillto`, `direction(Direction)`, `color`, `strokewidth`, `strokecolor` | `set_data(v)`, `edges()`, `heights()` |
| `BarPlot` | `gap` (0.2), `width`, `dodge(impl Data1D)`, `dodge_gap`, `stack(impl Data1D)`, `fillto`, `offset`, `direction`, `color`, `strokewidth`, `strokecolor` | `set_data(x, h)`, `set_heights(h)` |
| `Band` | `color`, `direction`, `strokewidth`, `strokecolor`, colormap mixin | `set_data(x, lo, hi)` |
| `HLines`/`VLines`/`ABLines` | `color`, `linewidth`, `linestyle`, `xmin`/`xmax` (`ymin`/`ymax`) as axis fractions | `set_data(..)` |
| `TextPlot` | `fontsize`, `font`, `color`, `align((HAlign, VAlign))`, `rotation`, `offset`, `space(Space)` | `set_data(pos, texts)` |

**Colour rule.** A colour, a `Cycled(i)` or a `(colour, alpha)` gives a solid colour. `&Vec<f64>` or `&[T]` of `Scalar` gives values mapped through the colormap. `&[Color]` gives explicit per-point colours.

**Non-mutating free functions.** `lines`, `lines_points`, `scatter`, `scatter_points`, `scatterlines`, `heatmap`, `heatmap_xy`, `hist`, `barplot`, `band`. Each creates a new Figure, an Axis at `(1, 1)` and the plot, and returns the plot handle.

**Enums**

```rust
pub enum Marker { Circle, Rect, Diamond, UTriangle, DTriangle, LTriangle, RTriangle, Cross, XCross, Pentagon, Hexagon, Star5,
                  FullCircle /* Makie Circle type */, FullRect /* Makie Rect type */ }
pub enum Linestyle { Solid, Dash, Dot, DashDot, DashDotDot, Custom(Vec<f32>) }   // .dense() / .loose() -> Custom (Makie gaps)
pub enum Normalization { None, Pdf, Density, Probability }
pub enum Direction { X, Y }
pub enum LineCap { Butt, Square, Round }  pub enum JoinStyle { Miter, Bevel, Round }
```

### 2.5 Colorbar, Legend, Label

```rust
impl Colorbar {
    pub fn new(pos: GridPosition, plot: &impl ColorMapped) -> Colorbar;      // links to the plot's colour mapping at every snapshot
    pub fn from_colormap(pos: GridPosition, cmap: impl IntoColormap, limits: (f64, f64)) -> Colorbar;
    // label, labelsize, labelpadding, vertical, flipaxis, size (12), ticks, tickformat, minorticksvisible, minorticks,
    // ticklabelsize, ticklabelpad, ticksize, spinewidth, nsteps, width, height, tellwidth, tellheight, halign, valign
}
// A plot-backed colorbar that is given colormap/limits/clip setters: warn once and ignore (Makie errors; sciplot never panics on live paths).
// A solid-coloured plot: draws that plot's colormap over its colorrange (default (0,1)) and warns once. No creation-order panic.

impl Legend {
    pub fn new(pos: GridPosition, src: &(impl LegendSource + ?Sized)) -> Legend;   // LegendSource: Axis, [&Axis], [&Axis; N]
    pub fn from_entries(pos: GridPosition, entries: &[(AnyPlot, RichText)]) -> Legend;
    pub fn from_groups(pos: GridPosition, groups: &[(RichText, Vec<(AnyPlot, RichText)>)]) -> Legend;
    // title, merge, unique, orientation(Orientation::{Vertical, Horizontal}), nbanks, framevisible, framecolor, framewidth,
    // backgroundcolor, padding, margin, patchsize, rowgap, colgap, patchlabelgap, labelsize, titlesize, titlegap,
    // position(Pos) [axislegend], halign, valign, tellwidth, tellheight
}   // entries are collected at every snapshot: plots labelled later appear; an empty source draws nothing (warn once)
pub enum Pos { LT, CT, RT, LC, CC, RC, LB, CB, RB, Frac(f64, f64) }

impl Label { pub fn new(pos: GridPosition, text: impl Into<RichText>) -> Label;
    // text (update), fontsize, font, color, rotation, padding, halign, valign, justification, lineheight, tellwidth, tellheight
}
pub enum HAlign { Left, Center, Right, Frac(f64) }   pub enum VAlign { Bottom, Center, Top, Frac(f64) }
```

### 2.6 Text, fonts, units, save

```rust
pub struct RichText { spans: Vec<TextSpan> }       // From<&str>, From<String>; plain strings are never parsed
pub struct TextSpan { /* text, font, color, size_scale, baseline_shift, x_offset */ }   // From<&str>, From<String>
pub fn superscript(s: impl Into<String>) -> TextSpan;  pub fn subscript(..) -> TextSpan;
pub fn tex(s: &str) -> RichText;   // opt-in: ^{..}, _{..}, \alpha..\omega; '-' inside ^{} -> U+2212; \^ \_ escape
pub enum Font { Regular, Bold, Italic, BoldItalic, Custom(FontId) }
impl FontId { pub fn from_bytes(b: &'static [u8]) -> Result<FontId>; pub fn load(p: impl AsRef<Path>) -> Result<FontId>; }

pub const PX: f64 = 1.0; pub const PT: f64 = 96.0 / 72.0; pub const INCH: f64 = 96.0;
pub const CM: f64 = 96.0 / 2.54; pub const MM: f64 = CM / 10.0;

#[derive(Clone)] pub struct Save { /* px_per_unit = 2.0, pt_per_unit = 0.75, transparent = false, svg_text = Outlines */ }
impl Save { pub fn new() -> Self; pub fn dpi(d: impl Num) -> Self;          // px_per_unit = d / 96
    pub fn px_per_unit(self, f: impl Num) -> Self; pub fn pt_per_unit(self, f: impl Num) -> Self;
    pub fn transparent(self, b: bool) -> Self; pub fn backgroundcolor(self, c: impl IntoColor) -> Self;
    pub fn svg_text(self, t: SvgText /* Outlines | Text */) -> Self; }
pub struct RgbaImage { pub width: u32, pub height: u32, pub data: Vec<u8> }
```

### 2.7 Colour, palette, colormap

```rust
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }        // sRGB-encoded, straight alpha
impl Color { pub const fn rgb(..) -> Self; pub const fn rgba(..) -> Self; pub const fn rgb8(u8, u8, u8) -> Self;
    pub const fn hex(v: u32) -> Self;            // 0xRRGGBB (const, cannot fail)
    pub fn parse(s: &str) -> Result<Self>;       // "#0072B2", "#0072B280", CSS / Colors.jl names, "gray50"
    pub const fn with_alpha(self, a: f32) -> Self; pub const TRANSPARENT: Self; }
pub mod colors { /* BLACK, WHITE, RED, GREEN, BLUE, ORANGE, PURPLE, GRAY, … all CSS names */ }
pub const WONG: [Color; 7];  pub const WONG_PATCH: [Color; 7];  pub fn wong_colors() -> [Color; 7];
pub struct Cycled(pub usize);                    // 1-based, as in Makie (it indexes a palette, not user data)
pub trait IntoColor { }      // Color, &str (panics #[track_caller] on an invalid literal), (Color, f32), (&str, f32)
pub trait IntoColorSpec { }  // IntoColor types + Cycled + (Cycled, f32) + values: &[T], &Vec<T>, Vec<T> (T: Scalar) + &[Color], Vec<Color>
                             // arrays are deliberately NOT impl'd (so [f32; 3] is never ambiguous)
pub struct Colormap { /* Cow<'static, [[f32; 4]]>, categorical, reversed */ }
impl Colormap { pub const VIRIDIS, MAGMA, INFERNO, PLASMA, CIVIDIS, TURBO, GRAYS, BLUES, REDS, BALANCE, RDBU, COOLWARM: Colormap;
    pub fn from_colors(cs: &[Color]) -> Self; pub fn categorical(cs: &[Color]) -> Self;
    pub fn reversed(self) -> Self; pub fn with_alpha(self, a: f32) -> Self; pub fn sample(&self, t: f64) -> Color; }
pub trait IntoColormap { }   // Colormap, &str ("magma"; unknown -> #[track_caller] panic listing close matches)
pub struct Palette { /* color, patchcolor, marker, linestyle */ }   // chainable setters
```

**Cycling (Makie rules).** There is a counter per axis and per plot function. At the first snapshot, plots are resolved in insertion order, and a plot advances its counter only if its cycled attribute is `Auto`. Once resolved, the index is **frozen**, so setting a colour later never shifts other plots. Lines, scatter, scatterlines and h/v/ablines use `palette.color` (Wong). Hist, bar and band use `palette.patchcolor` (0.2·bg + 0.8·c).

### 2.8 Theme and scoping

```rust
#[derive(Clone, Default)] pub struct Theme { /* private: Option-override for every global, block and plot key */ }
impl Theme {
    pub fn new() -> Self;                 // empty overrides
    pub fn makie() -> Self;               // the full default table (§4)
    // global: fontsize, font, fonts(FontSet), textcolor, backgroundcolor, colormap, palette, figure_padding, rowgap, colgap,
    //         size, linewidth, linecolor, linestyle, markersize, marker, markercolor, patchcolor, patchstrokecolor, ...
    pub fn axis(self, f: impl FnOnce(AxisTheme) -> AxisTheme) -> Self;       // AxisTheme = same attribute table as Axis
    pub fn legend(..) -> Self; pub fn colorbar(..) -> Self; pub fn label(..) -> Self;
    pub fn lines(self, f: impl FnOnce(LinesTheme) -> LinesTheme) -> Self;    // + scatter, heatmap, hist, barplot, band
    pub fn merge(self, other: Theme) -> Self;                                // left wins (Makie merge)
}
pub fn with_theme<R>(t: Theme, f: impl FnOnce() -> R) -> R;   // thread-local stack; RAII restore on panic
pub fn set_theme(t: Theme); pub fn update_theme(t: Theme); pub fn reset_theme(); pub fn current_theme() -> Theme;
pub fn theme_minimal() -> Theme; pub fn theme_light() -> Theme; pub fn theme_dark() -> Theme; pub fn theme_black() -> Theme;
```

- Resolution order: explicit attribute, then the figure's theme (captured at `Figure::new`, replaceable with `fig.theme(..)`), then the default table.
- Attributes are stored as `Attr<T> = Inherit | Set(T)`.
- `@inherit fontsize` keys resolve through the theme.

### 2.9 Data-input traits (exact, coherence-safe impl list)

```rust
mod sealed { pub trait Sealed {} }
pub trait Scalar: Copy + Send + Sync + 'static + sealed::Sealed { fn to_f64(self) -> f64; }
// impls: f64 f32 i8 i16 i32 i64 isize u8 u16 u32 u64 usize, and &'a T for T: Scalar (so slice iterators work).
// (i64/u64 > 2^53 are lossy; documented.)
pub trait Num: Scalar {}   // f64 f32 i32 i64 u32 usize — for setters, so `linewidth = 2` works (literals fall back to i32)

#[diagnostic::on_unimplemented(message = "`{Self}` is not plottable data; pass a slice/Vec/array/range/iterator-adapter, or wrap any iterator in `sciplot::iter(..)`")]
pub trait Data1D { fn write_f64(self, out: &mut Vec<f64>); fn len_hint(&self) -> Option<usize> { None } }
```

**`Data1D`: every impl is explicit and generated by one macro.**
- `&[T]`, `&Vec<T>`, `Vec<T>`, `[T; N]`, `&[T; N]` for `T: Scalar`.
- `Range<I>` and `RangeInclusive<I>` for `I` in {i32, i64, u32, usize}. Integer ranges are data; there is no f64 range as Data1D.
- `Iter<I>` from `sciplot::iter(it)`, where `I: Iterator, I::Item: Scalar`.
- These std adapters, each with `where Self: Iterator, <Self as Iterator>::Item: Scalar`: `Map<I, F>`, `Copied<I>`, `Cloned<I>`, `StepBy<I>`, `Take<I>`, `Skip<I>`, `Rev<I>`, `Chain<A, B>`, `Zip`-free, `slice::Iter<'a, T>`, `vec::IntoIter<T>`.
- Feature `ndarray`: `&ArrayBase<S, Ix1>` where `S: Data<Elem = T>, T: Scalar`, and `Array1<T>`.
- Feature `nalgebra`: `&DVector<T>`.
- **Not implemented:** bare scalars, tuples, a blanket over `IntoIterator`, or `AsRef`.

**`PointData`** (for `lines!(ax, y)` and point lists):
- every `Data1D` type above (y-only, x = 1..=n);
- `&[[T; 2]]`, `&Vec<[T; 2]>`, `&[(T, T)]`, `&Vec<(T, T)>`.

This is coherent because `[T; 2]: Scalar` and `(T, T): Scalar` are knowably false: `Scalar` is local and sealed, and the orphan rules stop downstream crates from adding them.

**`Data2D`.** The first index is always x (Makie `z[i, j]`).

```rust
pub trait Data2D { fn dims(&self) -> (usize, usize); fn write_f32(&self, out: &mut Vec<f32>) -> Strides; }
pub struct Field<'a, T: Scalar> { .. }
impl<'a, T: Scalar> Field<'a, T> {
    #[track_caller] pub fn new(data: &'a [T], nx: usize, ny: usize) -> Self;        // data[j*nx + i]; panics if len != nx*ny
    #[track_caller] pub fn y_fastest(data: &'a [T], nx: usize, ny: usize) -> Self;  // data[i*ny + j]
    pub fn try_new(..) -> Result<Self, DataError>;
}
```

`Data2D` impls:
- `Field<'_, T>`;
- `(&[T], usize, usize)` and `(&Vec<T>, usize, usize)`, which are x-fastest sugar for `Field::new` and explicitly typed, so there is no unconstrained parameter;
- `&Vec<Vec<T>>` and `&[Vec<T>]` as `v[ix][iy]` (ragged input panics with `#[track_caller]`);
- `&[[T; NY]; NX]`;
- ndarray `&ArrayBase<S, Ix2>` where `S: Data<Elem = T>, T: Scalar`, as `a[[ix, iy]]`. Contiguous arrays in either order are copied in memory order and their (sx, sy) strides go to the shader, so there is never a transpose. Other views are gathered.
- nalgebra `&DMatrix<T>` as `m[(ix, iy)]` (column-major, which is already x-fastest).

**`CellCoords`**
- `RangeInclusive<f64>`: a closed interval of **outer edges** (Makie `a..b` interval).
- `RangeInclusive<i32>` / `RangeInclusive<usize>`: centres, like Makie's `1:10`.
- `&[T]`, `&Vec<T>`, `Vec<T>`: centres (length n) or edges (length n+1), and anything else panics with `#[track_caller]`.
- `RangeFull` (`..`): centres 1..=n.
- The half-open f64 `Range` is **not** implemented; its `on_unimplemented` message says "use `a..=b`".

**`BarX`**
- The same explicit numeric container list as `Data1D`.
- `[&str; N]`, `&[&str; N]`, `&[&str]`, `Vec<&str>`, `&[String]`, `Vec<String>`: categorical, mapped to 1..=n in order of appearance, with tick labels set to the names.
- There is no blanket `impl<D: Data1D> BarX for D`; each impl is separate.

**Helpers:** `linspace(a, b, n) -> Vec<f64>`, `logspace(exp_a, exp_b, n) -> Vec<f64>`, `iter(it) -> Iter<I>`.

**Conversion traits** (`IntoSize2`, `IntoPadding`, `IntoLimits`, `IntoTicks`, `IntoBins`, `IntoSizes`, …) are all sciplot-local with explicit impls. Tuple impls use generic `Num` components, so `(900, 650)` and `(0, 5, 5, 0)` infer through integer fallback.

### 2.10 Macros (`macro_rules!`, each written by hand — none generated by another macro)

```rust
#[doc(hidden)] #[macro_export]
macro_rules! __kw { ($base:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $base $($(.$k($v))*)? }; }

/// Makie keywords on any expression with chainable setters: kw!(scatter(&x, &y); markersize = 4)
#[macro_export]
macro_rules! kw { ($base:expr; $($k:ident = $v:expr),* $(,)?) => { $base $(.$k($v))* }; }

/// lines!(target, x, y; kw…) | lines!(target, y_or_points; kw…)   — always mutating; target = Axis | &Axis | GridPosition
#[macro_export]
macro_rules! lines {
    ($t:expr, $x:expr, $y:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__target(&$t).lines($x, $y) $(; $($k = $v),*)?)
    };
    ($t:expr, $p:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $crate::__kw!($crate::__target(&$t).lines_points($p) $(; $($k = $v),*)?)
    };
}
// scatter!, scatterlines!: identical template (methods scatter/scatter_points, scatterlines/scatterlines_points)
// heatmap!: (t, z) -> .heatmap(z);  (t, x, y, z) -> .heatmap_xy(x, y, z)
// barplot!: (t, h) -> .barplot_heights(h);  (t, x, h) -> .barplot(x, h)
// hist!(t, v; ..), band!(t, x, lo, hi; ..), hlines!(t, y; ..), vlines!(t, x; ..), ablines!(t, a, b; ..), text!(t, pos, s; ..)

#[macro_export]
macro_rules! Figure { ($($k:ident = $v:expr),* $(,)?) => { $crate::Figure::new() $(.$k($v))* }; }
#[macro_export]
macro_rules! Axis {
    ($pos:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $crate::__kw!($crate::Axis::new($pos) $(; $($k = $v),*)?) };
}
#[macro_export]
macro_rules! Colorbar {
    ($pos:expr, $plot:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $crate::__kw!($crate::Colorbar::new($pos, $plot) $(; $($k = $v),*)?) };
}
#[macro_export]
macro_rules! Legend {
    ($pos:expr, $src:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $crate::__kw!($crate::Legend::new($pos, $src) $(; $($k = $v),*)?) };
}
#[macro_export]
macro_rules! Label {
    ($pos:expr, $text:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $crate::__kw!($crate::Label::new($pos, $text) $(; $($k = $v),*)?) };
}
#[macro_export]
macro_rules! axislegend {
    ($ax:expr $(; $($k:ident = $v:expr),* $(,)?)?) => { $crate::__kw!($crate::axislegend(&$ax) $(; $($k = $v),*)?) };
}
#[macro_export]
macro_rules! hideydecorations {
    ($ax:expr $(; $($k:ident = $v:expr),* $(,)?)?) => {
        $ax.hideydecorations($crate::__kw!($crate::HideDecorations::new() $(; $($k = $v),*)?))
    };
}   // hidexdecorations!, hidedecorations!: same shape
#[macro_export] macro_rules! linkxaxes { ($($ax:expr),+ $(,)?) => { $crate::linkxaxes(&[$(&$ax),+]) }; }   // + linkyaxes!, linkaxes!
#[macro_export] macro_rules! rich { ($($s:expr),* $(,)?) => { $crate::RichText::from_spans([$($crate::TextSpan::from($s)),*]) }; }
```

- `#[track_caller] #[doc(hidden)] pub fn __target<T: AsAxis + ?Sized>(t: &T) -> Axis`. `AsAxis` is implemented for `Axis`, `&Axis` and `GridPosition`. A position with no Axis panics with: "no Axis at fig.at(1, 2); create one with Axis::new(fig.at(1, 2))".
- The prelude's `pub use crate::{Axis, Figure, Legend, Label, Colorbar, lines, scatter, …}` imports each type or function together with its macro (separate namespaces).
- A misspelled key gives rustc's `no method named 'colr' found for struct 'Lines'`, pointing at the key. Wrong arity gives `no rules expected …`. Both are pinned by trybuild.

**Exported macros (27):** `kw! lines! scatter! scatterlines! heatmap! hist! barplot! band! hlines! vlines! ablines! text! Figure! Axis! Colorbar! Legend! Label! axislegend! hidexdecorations! hideydecorations! hidedecorations! linkxaxes! linkyaxes! linkaxes! rich!`. A was larger; `xlims!`, `colsize!` and friends are plain methods.

### 2.11 Window, live and errors

```rust
pub struct Screen { .. }                   // !Send; from fig.display()
impl Screen { pub fn is_open(&self) -> bool; pub fn pump(&self) -> Result<()>; pub fn wait(self) -> Result<()>; pub fn close(self); }
pub struct Live { .. }                     // Send + Sync; passed as &Live to the show_live closure
impl Live { pub fn is_open(&self) -> bool; pub fn close(&self);
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R;     // = figure.batch
    pub fn frame_due(&self) -> bool;                         // rendered_rev >= published_rev (backpressure)
    pub fn wait_frame(&self);                                // block until the latest state is presented
    pub fn figure(&self) -> &Figure; }

pub type Result<T, E = Error> = std::result::Result<T, E>;
#[non_exhaustive] #[derive(Debug)]
pub enum Error { Io(std::io::Error), Encode(String), UnsupportedFormat(String), NoGpuAdapter, Gpu(String),
    NotMainThread, Reentrant, EventLoop(String), WorkerPanicked(String), Font(String), Data(DataError) }
#[non_exhaustive] #[derive(Debug, Clone)]
pub struct DataError { pub plot: String /* "Lines #2 in Axis fig[1, 1]" */, pub kind: DataErrorKind }
#[non_exhaustive] #[derive(Debug, Clone)]
pub enum DataErrorKind { LengthMismatch { expected: usize, got: usize }, ShapeMismatch { expected: (usize, usize), got: (usize, usize) },
    Ragged { row: usize }, Unsorted, Stale }
// Display + std::error::Error (+source) hand-written; no foreign crate types in public signatures.
```

---

## 3. Architecture

### 3.1 Data flow

```
handles ──setter (convert off-lock, O(1) swap under lock)──▶ FigState (arena; Attr<T>; revs; dirty; batch_depth)
                                                             │ snapshot(): lock, clone attrs + Arcs, unlock   [O(#plots)]
                                                             ▼
  FrameInput ─▶ resolve (explicit ▷ fig theme ▷ defaults; cycle freeze) ─▶ limits (autolimits, links, follow, Float32Convert)
             ─▶ ticks (+ user tick closures) ─▶ text measure ─▶ protrusions ─▶ GridLayout solve ─▶ block rects (axis rects → integer units)
             ─▶ scene::build ─▶ DrawList (backend-neutral; memoised per stage on revs)
                                  ├─▶ render::gpu  (window surface | offscreen → PNG)
                                  └─▶ render::svg  (String/file; CPU only, f64)
```

- Coordinates are y-up with a bottom-left origin inside `layout/`, so the GridLayoutBase and Makie formulas port verbatim.
- `scene::build` flips once to y-down units.
- The GPU multiplies by `px_per_unit`. The SVG writes units into a `viewBox`.
- Ticks do not depend on pixel size, so one layout pass is enough. `DataAspect` and `autolimitaspect` iterate at most 3 times.
- During interaction, the ticklabelspace jitter guard freezes the space and restores it 0.2 s after the last event (Makie).

### 3.2 DrawList

```rust
pub(crate) struct DrawList { size_units: Vec2, bg: Color, axes: Vec<AxisXform>, items: Vec<DrawItem>, overlay: Vec<DrawItem> }
struct DrawItem { z: f32, seq: u32, clip: Option<RectU>, space: Space, prim: Prim }
enum Space { Figure /* units, y-down */, Data(AxisSlot) }
enum Prim {
    Lines   { src: Src<[f32; 2]>, cpu: Arc<[[f64; 2]]>, mode: Strip | Segments, closed: bool, style: LineStyleU, color: ColorSrc },
    Markers { src: Src<[f32; 2]>, cpu: Arc<..>, style: MarkerStyleU, color: ColorSrc, size: SizeSrc },
    Glyphs  (GlyphRun),                                  // figure space; sprite pipeline
    Field   { values: Arc<[f32]>, dims: (u32, u32), strides: (u32, u32), geom: CellGeom /* Regular{edge0, d} | Irregular{edges} */,
              cmap: ColorMapping, interpolate: bool },
    Mesh    { verts: Src<MeshVertex>, idx: Src<u32>, color: ColorSrc },   // bars, hist, band, spans, backgrounds, legend patches
    Rects   { rects: Vec<SnapRect> },                    // spines, ticks, grid: position-snapped, width-exact
}
enum Src<T> { Plot { id: PlotId, part: u8, rev: u64, data: Arc<[T]> }, Inline(Vec<T>) }
struct AxisXform { rect: RectU, lim: Rect64 /* scaled space */, xscale: Scale, yscale: Scale, xrev: bool, yrev: bool, rebase: Rebase }
```

**Z order (Makie).** Items are sorted stably by `(z, seq)`.

| element | z |
|---|---|
| axis background | −100 |
| grid | −10 |
| plots | 0 (+ `z()`) |
| ticks | +10 |
| spines | +20 |
| legend / colorbar | block scene |
| rect-zoom shade | +1000 |
| tooltip | +9000 |

**Clipping.** Plots and grid are scissored to the axis rect, rounded to physical pixels. Decorations are unclipped.

**Recipes lowered here:**
- hist: StatsBase edges (closed-left, NaN skipped) and normalisation, then bars;
- barplot: dodge/stack math, then rects, then mesh;
- band: strip mesh with NaN quads cut;
- scatterlines: lines plus markers;
- h/vlines: segments in axis fractions;
- ablines: segments spanning the current x limits.

All are cached per `(data_rev, style_rev)`. Geometry is in data-local f32 and drawn with the axis affine, so pan needs no re-upload.

### 3.3 Float precision (Makie Float32Convert, improved)

- **Per axis,** `Rebase { origin, scale }`. A plot buffer holds `f32((T(x) − origin) · scale)`, computed in f64, where `T` is identity, log10, log2, ln or sqrt applied on the CPU.
- **Rebase trigger (Makie's criterion, resolution 1e4):**
  - `Δ < 1e4 · eps(f32) · max(|lo|, |hi|)`, or the range leaves `floatmin·1e4 .. floatmax/1e4`;
  - hysteresis ×4.
  - A rebase re-uploads that axis's plots. It is rare, and it is counted in `RenderStats.rebases`.
- **Per frame,** the affine `local → framebuffer px` (`sx, sy, tx, ty`) is computed in f64 from limits, rect, reversal and ppu, and uploaded as one `vec4<f32>`. **Pan and zoom rewrite uniforms only**, with two exceptions: dashed lines (the screen-pixel arc length is recomputed on the CPU, O(n)) and rebases.
- **Heatmap** fragment→cell coefficients are computed separately in f64.
- **Log domain:** non-positive values become NaN, with one warning (D4).

### 3.4 GPU renderer (wgpu =30.0.1)

**Context**
- `static GPU: OnceLock<Result<Arc<Gpu>, String>>`, built from `Instance::new(InstanceDescriptor::new_without_display_handle())`, a HighPerformance adapter, and `required_limits = adapter.limits()` (16384 textures on the M3).
- One device serves headless export (any thread) and every window.
- A missing adapter gives `Err(NoGpuAdapter)`.

**Frame**
- One render pass per frame.
- 4× MSAA colour target: `TRANSIENT_ATTACHMENT | RENDER_ATTACHMENT` with `Clear(bg)` and `StoreOp::Discard`. Verified at M1, with a fallback to an ordinary MSAA texture.
- Resolved into the surface (`Bgra8Unorm`, **non-sRGB**, picked explicitly) or an offscreen `Rgba8Unorm` with `COPY_SRC`.
- No depth buffer; painter's order; `BlendState::PREMULTIPLIED_ALPHA_BLENDING`; `cull_mode: None`; `set_scissor_rect` per item.
- All colour math is in sRGB-encoded (gamma) space, like GLMakie and Cairo.

**Bind groups**
- `@group(0)`: `Globals { target_px: vec2f, ppu: f32 }`, a linear-clamp sampler and a nearest-clamp sampler.
- `@group(1)`, per item:
  - its uniform from one ring buffer with 256-byte dynamic offsets;
  - storage buffers, with a 16-byte dummy for unused slots;
  - the colormap LUT texture (256×1 `Rgba8Unorm`) or the glyph atlas.
- Bind groups are rebuilt only when a buffer is reallocated.

**WGSL.** `common.wgsl` (`px_to_clip`, `finite_bits`, `nan_bits`, `CMap` + `cmap()` with the half-texel remap, via `textureSampleLevel`) is prepended with `concat!(include_str!(..))`. The research §11 sketches are the starting point.

| Pipeline | Draw | Buffers | Uniform | Notes |
|---|---|---|---|---|
| `line` | TriangleStrip, `draw(0..4, 0..n_seg)`, vertex pulling | `pts: array<vec2f>`; `cum: array<f32>` (screen-px arc length, dashed only); `vcol: array<u32>` RGBA8 or `vval: array<f32>` values (stride 0 or 1) | `LineU { xform, color, linewidth_px, miter_limit = cos(π − θ), joinstyle, linecap, color_mode {uniform, rgba, value}, mode {strip, segments}, closed, n_breaks, pattern_len, breaks[2×vec4], CMap }` | Port of `lines.geom` (max_vertices 4) and `lines.frag`. Each vertex recomputes the identical flat joint data from p0..p3 with one `normalize()` form. Miter joints end on the shared miter line. Truncated joints use the flat-data discard split (exact translucency). AA_RADIUS 0.8 px; widths under 0.8 px fade. NaN and degenerate segments give zero-area quads. ±16k px guard band. Value colours are interpolated, then colormapped in the FS. Dashes are analytic from ≤ 8 breakpoints. `process_pattern` (dash joint adjustment) is a separate later sub-milestone. |
| `sprite` | TriangleStrip, `draw(0..4, 0..n)` | markers: `pos: array<vec2f>`, `col: array<u32>` or `val: array<f32>`, `size: array<f32>`, `rot: array<f32>` (strides 0/1); glyphs: `array<Glyph { pos_px, size_px, uv_rect, color: u32, angle }>` | `SpriteU { xform, color, stroke_color, size_px, stroke_px, shape, strides, CMap }` | Markers: analytic SDFs in Makie marker units (circle r 0.3525, rect 0.3157, …), AA 1/√2 px, GLMakie outer stroke, value colormapped in the VS. The quad is **tight**: half-extent = shape bbox · size/2 + stroke + 1 px (a circle quad is 0.705·size, not size). Glyph mode samples the R8 atlas: nearest when axis-aligned (0°/90° exact), linear otherwise. |
| `field` | TriangleStrip, `draw(0..4)` | `z: array<f32>` with strides (sx, sy); `xedges`, `yedges: array<f32>`; LUT | `FieldU { rect_px, imap, lmap, dims, strides, interpolate, irregular bits, colorscale, CMap }` | One quad = bbox ∩ axis rect (f64 on the CPU). Regular grid: `frag·a + b` (f64 coefficients). Irregular or log: binary search over edges in the FS. Regularity is detected with relative tolerance 1e-6 of the step, so linspace centres take the fast path. Nearest, or manual NaN-aware bilinear. Colorscale in the shader. Row bands above `max_storage_buffer_binding_size`. Also draws the Colorbar gradient (N×1). |
| `mesh` | indexed TriangleList | vertex `{pos: Float32x2, color: Unorm8x4}` or `{pos, value: Float32}` | `xform`, `CMap` | Fills, decoration rects, legend patches, rect-zoom shade, tooltip box. MSAA gives the AA, with no seams between adjacent bars. No FXAA. |

**Uploads**
- Per plot the GPU cache keys on `(data_rev, rebase_epoch, scale)`.
- `write_buffer` in place when capacity allows; otherwise grow ×1.5 and rebuild the bind group.
- `push` and `extend` write only `[uploaded_len..]`.
- Big CPU buffers go back to the plot's pool after upload, so a steady 2048² live update allocates nothing.
- `RenderStats { bytes_uploaded, draws, rebases }` is `#[doc(hidden)] pub`, for perf tests.

**Offscreen and PNG**
- Render at `round(size·ppu)`. Above `max_texture_dimension_2d`, render in tiles, with `t −= tile_origin` on every affine plus a scissor.
- `copy_texture_to_buffer` with 256-byte row alignment, `map_async`, `poll(PollType::Wait)`, then strip the padding.
- Un-premultiply only when `transparent`.
- `png` 0.18 with the sRGB chunk and pHYs = `round(96·ppu / 0.0254)` px/m.

**macOS colour**
- `SurfaceConfiguration { color_space: SurfaceColorSpace::Srgb, alpha_mode: Opaque, present_mode: AutoVsync }`.
- M1 checks with Digital Color Meter that #0072B2 reads (0, 114, 178) on the built-in P3 panel.
- If the layer colorspace is still nil (gfx-rs/wgpu#10286 is not in 30.0.1), `window/macos.rs` walks the NSView's layer and sublayers to the `CAMetalLayer` and sets `colorspace = CGColorSpace(kCGColorSpaceSRGB)` via objc2.
- Delete the shim when wgpu ships the fix.

### 3.5 SVG backend (CPU, f64, deterministic)

- Root: `<svg width="{W·ptpu}pt" height="{H·ptpu}pt" viewBox="0 0 W H">`, with one `<clipPath>` per axis.
- Numbers use ≤ 3 decimals with trailing zeros trimmed. Ids are sequential. There are no timestamps, so output is byte-stable.

| Primitive | SVG output |
|---|---|
| Lines | `<path>`; NaN starts a new `M`; `stroke-linejoin`/`stroke-linecap`; `stroke-miterlimit="2"` for π/3; `stroke-dasharray` = pattern × linewidth. Value-coloured lines become per-segment paths. |
| Markers | `<symbol>` per (shape, size) with `<use>` per point. The outer stroke is `paint-order="stroke"` with 2× width. Above 100k points: warn. `rasterize(true)` embeds a GPU-rendered PNG at 2×, falling back to vector with a warning if there is no GPU. |
| Field | `<image>` with a base64 PNG of the colormapped cells, **made on the CPU** from the LUT, one pixel per cell. `image-rendering="pixelated"` unless `interpolate`. Irregular grids become `<rect>` elements. |
| Mesh | `<path>`. Adjacent same-colour bars are merged into one path, so there are no conflation seams. |
| Rects | `<rect>` elements (spines, ticks, grid). |
| Glyphs | ab_glyph outlines (y-flipped) as `<symbol>` per (face, glyph), with `<use transform>` per glyph, as Cairo does. `SvgText::Text` emits `<text font-family="TeX Gyre Heros">` instead. |

### 3.6 Text system

**Fonts**
- `include_bytes!` of `TeXGyreHerosMakie-{Regular,Bold,Italic,BoldItalic}.otf` (CFF, about 700 KB), loaded with `ab_glyph::FontRef`.
- A unit test asserts ascender 947, descender −218, UPM 1000.
- `PxScale = fontsize_px × (asc − desc)/upem` (×1.165).

**Layout (Makie, exactly)**
- Advance-only, no kerning (the font has no kern or GPOS table).
- Line box `[desc, asc]` = 1.165 em.
- Vertical alignment uses the first line's ascender and the last line's descender.
- Bounding box `[0, hadv] × [desc, asc]`.

**Rich spans**
- Superscript: 0.66×, baseline +0.4·size, x offset 0.1·sup size.
- Subscript: 0.66×, baseline −0.25·size.
- Log labels are "10" + superscript. Scientific labels are mantissa + "×10" + superscript. U+2212 throughout.

**Atlas**
- R8Unorm, 2048², growing to 4096² and then 8192². Hand-written shelf packer.
- Key: `(face, glyph, round(px·4), subpixel bin 0..3 along the advance, angle bin)`.
  - Angle bin 0 covers 0° and ±90°: axis-aligned glyph, rotated quad, nearest sampling, snapped origin.
  - Other angles use 1° bins, rasterised rotated by transforming ab_glyph outline curves and drawing with `ab_glyph_rasterizer`.
- Baselines are snapped to whole device pixels.
- **When the atlas is full**, it grows. At the limit it evicts least-recently-used entries and **re-rasterises the current frame's glyph set before encoding**, so no frame draws missing glyphs.
- A ppu change (window 2 vs export 3.125) rasterises new sizes.

**Missing glyphs:** `.notdef`, with a warning once per codepoint.

### 3.7 Ticks, formatting, autolimits

| module | contents |
|---|---|
| `ticks/wilkinson.rs` | Literal port of PlotUtils `optimize_ticks` (research 2 §1.2): z descending, k ascending, Q order, r ascending, strict `>`, f64, `round_sigdigits` ties-to-even, `fallback_ticks`. Makie parameters k_min 3, k_ideal 5, k_max 10, strict_span. |
| `ticks/format.rs` | `format_ticks_auto`: scientific when `\|log10(max − min)\| > 4`; `plain_precision` from the shortest **f32** representation (matches Makie's noise hiding), digits printed from f64; U+2212; mantissa stripping only when all are integral. |
| `ticks/log.rs` | Default `LogTicks::integer()`: Wilkinson restricted to integer exponent steps when span ≥ 1 decade (D1), otherwise linear Wilkinson in data space with plain labels. `LogTicks::makie()` is the exact port. |
| `ticks/minor.rs` | `IntervalsBetween(n)` with mirroring (exact port). `LogMinor` = k·10ⁿ, k = 2..9, in each visible decade; when majors skip decades, only the skipped decades (D2). `Auto` = `IntervalsBetween(2)` on linear axes, `LogMinor` on log axes. |
| `ticks/linear.rs` | `LinearTicks` (MaxNLocator), `MultiplesTicks`, explicit ticks, closures (run on the snapshot). |
| `limits.rs` | `reset_limits`/`autolimits`: union of linked axes before margins; margins in scaled space; degenerate → ±\|v\| or (−1, 1); log at v = 1 → (0.1, 10) (D3); `tightlimits` permanently for heatmap; heatmap limits at cell edges; bars/hist include `fillto`; h/vlines count toward one dimension only; `autolimitaspect`; interaction validation; `follow` (D5); link propagation with a guard flag. |

### 3.8 Layout (full GridLayoutBase port)

Ported: `compute_rowcols`, `compute_col_row_sizes`, `align_to_bbox!`, `determinedirsize`, effective protrusions, `tight_bbox` / `resize_to_layout`, auto-growth, prepend, nested layouts, sides; sizes `Auto(trydetermine, ratio)`, `Fixed`, `Relative`, `Aspect`; align modes `Inside`, `Outside`, `Mixed`.

**Blocks**
- **Axis:** protrusion = `tickspace + ticklabelspace + pad + label + labelpadding` (spinewidth excluded — Makie's quirk kept), plus the title and titlegap. The rect is rounded to integer units.
- **Colorbar:** autosize (12, None); protrusion on the tick side.
- **Legend:** vertical: tellwidth true, tellheight false.
- **Label:** text box plus padding.
- **axislegend:** placed in the axis viewport with margin 6, outside the grid.

### 3.9 Window, interaction and live model (macOS)

**Event loop**
- winit =0.30.13. `thread_local! { static EL: RefCell<Option<EventLoop<UserEvent>>> }`, created lazily once per process.
- Before any winit call, `mainthread::check()` runs:
  - macOS: `libc::pthread_main_np() == 1`;
  - Linux: `gettid() == getpid()`;
  - Windows: always passes.
  - Failure returns `Err(NotMainThread)` with the hint "use fig.show_live".
- Reentrancy is detected with `try_borrow_mut` and returns `Err(Reentrant)`.
- One `App: ApplicationHandler<UserEvent>` owns `HashMap<WindowId, Screen>`: figure clone, surface, GPU cache, interaction state, inspector, last snapshot and DrawList.
- Windows are only created in `resumed` / `new_events`.

**Entry points**

| call | mechanism |
|---|---|
| `fig.show()` / `show_all` | `run_app_on_demand` with `ControlFlow::Wait` (0% CPU when idle). Returns when all windows close. Repeatable. Windows and surfaces are dropped before return. |
| `fig.show_live(f)` | `std::thread::scope`: `spawn_scoped` the simulation (thread name `sciplot-sim`), then run the loop on main. A drop guard sets `open = false`, then an explicit join. A worker panic becomes `Err(WorkerPanicked)` once the window closes. A main-thread panic or error trips the guard so the join cannot hang. The window stays open after the simulation returns; `live.close()` closes it. |
| `fig.display()` + `screen.pump()` | `pump_app_events(Some(ZERO))`, self-throttled: calls closer than 8 ms apart only check a timestamp. Never calls `el.exit()`. Renders only inside `RedrawRequested`. Documented caveat: it stalls during macOS modal live-resize. |

**Redraw**
- `Wake` → `request_redraw()` (coalesced by winit).
- `RedrawRequested`:
  1. clear `pending`;
  2. snapshot, unless a batch is open;
  3. compute outside the lock;
  4. upload dirty buffers;
  5. encode and present;
  6. store `rendered_rev`.
- `AutoVsync` caps rendering at the display rate (120 Hz ProMotion).
- `WaitUntil` is used only while a timer is pending (the 0.2 s tick-label freeze).

**Interactions (Makie defaults)**

| action | input |
|---|---|
| rectangle zoom | left drag after 2 px; `x`/`y` held restricts it; outside shaded (black, 0.2); selection clamped to the limits |
| pan | right drag |
| scroll zoom | `0.9^Δ` about the cursor; trackpad `PixelDelta/scale/16`; `x`/`y` held restricts it |
| reset limits | Ctrl+click → `reset_limits` (resumes `follow`) |
| full autolimits | Ctrl+Shift+click → `autolimits` |
| extras (D11) | pinch → zoom; Option+left drag → pan; double-click → reset |

- Interaction is a pure state machine: `interact::handle(&mut InteractState, &mut AxisLimits, Input, &Viewport) -> Effect`.
- It writes `targetlimits` under a brief lock and propagates across linked axes with the guard flag.

**Hover (DataInspector, on by default in windows; D6)**
- Runs on the last snapshot and never takes the lock.
- Scatter and lines: a CPU uniform grid in **data-local** space, built lazily per `data_rev` (not per view), queried within 10 px. For lines, the closest point on the segment.
- Heatmap: the inverse f64 affine gives the cell; the value comes from the CPU f32 mirror.
- Bars and band: rectangle and segment tests.

| Hovered | Text |
|---|---|
| points / lines | `x: …\ny: …` (6 significant digits) |
| heatmap | `x = …, y = …\n[i, j] = v` (0-based; v via the tick formatter), red cell outline width 2 |
| bars | as in Makie |
| band | as in Makie |

- Tooltip: white background, 1 px black outline, padding (5, 5, 3, 3), triangle 7, offset 10, fontsize 14, auto placement.
- It is drawn from the overlay list, so hovering never triggers a relayout.

**HiDPI**
- Logical window size = figure units; `ppu = scale_factor()`.
- The window size is a *display override* for layout; `fig.size` stays the export size.
- `Resized` / `ScaleFactorChanged` → relayout, reconfigure the surface and MSAA target, new glyph sizes.
- The `CurrentSurfaceTexture` enum is handled per research 4 §2.3.

**Dirty tracking**
- Per plot: `data_rev`, `style_rev`. Per axis: `limits_rev`. Per figure: `layout_rev`. All `u64`, never reset.
- Each pipeline stage is memoised on the revisions it reads.

---

## 4. Makie default constants to reproduce

| Area | Constants |
|---|---|
| Figure | size (600, 450) units = CSS px; figure_padding 16 (`Outside`); rowgap = colgap = 18; fontsize 14 (every `@inherit fontsize` → 14, including the **bold 14** title); bg white; textcolor black |
| Export | px_per_unit 2 (PNG → 1200×900); pt_per_unit 0.75 (SVG 450×337.5 pt); PNG pHYs 96·ppu dpi |
| Font | TeXGyreHerosMakie; UPM 1000, asc 947, desc −218 → line box 1.165 em (16.31 px at 14); advances: digit 556, `.` 278, `-` 333, `−` 584, `×` 584 |
| Axis text | titlegap 4, titlefont bold, subtitlegap 0; xlabelpadding 3, ylabelpadding 5; xticklabelpad 2, yticklabelpad 4; y label rotated +π/2 |
| Ticks | size 5, width 1, align 0 (outward), colour black; minor ticks hidden, size 3, width 1, `IntervalsBetween(2)` |
| Grid / spines | grid visible, width 1, RGBA(0,0,0,0.12); minor grid off, RGBA(0,0,0,0.05); spinewidth 1, all four spines black |
| Limits | autolimitmargin (0.05, 0.05) in scaled space; empty axis (0,10) / log (1,1000) / sqrt (0,100); heatmap → tight (0,0) permanently |
| Wilkinson | Q = [(1,1),(5,0.9),(2,0.7),(2.5,0.5),(3,0.2)]; weights 1/4, 1/6, 1/3, 1/4; k_min 3, k_ideal 5, k_max 10; strict_span |
| Formatter | scientific when \|log10 range\| > 4; U+2212; superscript 0.66×, +0.4·size, x-offset 0.1; subscript −0.25 |
| Protrusion (14 px) | x axis 5 + 16.31 + 2 (+16.31 + 3 with label); y axis 5 + maxw + 4 (+16.31 + 5); title 16.31 + 4; worked example → viewport (74, 59) 510×355 |
| Legend | frame black 1 px, bg white (hidden with the frame); padding 6; margin 0 (`axislegend` 6); patchsize (20, 20); rowgap 3, colgap 16, patchlabelgap 5; titlegap 8, groupgap 16; title bold; `axislegend` position :rt; vertical legend tellwidth true / tellheight false |
| Colorbar | size 12; vertical; flipaxis true (ticks right); ticklabelpad 3; labelpadding 5; minor `IntervalsBetween(5)` off; nsteps 100; clip triangles only when set (height 12·sin 60°) |
| Label | padding 0; halign/valign center; tell both |
| Lines | linewidth 1.5; butt caps; miter joins; miter_limit π/3 (GPU `cos(π − π/3)` = −0.5, SVG miterlimit 2); AA 0.8 px |
| Scatter | markersize 9; `:circle` r 0.3525 → 6.35 px; strokewidth 0; colour cycles, marker does not; AA 1/√2 |
| Marker geometry (× markersize) | rect half 0.315718; diamond half-diagonal 0.4465; utriangle (0, 0.485), (±0.36375, −0.2425); cross 0.375 × 0.1245; pentagon r 0.375; star5 0.45/0.21; FullCircle/FullRect = 1.0 |
| Linestyles (× linewidth) | dash [0,3,6]; dot [0,1,3]; dashdot [0,3,6,7,10]; dashdotdot [0,3,6,7,9,10,13]; dense/loose gaps 1/2 and 4/6 |
| Colormapping | viridis default (256 ColorSchemes entries; first (0.267004, 0.004874, 0.329415), last (0.993248, 0.906157, 0.143936)); auto range NaN-aware, lo = hi → ±0.5; low/highclip = end colours; nan_color transparent; LUT half-texel `(1 − 1/N)t + 0.5/N` |
| Heatmap / hist / bar / band | heatmap interpolate false; hist bins 15, `range(min, nextfloat(max))`, closed-left, gap 0; barplot gap 0.2, dodge_gap 0.03, width = min diff; band stroke 0; patch fills opaque |
| Palette | Wong #0072B2 #E69F00 #009E73 #CC79A7 #56B4E9 #D55E00 #F0E442; patchcolor (on white) #338EC1 #EBB233 #33B18F #D694B9 #78C3ED #DD7E33 #F3E968 |
| Interaction | ScrollZoom(0.1, reset 0.2) → 0.9^Δ; DragPan right; rect zoom left; Ctrl+click reset; Ctrl+Shift+click autolimits; drag threshold 2 px; double-click 0.2 s |
| Inspector / tooltip | range 10 px; offset 10; indicator red width 2; tooltip padding (5,5,3,3), triangle 7, placement :above → below if y > 0.75H, right if x < 0.25W, left if x > 0.75W |
| Themes | minimal: transparent bg, no grid, left+bottom spines only, ticks hidden, labelpadding 3, legend no frame and padding 0, colorbar ticks hidden / spinewidth 0 / ticklabelpad 5; light: textcolor gray50, grid (black, 0.07), no spines; dark: bg gray10, text gray45, grid (white, 0.09), palette re-lerped |

---

## 5. File layout and Cargo.toml

```
sciplot/
  Cargo.toml  README.md  LICENSE-MIT  LICENSE-APACHE
  assets/fonts/TeXGyreHerosMakie-{Regular,Bold,Italic,BoldItalic}.otf  GUST-FONT-LICENSE.txt  README.md (provenance: Makie artifact ad4e594b…, unmodified)
  src/
    lib.rs            re-exports, free fns, static Send/Sync asserts, #![deny(unsafe_code)]
    prelude.rs  macros.rs  error.rs  units.rs  style.rs (Marker, Linestyle, LineCap, JoinStyle, Normalization, Direction)
    figure/   mod.rs (Figure, FigShared, notify, batch)  state.rs (FigState, Attr)  arena.rs  position.rs (GridPosition, Span, Side, IntoSpan)  save.rs
    attrs/    table.rs (attributes! macro)  axis.rs  colorbar.rs  legend.rs  label.rs  plots.rs  resolve.rs
    theme/    mod.rs  defaults.rs (Makie table)  presets.rs  scope.rs
    color/    mod.rs (Color, parse, IntoColor, IntoColorSpec)  named.rs  palette.rs (WONG, patch lerp, Cycled, cycle freeze)  colormap.rs  cmap_data.rs (generated)
    data/     scalar.rs  data1d.rs  points.rs  data2d.rs (Field)  coords.rs  barx.rs  conv.rs (Into* traits)  ndarray.rs  nalgebra.rs  helpers.rs (linspace, logspace, iter)
    blocks/   axis/{mod.rs, limits.rs, link.rs, decorations.rs}  lineaxis.rs  colorbar.rs  legend.rs  label.rs
    plots/    mod.rs (common inherent methods macro, AnyPlot, ColorMapped)  lines.rs  scatter.rs  scatterlines.rs  heatmap.rs  hist.rs  barplot.rs  band.rs  reflines.rs  text.rs
    ticks/    wilkinson.rs  format.rs  log.rs  minor.rs  linear.rs  locator.rs
    layout/   gridlayout.rs  sizes.rs  alignmode.rs  protrusion.rs  solve.rs
    transform/ scale.rs  rebase.rs  affine.rs
    text/     font.rs  layout.rs  rich.rs (RichText, TextSpan, tex())  atlas.rs (packer, keys, rotated raster)  outline.rs
    scene/    snapshot.rs  drawlist.rs  build_axis.rs  build_blocks.rs  lower.rs (recipes)  inspect.rs (pick grids, tooltip text)
    render/gpu/ context.rs  frame.rs  cache.rs (buffers, pools, RenderStats)  offscreen.rs  tiles.rs  png.rs
                pipelines/{line.rs, sprite.rs, field.rs, mesh.rs}  shaders/{common.wgsl, line.wgsl, sprite.wgsl, field.wgsl, mesh.wgsl}
    render/svg/ mod.rs  writer.rs  num.rs  glyphs.rs  raster.rs (CPU heatmap PNG)
    window/   mod.rs  mainthread.rs  event_loop.rs  app.rs  screen.rs  live.rs  pump.rs  interaction.rs  inspector.rs  macos.rs  testing.rs
  examples/ s1_scatter.rs s1_show.rs s2_lines.rs s3_panels.rs s4_fields.rs s5_live.rs s5_pump.rs s6_log.rs s7_stats.rs s8_paper.rs
            gallery.rs  compare.rs  dump_data.rs  perf.rs  interact_check.rs
  tests/    ticks.rs format.rs minor.rs limits.rs layout.rs colors.rs theme.rs cycle.rs data_inputs.rs hist.rs interaction.rs
            threading.rs svg_snapshots.rs png_render.rs backend_consistency.rs ui.rs (trybuild) ui/*.rs ui/*.stderr
            window_smoke.rs (harness = false)  fixtures/*.json  snapshots/*.svg
  tools/    Project.toml (uses only locally installed Makie, CairoMakie, PlotUtils, StatsBase, ColorSchemes; JSON written by a
            20-line helper, no new packages)  gen_fixtures.jl  gen_layout_fixtures.jl  gen_colormaps.jl  makie_gallery.jl  attr_diff.jl
```

```toml
[package]
name = "sciplot"
version = "0.1.0"
edition = "2024"
rust-version = "1.89"
license = "(MIT OR Apache-2.0) AND LPPL-1.3c"      # LPPL-1.3c = GUST Font License for the embedded TeX Gyre Heros Makie fonts
include = ["src/**", "assets/fonts/**", "LICENSE-*", "README.md"]

[features]
default  = ["window"]
window   = ["dep:winit", "dep:libc", "dep:objc2", "dep:objc2-quartz-core", "dep:objc2-core-graphics", "dep:objc2-app-kit"]
ndarray  = ["dep:ndarray"]
nalgebra = ["dep:nalgebra"]
testing  = ["window"]                               # Screen::inject(InputEvent) for scripted window tests

[dependencies]
wgpu        = { version = "=30.0.1", default-features = false, features = ["std", "parking_lot", "wgsl", "metal", "vulkan", "dx12"] }
winit       = { version = "=0.30.13", optional = true }
pollster    = "1.0.1"
bytemuck    = { version = "1.25.2", features = ["derive"] }
parking_lot = "0.12.5"
png         = "0.18.1"
ab_glyph    = { version = "0.2.32", default-features = false, features = ["std"] }
ab_glyph_rasterizer = "0.1.10"                      # rotated-glyph rasterisation from transformed outlines
base64      = "0.23.1"
log         = "0.4.34"
ndarray     = { version = "0.17.2", optional = true }
nalgebra    = { version = "0.35.0", optional = true }

[target.'cfg(unix)'.dependencies]
libc = { version = "0.2.189", optional = true }

[target.'cfg(target_os = "macos")'.dependencies]    # colorspace shim; exact objc2 feature flags confirmed at M1
objc2               = { version = "0.6.4", optional = true }
objc2-quartz-core   = { version = "0.3", optional = true }
objc2-core-graphics = { version = "0.3", optional = true }
objc2-app-kit       = { version = "0.3.2", optional = true }

[dev-dependencies]
rand = "0.9.5"
serde_json = "1"
trybuild = "1"
resvg = "0.48.1"

[[test]]
name = "window_smoke"
harness = false
```

**Fonts and licence**
- The four Makie font files are copied **unmodified** from `~/.julia/artifacts/ad4e594b35357bcfafa2ed97db3137382a3f09bb/fonts/`, with the GUST licence text taken from its `LICENSES.md` (lines 360–389).
- The provenance README records the file sha256 values.

**Build environment:** the first build needs network access. wgpu, winit, ab_glyph, pollster, bytemuck_derive, ndarray and nalgebra are not in the local cargo cache.

---

## 6. Milestones

| # | Milestone | Done when |
|---|---|---|
| **M1** | **End-to-end skeleton.** Cargo project; `Gpu`; `mesh` + `sprite` (circle only); FigShared, handles, lock and notify; Figure/Axis/Scatter; naive autolimits; fixed single-axis layout (constant protrusions, no text); DrawList; offscreen, readback, PNG; `show()` with a wake proxy; main-thread check; surface colour space. | (1) `cargo run --example s1_scatter` writes a 1200×900 PNG with pHYs: 1000 Wong-blue dots of 6.35 units in a black 1-unit frame. (2) `s1_show` opens a 600×450-logical window on the M3, crisp at ppu 2; it re-lays out on resize; a second `show()` in the same process works. (3) Digital Color Meter reads #0072B2 as (0,114,178) on the P3 panel; if not, the objc2 shim lands now. (4) The TRANSIENT MSAA flag is verified, or the fallback is in place. (5) `tests/png_render.rs` finds white corners and about 1000 connected components, and skips cleanly with `NoGpuAdapter`. (6) `show()` from a spawned thread returns `Err(NotMainThread)`. |
| **M2** | **Full API surface, compiled.** Data traits, attribute-table macro (Axis, blocks, plots, themes), every macro, theme scoping, cycle freeze, errors, stale handles, `batch` gate. | S1–S8 compile as examples; missing primitives render nothing and warn once. trybuild pins the errors for an unknown keyword, a wrong type, wrong arity, `fig.at(1, 1..2)`, `Range<f64>` extent and a non-plottable input. Doctests cover `0..100`, `[1, 2, 3]`, `&Vec<f32>`, `Vec<i64>`, `t.iter().map(..)`, `iter(it)`, `["a", "b"]`, `0.0..=1.0`, `(&v, nx, ny)`, `&Array2`, `&Array1`, empty input. Static Send/Sync asserts compile, and a `compile_fail` doctest shows `Screen: !Send`. A threading test hammers `set_data` from 8 threads plus `batch` with no deadlock. |
| **M3** | **Axis math.** Wilkinson, format, log and minor ticks, LinearTicks, limits, links, follow, Float32Convert. | Exact equality (bitwise f64 and exact strings) against `fixtures/ticks.json` and `format.json` (≥ 1000 limit pairs from `gen_fixtures.jl`: research 2 §1.4 vectors, 1e11 ± 2, random magnitudes and offsets, near-singular, 1e±300) and against `minor.json` (`get_minor_tickvalues`). Autolimit fixtures match Makie `finallimits`. |
| **M4** | **Text and layout.** Fonts, rich text, `tex()`, atlas, GridLayout port, Axis/LineAxis protrusions and decorations (snapped rects), Legend/Colorbar/Label autosize, `resize_to_layout`. | The metric unit test passes (947/−218/1000). The worked example gives (74, 59) 510×355. For 15 reference figures (S2–S4, S8, plus spans, Side labels, nested grids, `f[0, :]`, Aspect columns, horizontal legend), `fig.debug_layout()` matches `gen_layout_fixtures.jl` dumps (viewports, tick values and strings, legend/colorbar/label bboxes) within 0.01 unit before rounding and exactly after. |
| **M5** | **Lines and SVG.** `line` pipeline in stages — segments/caps, then miter/bevel with the discard split, then round, then dashes (without `process_pattern`); cycling; axislegend; full SVG backend. | CPU mirror tests of the extrusion and joint math pass. The line torture page (widths 0.5–20 px, alpha 0.5, acute angles, zero-length segments, NaN runs, 180° reversal) shows no double-blended joints. S2 and S3 SVGs pass `xmllint --noout` and open in Safari and Chrome. SVG snapshots are pinned. resvg(SVG) vs GPU PNG mean abs diff < 1.5% per channel. |
| **M6** | **Fields and colour.** `field` pipeline (regular, irregular/log, interpolate, NaN, colorscale, bands), LUTs, Colorbar with clip triangles, value-coloured scatter/lines on the GPU, all Data2D inputs. | S4 renders. Orientation test: a field that is nonzero only at `(i = nx−1, j = 0)` lights the bottom-right cell for Field, the tuple, `Vec<Vec>`, ndarray (both memory orders) and DMatrix. Colorbar ticks for (0, 1.2) match Makie. A linspace-centred grid takes the regular path. |
| **M7** | **Stats, log and reference plots.** hist, barplot (dodge, stack, categorical), band, h/v/ablines, text plot, log axes. | S6 and S7 render. Hist edges and normalisations match StatsBase fixtures; the pdf integrates to 1 ± 1e-12. Bar ticks read CG, GMRES, BiCGStab, Jacobi. Log S6 shows majors at integer decades and minors at 2..9. |
| **M8** | **Interaction and hover.** Pure interaction state machine, overlay, inspector with data-local pick grids, jitter guard, Mac extras. | `tests/interaction.rs` scripts (zoom about the cursor, rect zoom with x-restrict, right-drag pan, Ctrl-click reset, Ctrl+Shift autolimits, lock flags, log-space zoom, linked propagation) match Makie's formulas. The inspector text is asserted for a known heatmap cell and scatter point. The manual M3 checklist (§7) passes. Idle CPU ≈ 0%. |
| **M9** | **Live.** Pools, tail uploads, `follow`, `show_live` (scoped, guard, panic path), `display`/`pump`, `frame_due`, `wait_frame`, `show_all`. | S5 (both forms) presents at 120 Hz while the simulation runs at ≥ 95% of headless throughput. A `wait_frame` + readback test shows the title step always matches the field. `RenderStats.bytes_uploaded == 0` (excluding uniforms) during a scripted pan. Closing the window returns `Ok`; a worker panic returns `Err(WorkerPanicked)`; a main-thread error does not hang. |
| **M10** | **Themes, paper export and polish.** Themes, `Save::dpi`, `rasterize`, tiling, `process_pattern` sub-milestone, perf pass, docs. | S8: 1200×900 PNG with pHYs 11811 px/m, SVG `width="288pt" height="216pt"`. Minimal, light and dark snapshots of S2 match the CairoMakie references. A 20000×15000 tiled render succeeds. The §7 perf targets are met. Every public item has a doc example. `attr_diff.jl` reports v1 coverage. |

---

## 7. Verification plan

**Unit tests (CPU only, run in CI)**
- Ticks, format and minor ticks against Julia fixtures (exact).
- Limits edge cases: a single point, log at 1, h/vlines only, heatmap + scatter, linked unions.
- Hist, bar-width, dodge and stack maths; band triangulation with NaN.
- Colour parsing (every Colors.jl name in the fixtures); WONG and WONG_PATCH hex values; viridis endpoints.
- Units: `12.0 * PT == 16.0`, `4.0 * INCH == 384.0`.
- Theme merge precedence; cycle freeze (a colour set later does not shift other plots).
- Data2D equivalence: identical uploads from every input form.
- Text metrics; `tex()` parsing; the pure layout solver on GridLayoutBase's own test cases.

**Julia fixtures**
- `julia --project=tools tools/gen_fixtures.jl` and `gen_layout_fixtures.jl` use CairoMakie, `Makie.update_state_before_display!`, then dump `ax.scene.viewport[]`, LineAxis tick values and strings, and `layoutobservables.computedbbox`.
- The developer runs them; the JSON is committed, so `cargo test` never needs Julia.
- `gen_colormaps.jl` writes `cmap_data.rs`.

**SVG snapshots.** `tests/svg_snapshots.rs` covers S2, S3, S4, S6, S7, S8 and gallery figures with seeded data (xorshift), compared byte-for-byte. `SCIPLOT_BLESS=1` rewrites them.

**Gallery.** `cargo run --release --example gallery` writes `target/gallery/*.png|svg` for S1–S8 (S5 as a still after 500 steps) plus stress figures:
- line torture, every marker with strokes, dashed lines on log axes;
- heatmap on a log axis, irregular grid, reversed axes, 1e9-offset time axis;
- NaN gaps, empty axis, 1-point data, 50-series cycling;
- all themes, horizontal legend, lowclip/highclip, a non-square "L" orientation field.

It also writes an `index.html` contact sheet.

**CairoMakie side-by-side**
- `examples/dump_data.rs` writes each scenario's exact arrays. `tools/makie_gallery.jl` renders the Julia twins at ppu 2 (PNG and SVG) and dumps `layout.json`.
- `examples/compare.rs` writes `target/compare/index.html` with sciplot | Makie | diff heatmap (plotted with sciplot itself).
- Pass criteria:
  - layout numbers and tick strings exactly equal;
  - pixels differing by ΔRGB > 24 < 0.5%, after excluding a 1 px dilation of reference edges;
  - no connected region above 10% difference larger than 6 px in the 4× downsampled diff.
- Known deviations (§8, D-list) are masked and listed.

**Cross-backend.** `tests/backend_consistency.rs`: resvg rasterises the sciplot SVG at the same ppu; mean abs diff against the GPU PNG must be < 1.5% per channel.

**Window checks**
- Automated: `tests/window_smoke.rs` (`harness = false`, main thread, feature `testing`) opens S1 and S5, injects scroll, drag, rect-drag with `x`, Ctrl-click, a hover over a known cell, then Close. It asserts `finallimits`, linked propagation, the tooltip string, frame count and clean exit. `SCIPLOT_AUTOCLOSE=3` smoke-runs every windowed example.
- Manual on the M3, with trackpad and mouse, on the built-in P3 panel and an external display:
  - colours match the PNG;
  - 1 px spines are crisp at ppu 1 and 2;
  - live resize;
  - moving the window between scale factors;
  - pinch and scroll zoom; Option-drag and right-drag pan;
  - Ctrl-click reset;
  - no tick-label jitter while zooming;
  - closing during S5;
  - calling `show()` twice.

**Performance** (`examples/perf.rs`, release, M3; window 1200×900 logical at ppu 2 = 2400×1800 physical; CPU timing plus `on_submitted_work_done`; 300-frame scripted zoom)

| Case | Target |
|---|---|
| 1M scatter, markersize 4, pan/zoom | p99 < 8.3 ms, 0 data bytes uploaded |
| 1M scatter, markersize 9 (default; ~280M fragments/frame, fill-rate bound; tight quads) | p99 < 16.7 ms, reported alongside the size-4 case |
| 1M-point solid line, width 1.5, pan | p99 < 8.3 ms |
| 1M-point dashed line, pan (CPU arc length + 4 MB upload per frame) | p99 < 16.7 ms |
| 1M scatter first frame (convert + upload + draw) | < 40 ms |
| 2048² heatmap render, pan | < 3 ms GPU |
| 2048² `set_data` from f64 on the caller thread | < 10 ms |
| lock hold p99 (2048² updates at 30 Hz while panning) | < 50 µs; renderer never blocks > 100 µs |
| S5 256², publishing every 20 steps | sim ≥ 95% of headless; 120 Hz presented |
| hover pick-grid build at 1M (per data_rev) | ≤ 30 ms |
| hover query | < 1 ms |
| S1 PNG export, warm device | < 150 ms |
| idle | ~0% CPU |

---

## 8. Non-goals and known limitations (v1)

**Non-goals**
- 3D and volumes; `image` (RGB) plots; contour/contourf; errorbars; stairs/stephist; stem; boxplot/violin; polar axes.
- Dim-converts (dates, units, categorical on arbitrary plots other than barplot).
- PDF export; video-recording API (use `render_rgba` from the worker).
- Observables/`lift` graph (live handles and setters replace it); `current_axis()` / implicit targets.
- Legend click-to-toggle; web/wasm; GLES backend; winit 0.31 (migrate once stable; isolated in `window/`).
- GLMakie FXAA/OIT; LaTeX (only `tex()` mini-markup); system font discovery (custom fonts via `FontId::load`).

**Known limitations**
- Translucent lines darken at self-crossings on the GPU, as GLMakie does; SVG does not, as Cairo does not.
- Dashes restart at joints until `process_pattern` lands in M10.
- The GPU marker stroke is drawn outside the shape; SVG approximates this with `paint-order`.
- Pump mode stalls during macOS live-resize; `show_live` does not.
- Glyph coverage is limited to TeX Gyre Heros: no ∇, ∫, ℏ or Unicode superscripts; use `tex()`.
- Large vector scatters produce huge SVGs; use `rasterize(true)`.
- Winit windows cannot be tested under the default `cargo test` harness (`harness = false` tests only).
- i64/u64 above 2^53 lose precision.

**Deliberate deviations from Makie**

| # | Deviation |
|---|---|
| D1 | Log majors on integer decades (`LogTicks::makie()` for exact Makie) |
| D2 | Log minors at 2..9·10ⁿ |
| D3 | Singular log limit v → (v/10, 10v) |
| D4 | Non-positive values on log axes are masked with a warning |
| D5 | `follow` autoscale for live data |
| D6 | Inspector on by default; heatmap readout shows 0-based `[i, j]` plus coordinates |
| D7 | Categorical bars in order of appearance |
| D8 | Decoration rects position-snapped |
| D9 | MSAA instead of FXAA |
| D10 | Window resize is a display override, not a change to `fig.size` |
| D11 | Pinch, Option-drag and double-click added to Makie's bindings |
| D12 | Plot-backed Colorbar misuse warns instead of erroring |

---

## 9. Open questions for the user

1. **Log-axis defaults.** Should D1 and D2 (integer-decade majors, 2..9 minors) be the default as recommended, or should sciplot default to Makie-exact `LogTicks`/`IntervalsBetween(2)` and make the readable form opt-in?
2. **Font packaging.** Ship the four GUST-licensed fonts inside `sciplot` (licence `(MIT OR Apache-2.0) AND LPPL-1.3c`, as recommended), or split them into an `sciplot-fonts` crate so the core crate is purely MIT/Apache?
3. **Default pan gesture on macOS.** Keep Makie's right-drag as primary, with Option-drag and pinch as extras (recommended), or make `InteractionScheme::Trackpad` (two-finger scroll pans, pinch zooms) the default on macOS?