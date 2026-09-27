# sciplot — a Makie-style plotting crate for Rust (plan)

## Context
Rust plotting is wordy (plotters' builder chains) and has no good "look at my simulation field live"
story. The goal is a minimalist, easy API that copies Makie's best practices (Figure/Axis/GridLayout
object model, mutating-into-axis vs one-liner, unitless CSS-px size model, Wong palette, viridis,
themes, Colorbar-takes-the-plot, in-place updates). With it, `scatter(&x, &y).save("a.png")` just works,
and a running PDE simulation can push a heatmap into a pan/zoom/hover window.

User decisions (fixed):
- Output: a native interactive window (GLMakie-like) **and** PNG + SVG export (CairoMakie-like).
- Renderer: GPU via **wgpu** with custom GLMakie-style pipelines, plus a separate SVG writer.
- Attributes: builder methods plus thin `macro_rules!` keyword sugar.
- v1 plots:
  - lines, scatter (+ scatterlines);
  - heatmap + Colorbar;
  - multi-panel grids (spans, linked axes, shared legends);
  - hist / barplot / band;
  - log scales, legends, themes.

Environment:
- An empty directory, `/Users/dansprague/Documents/repos/sciplot`, that is not a git repo. Rust 1.98.1, macOS on an Apple M3 (Metal).
- Reference sources, all under `~/.julia/packages/`: Makie 0.24.14, GLMakie 0.13.14, CairoMakie 0.15.14, GridLayoutBase 0.11.3 and PlotUtils 1.5.0. They are the reference for defaults, algorithms and shaders.
- Makie fonts: `~/.julia/artifacts/ad4e594b35357bcfafa2ed97db3137382a3f09bb/fonts/`.

How this plan was made: a research and design workflow.
- Four research reports: Makie defaults, Makie algorithms, GLMakie → WGSL, and the Rust crate ecosystem. All Makie claims cite the local source.
- Three competing designs, three judges, a synthesis, then an adversarial critique.
- The outputs are in the session scratchpad
  `/private/tmp/claude-501/-Users-dansprague-Documents-repos-sciplot/691d7a35-c8ae-4f0c-8088-c8ffc534fa49/scratchpad/`:
  - `synthesis.md`: the full design, about 1300 lines. It has the API reference, WGSL notes, and the Makie constants with citations.
  - `critique.md`
  - `r_defaults.md` = Makie defaults
  - `r_algos.md` = Makie algorithms
  - `r_eco.md` = the **GLMakie→wgpu** report (the file name is misleading)
  - `r_gpu.md` = the **Rust ecosystem** report (the file name is misleading)
- In M0 these files are copied into `docs/`. **Where this plan and `docs/DESIGN.md` disagree, this plan wins.** The plan already includes the critique's fixes.

---

## 1. Key design decisions

| Topic | Decision |
|---|---|
| Object model | `Figure` → `GridLayout` → blocks (`Axis`, `Colorbar`, `Legend`, `Label`), placed with `fig.at(row, col)`. Plots live in an `Axis`. **Grid positions are 1-based and inclusive, as in Makie:** `fig.at(2, 1..=2)`. `1..2` is a compile error with a hint, and `0` panics with "grid positions are 1-based". Prepending is explicit: `fig.at(Prepend, ..)`. Anything that indexes *your data* (the inspector's `[i, j]`, `Field`) is 0-based. |
| Handles | `Figure`, `Axis` and every plot and block handle are `Arc<FigShared>` + id, so they are `Clone + Send + Sync + 'static`. They don't borrow the figure, so you can hold several axes and move them into a sim thread. There is one non-reentrant `parking_lot::Mutex<FigState>` per figure, and only O(1) work happens under it. |
| Setters | One shape everywhere: `fn attr(&self, v) -> Self`. The same method builds (`ax.lines(..).color(RED)`), serves the macros, and updates live (`ax.title(format!("step {n}"))`). There is no terminal `.build()`; plots register at creation. |
| Three ways to plot (Makie's) | `ax.lines(x, y)` or `lines!(ax, x, y; kw…)` draw into an existing axis. `fig.at(1, 2).lines(x, y)` creates an Axis there plus the plot. The free function `lines(x, y)` creates a new Figure+Axis. All three return the **plot handle**, which has `.save()`, `.show()`, `.figure()`, `.axis()` and `.unpack() -> (Figure, Axis, Lines)`. The free functions are `#[must_use = "creates a Figure; call .save() or .show()"]`. |
| Keyword sugar | `lines!(ax, &x, &y; color = RED, linewidth = 2, label = "sin")` expands to the builder chain. The same pattern covers `Figure!`, `Axis!`, `Colorbar!`, `Legend!`, `Label!`, `axislegend!`, `hide*decorations!`, `link*axes!` and `kw!(expr; …)`. A misspelled key gives rustc's `no method named 'colr' found for struct 'Lines'`. A Makie-style comma before the keywords hits a `compile_error!` arm: "use `;` before keyword arguments". |
| Data input | `Scalar` is sealed and covers every int and float type, with no `Send`/`'static` supertraits (so `&T` impls compile). `Data1D` has **explicit** impls, with no blanket: slices, `Vec`, arrays, integer ranges, common iterator adapters (so `t.iter().map(..)` needs no `.collect()`), the `sciplot::iter(it)` escape hatch, and ndarray behind a feature. `Data2D`: `Field::new(&v, nx, ny)`, `(&v, nx, ny)`, `Vec<Vec<T>>` and ndarray `Array2`. The first index is x (Makie's `z[i, j]`). |
| Heatmap coordinates (Makie-exact) | `a..=b` or a vector of length n = cell **centres**. A vector of length n+1 = edges. `Edges(a, b)` = outer edges, Makie's `EndPoints`, which is what finite-volume grids want. With no coordinates, the centres are `1..=n`. |
| Errors | A programmer error at the call site (length mismatch, bad colour literal) panics with `#[track_caller]`. Live setters also have `try_*` variants. I/O, GPU and window failures return `sciplot::Result`. |
| Nothing auto-shows | There is no `Drop` magic; `save` and `show` are explicit (Makie practice). |
| Units | Makie's unitless CSS-px model: default `size (600, 450)`, `fontsize 14`, PNG `px_per_unit = 2`, SVG `pt_per_unit = 0.75`. Constants `INCH`, `PT`, `CM`, `MM`; export at a given DPI with `Save::dpi(300)`. |
| Look | Makie 0.24's defaults, reproduced exactly (table in DESIGN §4): the TeX Gyre Heros Makie font (bundled, GUST licence), Wong palette, viridis, grid alpha 0.12, 5 px outward ticks, Wilkinson ticks, 5 % autolimit margins, tight limits for heatmaps. |
| Log axes | Major ticks on integer decades, labelled 10ⁿ. Minor ticks at 2..9·10ⁿ; when majors skip decades, minors go at the skipped 10ⁿ. This is the readable default; `LogTicks::makie()` gives Makie-exact ticks. |
| Themes | `set_theme` is process-global, as in Makie. `with_theme(t, \|\| …)` adds a thread-local, scoped override. `theme_minimal`, `theme_light` and `theme_dark` are included. |
| Live updates | Handle setters plus dirty revision counters take the place of an Observables graph. `fig.show_live(\|live\| { … })` runs the sim on a **scoped worker thread**, which can borrow locals, while the winit loop keeps the macOS main thread. `live.batch(\|\| …)` makes updates to several attributes atomic within a frame. The secondary pump mode is `let s = fig.display()?; loop { …; s.pump()?; }`. |

## 2. What user code looks like (full S1–S8 in DESIGN §1, with the critique fixes applied)

```rust
use sciplot::prelude::*;

// S1: one-liners
scatter(&x, &y).save("scatter.png")?;            // 600×450 units → 1200×900 px
scatter(&x, &y).show()?;                         // window: pan/zoom/rect-zoom/hover

// S2/S3: explicit figure, keyword sugar, multi-panel
let fig = Figure!(size = (900, 650));
let a = Axis!(fig.at(1, 1); title = "ω = 1", ylabel = "u (V)");
let b = Axis!(fig.at(1, 2); title = "ω = 2");
let c = Axis!(fig.at(2, 1..=2); xlabel = "t (s)");
lines!(a, &t, t.iter().map(|t| t.sin()); label = "model");
scatter!(a, &td, &yd; label = "measurement", color = WONG[1], markersize = 7);
linkxaxes!(a, b);
hideydecorations!(b; grid = false);
Legend!(fig.at(1..=2, 3), &[&a, &b, &c]; unique = true);
fig.save("panels.svg")?;

// S4: heatmap on a finite-volume grid + colorbar
let hm = heatmap!(ax, Edges(0.0, lx), Edges(0.0, ly), Field::new(&n, nx, ny); colormap = Colormap::MAGMA);
Colorbar!(fig.at(1, 2), &hm; label = "n (a.u.)");
let p = fig.at(2, 1).lines(&t, &e);              // Makie's lines(f[2, 1], x, y): new Axis + plot

// S5: live simulation (correct on macOS)
let ax_d = Axis!(fig.at(1, 3); ylabel = "mean v", yticklabelspace = 50.0);   // no layout jitter as the data grows
fig.show_live(|live| {
    let mut step = 0;
    while live.is_open() {
        sim.step(); step += 1;
        if step % 20 == 0 {
            live.batch(|| {
                hm.set_data(Field::new(&sim.v, N, N));
                ax.title(format!("step {step}"));
                diag.push(step as f64, sim.mean_v());
            });
        }
    }
})?;

// S8: paper figure
with_theme(theme_minimal().fontsize(12.0 * PT), || {
    let fig = Figure!(size = (4.0 * INCH, 3.0 * INCH));
    …
    fig.save_with("fig.png", Save::dpi(300))?;   // 1200×900 px
    fig.save("fig.svg")                          // width="288pt" height="216pt"
})?;
```

## 3. Architecture

```
handles ─setter─▶ FigState (arena, Attr<T>=Inherit|Set, revisions, dirty flags)
                    │ snapshot: lock, clone Arcs, unlock   (O(#plots))
                    ▼
 resolve theme + cycles ─▶ autolimits/links ─▶ ticks + labels ─▶ text measure ─▶ protrusions ─▶ GridLayout solve
   ─▶ scene::build ─▶ DrawList (backend-neutral; Space::Figure | Space::Data(axis))
                        ├─▶ render::gpu (window surface | offscreen texture → PNG)
                        ├─▶ render::svg (deterministic string)
                        └─▶ CPU PNG fallback: SVG → resvg/tiny-skia when there is no GPU adapter (feature `cpu-png`, on by default)
```

### Storage and precision
- **Plot positions:**
  - Kept as an **f64 master** (`Arc<Vec<[f64;2]>>`).
  - The render thread applies the scale transform (log10 etc.) and the per-axis f32 rebase (Makie Float32Convert style), then converts to f32. This happens only at upload, and only when `data_rev` changes or a rebase happens.
  - `push`/`extend` use chunked, append-only storage (`Vec<Arc<[..; CHUNK]>>` + tail), so appending stays O(1) under the lock and only the tail is uploaded.
- **Values** (heatmap z, colour values):
  - Converted on the caller's thread to f32 `(v − off)·k`, so 1e40-scale fields survive, with extrema computed in f64.
  - Buffers are `Arc<Vec<T>>`. A buffer that has been replaced is recycled through `Arc::try_unwrap`.
  - **The latest buffer is never pooled**: it doubles as the hover mirror.
- **Pan and zoom never re-upload data.** The per-frame data→pixel affine is computed in f64 and sent as one uniform. The exceptions are dashed-line arc lengths and rare rebases.

### GPU (wgpu 30)
- **Shared:** `Device`, `Queue`, pipelines and colormap LUTs.
- **Per render context** (each window, each export call): the glyph atlas and the per-plot GPU caches, so a worker can export PNGs while a window renders. On device loss, rebuild the device and drop all caches.
- **Frame setup:**
  - one pipeline set, since the surface and offscreen targets are both `Bgra8Unorm` and readback swizzles;
  - 4× MSAA, premultiplied alpha, painter's order, a scissor per axis;
  - colour math in sRGB-encoded space, as Cairo and GLMakie do.

The four pipelines:
- `line`: a port of GLMakie's `lines.geom` to instanced 4-vertex strips. Vertices are pulled from storage buffers (p0..p3 per segment). It does miter, bevel and round joins, caps, NaN breaks, 0.8 px AA and analytic dashes.
- `sprite`: markers (analytic SDFs with Makie's marker geometry, stroke, per-point colour and size) **and** glyphs (R8 atlas quads; rotated text samples the upright glyph linearly).
- `field`: heatmaps and the colorbar gradient.
  - Inputs: an `array<f32>` of values plus a 256×1 LUT, with colorrange, clip and NaN colours as uniforms.
  - Regular grids use an affine map. Irregular or log-axis grids use a binary search over edges.
  - Sampling is nearest or a manual bilinear.
- `mesh`: indexed triangles. It draws:
  - bar, hist and band fills, and legend patches;
  - pixel-snapped decoration rects (spines, ticks, grid);
  - the rect-zoom shade and the tooltip box.

### Export, text, ticks and layout
- **PNG:** render offscreen at `round(size·ppu)`, read back with 256-byte row alignment, encode with the `png` crate including the sRGB and pHYs chunks. Above 16384 px it returns a clear error that suggests a lower DPI.
- **SVG:** CPU, f64, byte-stable.
  - Lines are `<path>`; markers are `<symbol>`/`<use>`.
  - A heatmap is an embedded base64 PNG, upsampled so each cell is at least 4 px, with `pixelated` / `optimizeSpeed`.
  - Decorations are `<rect>`. Glyphs are outline `<symbol>`s from ab_glyph.
  - Size is written as pt with a `viewBox` in figure units.
- **Text:** ab_glyph on the bundled CFF fonts, using Makie's exact layout (advance-only, line box 1.165 em).
  - Rich spans provide superscripts and subscripts. Log labels are `10` + a superscript, and the minus sign is U+2212.
  - `tex("k^{-5/3}")` is opt-in.
  - The glyph atlas is a shelf-packed R8 texture.
- **Ticks and limits:**
  - a literal port of PlotUtils `optimize_ticks`, and Makie's tick formatting;
  - log and minor ticks, and autolimits with margins in scaled space;
  - linked axes, and `follow` for live data;
  - `x/yticklabelspace` to stop layout jitter during live updates.
- **Layout:** a port of the GridLayoutBase solver.
  - Column and row sizes: Auto, Fixed, Relative, Aspect.
  - Gaps and spans; protrusions so that spines line up across cells.
  - `tellwidth`/`tellheight` for Legend and Colorbar; `resize_to_layout`; nested grids.

### Window (winit 0.30, `ApplicationHandler`)
- **Main-thread check:** `objc2::MainThreadMarker`, which is safe. Calling from another thread gives `Err(NotMainThread)` with a hint to use `show_live`.
- **Entry points:**
  - `show()`: `run_app_on_demand` with `ControlFlow::Wait`, so idle CPU is 0 %.
  - `show_live`: a worker under `std::thread::scope`, a drop guard, and an `EventLoopProxy` wake on the dirty edge. A worker panic is logged right away and put in the window title, then returned as `Err(WorkerPanicked)`.
  - `display()` / `pump()`: `pump_app_events`.
  - `wait_frame` never hangs: it returns at once when the window is occluded or closed, and there is a `_timeout` variant.
- **HiDPI:** the logical window size equals the figure units, and ppu = the scale factor.
- **macOS P3 colour:** use `SurfaceColorSpace::Srgb`. If the layer's colorspace is still nil, get the `CAMetalLayer` via `surface.as_hal::<Metal>` and set sRGB, reapplying after every `configure`.

### Interaction (Makie's bindings)
- Scroll zoom about the cursor, with x/y key locks.
- Left-drag rectangle zoom; right-drag pan; Ctrl+click reset.
- Mac extras: trackpad pinch zoom, Option-drag pan, double-click reset.
- **Hover inspector, on by default:**
  - lines and scatter show the nearest point within 10 px, using a grid in data space;
  - a heatmap cell shows `x, y, [i, j] = v` with a red outline;
  - it is drawn as an overlay, so hovering never re-runs layout.

## 4. v1 scope

**In:**
- everything in §1–3 and S1–S8;
- `hlines`, `vlines`, `ablines` and `text` annotations;
- reversed axes, and `lowclip`/`highclip`/`nan_color`;
- linked axes, and `colsize`/`rowsize`/`colgap`/`rowgap`.

**Deferred to v1.1+, to keep v1 lean:**
- the nalgebra feature;
- rasterising glyphs at arbitrary angles, and tiled renders above 16384²;
- SVG `rasterize` and a `<text>` output mode;
- legend groups and `nbanks`;
- adjusting dash patterns at line joins (`process_pattern`);
- inspector support for bars and bands, and a custom `inspector_label`;
- `LinearTicks` and `MultiplesTicks`;
- `theme_black`.

**Non-goals:** 3D, RGB `image`, contour, errorbars, boxplot/violin, polar axes, dates and units, PDF, a video API (use `render_rgba`), Observables/`lift`, a current-axis global, legend click-to-toggle, wasm. Not publishing to crates.io in v1.

## 5. Crate layout and dependencies
```
sciplot/
  Cargo.toml  rust-toolchain.toml  README.md  LICENSE-MIT  LICENSE-APACHE
  docs/DESIGN.md  docs/critique.md  docs/research/{makie-defaults,makie-algorithms,glmakie-rendering,rust-ecosystem}.md
  assets/fonts/TeXGyreHerosMakie-{Regular,Bold,Italic,BoldItalic}.otf + GUST licence + provenance (sha256)
  src/ lib.rs prelude.rs macros.rs error.rs units.rs style.rs
       figure/ (Figure, FigShared, batch, GridPosition/Span/Prepend, save)   attrs/ (attribute-table macro → setters + theme structs)
       theme/  color/ (Color, named, WONG, colormaps generated from ColorSchemes)   data/ (Scalar, Data1D, Data2D, Field, Edges, barx, storage chunks)
       blocks/ (axis, lineaxis, colorbar, legend, label)   plots/ (lines, scatter, scatterlines, heatmap, hist, barplot, band, reflines, text)
       ticks/ (wilkinson, format, log, minor)   layout/ (gridlayout solver)   transform/ (scale, rebase, affine)
       text/ (font, layout, rich, atlas, outline)   scene/ (snapshot, drawlist, build, lower recipes, inspect)
       render/gpu/ (context, frame, cache, offscreen, png, pipelines/*.rs, shaders/*.wgsl)   render/svg/   render/cpu.rs (resvg fallback)
       window/ (mainthread, app, screen, live, pump, interaction, inspector, macos)
  examples/ s1…s8 + gallery.rs compare.rs dump_data.rs perf.rs
  tests/ ticks format limits layout colors theme data_inputs hist threading svg_snapshots png_render ui(trybuild) window_smoke
  tools/ gen_fixtures.jl gen_layout_fixtures.jl gen_colormaps.jl makie_gallery.jl   (local Julia + installed Makie only)
```

Dependencies use the versions from the Sep-2026 research. Confirm them with `cargo add` in M0 and adjust if resolution differs. Edition 2024. The first build needs network access.
- **GPU and window:**
  - `wgpu 30.0.1`: default features off; `std`, `parking_lot`, `wgsl`, `metal`, `vulkan`, `dx12` on.
  - `winit 0.30.13` behind the default-on `window` feature.
  - `pollster 1`.
  - macOS only: `objc2` + `objc2-quartz-core` + `objc2-core-graphics` for the colour shim.
- **Core:** `bytemuck 1.25 (derive)`, `parking_lot 0.12`, `png 0.18`, `ab_glyph 0.2.32`, `base64 0.23`, `log 0.4`.
- **Optional:** `ndarray 0.17`; `resvg 0.48` behind the default-on `cpu-png` feature.
- **Dev:** `rand 0.9`, `serde_json`, `trybuild`.

## 6. Milestones (each ends with a runnable check; the live heatmap arrives early because it is the core motivation)

| # | Milestone | Done when |
|---|---|---|
| M0 | Scaffold: `git init`, `cargo init --lib`, `rust-toolchain.toml`; copy docs/research and the fonts + licence in; resolve dependencies. | `cargo build` succeeds. |
| M1 | **End-to-end skeleton:** GPU context, `mesh` + `sprite` (circle), handles/lock/notify, Figure/Axis/Scatter, naive limits, fixed layout, DrawList, offscreen PNG, `show()` window. | 1. `s1_scatter` writes a 1200×900 PNG that I inspect.<br>2. `s1_show` opens a crisp HiDPI window on the M3.<br>3. #0072B2 reads (0, 114, 178) in Digital Color Meter set to "Display in sRGB".<br>4. `show()` off the main thread → `Err(NotMainThread)`.<br>5. **Perf smoke: a 1M-point scatter pans with 0 data bytes uploaded.** |
| M2 | Core API: data traits, the attribute-table macro for Axis/Lines/Scatter/Heatmap, plot macros, `fig.at(..).plot()`, handles, `batch`, cycling, themes (global + scoped), errors. Other attribute tables arrive with the milestone that renders them. | S1–S3 compile. trybuild pins the errors for an unknown key, a comma instead of `;`, `fig.at(1, 1..2)`, index 0, and a non-plottable input. An 8-thread `set_data` hammer test has no deadlock. |
| M3 | Axis math: Wilkinson, formatting, log/minor ticks, limits, links, Float32 rebase. | Exact match against ≥ 1000 fixture cases generated from the local PlotUtils/Makie (`tools/gen_fixtures.jl`). |
| M4 | Text + layout: fonts, rich text, atlas, GridLayout port, protrusions, sizing of Legend/Colorbar/Label. | Font metrics 947/−218/1000. The default Axis viewport is (74, 59) 510×355. 15 layout fixtures match Makie's `debug_layout` within 0.01 unit. |
| M5 | **Fields + live minimum:** `field` pipeline, colormaps, Colorbar, all Data2D inputs + `Edges`, `show_live` + `set_data` + wake, scroll zoom, right-drag pan, heatmap hover. | 1. S4's heatmap part renders.<br>2. `heatmap(0..=1, 0..=1, 4×3)` gives limits (−1/6, 7/6) × (−0.25, 1.25), matching a Makie fixture.<br>3. The orientation test lights the same cell for every input form (C- and F-order ndarray included).<br>4. **A live Gray–Scott heatmap runs in a window with pan/zoom/hover.** |
| M6 | `line` pipeline (segments → joins → round → dashes), axislegend/Legend, value-coloured scatter/lines, full SVG backend + CPU PNG fallback. | 1. The line torture page shows no double-blended joints.<br>2. The S2/S3 SVGs are valid.<br>3. resvg(SVG) vs GPU PNG mean diff < 1.5 %.<br>4. Full S5 with the diagnostic line runs.<br>5. `save("x.png")` works with the GPU disabled. |
| M7 | hist, barplot (dodge/stack/categorical), band, h/v/ablines, text, log axes. | S6/S7 render. Hist edges and normalisation match StatsBase fixtures. Log majors sit on decades with 2..9 minors. |
| M8 | Interaction polish: rect zoom, Ctrl-click reset, key locks, trackpad extras, full inspector, tick-label jitter guard. | Scripted interaction tests match Makie's formulas. The inspector string is asserted on linear, log and reversed axes. Idle CPU ≈ 0 %. |
| M9 | Live polish: buffer recycling, chunked `push`, `follow`, `display/pump`, `frame_due`/`wait_frame(_timeout)`, device-loss recovery. | 1. S5 runs at display rate with the sim at ≥ 95 % of headless throughput.<br>2. The window minimised during S5 doesn't hang the worker.<br>3. PNG export from the worker while displaying works.<br>4. Closing → `Ok`; a worker panic → `Err(WorkerPanicked)`. |
| M10 | Themes, `Save::dpi`, the S8 paper figure, perf pass, docs, README gallery. | 1. S8: a 1200×900 PNG with pHYs 11811 px/m, and an SVG of 288×216 pt.<br>2. Theme snapshots match the CairoMakie references.<br>3. The perf targets are met.<br>4. Every public item has a doc example. |

Execution:
- Milestones run in order, and I make one git commit at the end of each. Tell me at approval if you'd rather commit yourself.
- Inside a milestone, independent modules can be built in parallel by subagents in worktrees, for example ticks vs colormaps vs font metrics.
- After each milestone, a review pass checks correctness and fidelity to Makie, including looking at the rendered PNGs.

## 7. Verification
- **Unit tests (no GPU):**
  - ticks, formatting and minor ticks against Julia fixtures (exact match);
  - limits edge cases, interval/`Edges` semantics, hist/bar math;
  - colour parsing, Wong hex values, viridis endpoints;
  - units (`12.0*PT == 16.0`), theme precedence (global vs scoped), cycle freeze;
  - Data2D equivalence, text metrics;
  - the layout solver, including fuzzing with empty grids and conflicting Aspect sizes.
- **Julia fixtures:** `tools/*.jl` runs with the locally installed Makie/CairoMakie/PlotUtils/StatsBase. The JSON output is committed, so `cargo test` never needs Julia.
- **SVG snapshots:** byte-for-byte for S2–S4 and S6–S8. The data is seeded with xorshift and polynomials instead of libm, so it is stable across OSes. `SCIPLOT_BLESS=1` updates the snapshots.
- **Gallery and side-by-side comparison:**
  - `cargo run --release --example gallery` → `target/gallery/*.png|svg`. It covers S1–S8 plus stress pages: line torture, all markers, NaN gaps, a 1e9-offset axis, a log-axis heatmap, and every theme.
  - `tools/makie_gallery.jl` renders the same data with CairoMakie.
  - `examples/compare.rs` builds an sciplot | Makie | diff contact sheet.
  - I read the PNGs directly at each milestone to check the visuals.
- **Cross-backend:** mean diff < 1.5 % per channel between the resvg-rasterised SVG and the GPU PNG. The same path runs in CI without a GPU.
- **Window:**
  - `tests/window_smoke.rs` (`harness = false`, feature `testing`, gated by `SCIPLOT_WINDOW_TESTS=1`) injects scroll, drag, rect-zoom, Ctrl-click, hover and minimise at scale factors 1 and 2. It asserts the final limits and the tooltip text.
  - `SCIPLOT_AUTOCLOSE=3` smoke-runs the windowed examples.
  - You run a short manual checklist on the M3: trackpad feel, colour, resize, moving between displays.
- **Perf (`examples/perf.rs`, release build):**
  - 1M-point scatter pan: p99 < 8.3 ms with 0 data uploaded;
  - 1M-point line pan: < 8.3 ms;
  - 2048² heatmap pan: < 3 ms GPU; its `set_data`: < 10 ms;
  - S1 PNG export: < 150 ms warm;
  - idle: ≈ 0 % CPU.

---

## 8. Update (2026-09-26): WASM visualization of dynamic systems is a major focus

The base stays a usable library for publication-quality science/biology figures. On top of it,
interactive **in-browser dynamic systems** become a core target. User decisions:
- **Browsers: WebGPU with WebGL2 fallback.** Pipelines must not use storage buffers: feed shaders
  through vertex/instance buffers (lines bind one buffer at several offsets for p0..p3) and data
  textures (`R32Float` + `textureLoad`) instead. Pipelines are created per target format (the web
  canvas may be `Rgba8Unorm` or `Bgra8Unorm`); keep non-sRGB targets.
- **Web model: the simulation runs in wasm first; standalone HTML export after.** One wasm module holds the
  Rust simulation and the figure, mounted in a canvas, driven by a per-frame callback. Later:
  `fig.save("fig.html")` for self-contained interactive figures (pan/zoom/hover) for supplements.
- **Dynamic-systems features (all wanted):** parameter widgets (Makie-style Slider/SliderGrid/Toggle/
  Button blocks drawn by sciplot itself, with `on_change` callbacks), vector fields (arrows, streamplot),
  contour/contourf (nullclines, level sets), 3D (Axis3 with lines3d/scatter3d/surface, orbit camera).

Architecture rules that follow:
- Core (figure, scene, layout, text, ticks, SVG) stays platform-independent: no threads, no
  `std::time::Instant` (use `web-time`), no blocking; GPU init has an async path (wasm) and a sync
  wrapper (native); file-path `save` gets byte-returning siblings (`to_png_bytes`, `to_svg_string`).
- **Portable animation API:** `fig.animate(|frame| { ... })` runs a callback every display frame on
  the event-loop thread (native main thread / browser `requestAnimationFrame` via winit), with `t`,
  `dt`, frame count and stop control. Widget callbacks run on the same thread. The native-only
  `show_live` (simulation on a worker thread) stays for heavy simulations.
- **Web entry:** on wasm, `fig.show()` mounts into a canvas (by id, or appended to the body) and
  returns immediately (`EventLoop::spawn_app`). A demo lives in `examples/web/` (Lorenz and
  Gray–Scott with sliders). It is verified by building for `wasm32-unknown-unknown` and rendering the
  page in headless Chrome with WebGPU enabled and with WebGL2 forced; this captures the page, never
  the desktop.

Revised order after wave 1 merges:
1. **Portability pass:** no storage buffers, per-format pipelines, `web-time`, async GPU init, byte
   exports; the `wasm32` build compiles. Also finish the 2D core: Axis tick attributes, Legend,
   Colorbar, Label, h/v/ablines, and the `animate` API on native.
2. **Web backend:** canvas mount, input events, HiDPI, the demo, and headless-browser checks
   (WebGPU and WebGL2).
3. **Dynamic systems 2D:** widgets (Slider/SliderGrid/Toggle/Button), arrows + streamplot,
   contour/contourf.
4. **3D:** Axis3, depth buffer, orbit camera, lines3d/scatter3d/surface/mesh, 3D ticks.
5. **Standalone interactive HTML export.**
6. The remaining original polish milestones (M8–M10: interaction polish, live polish, themes/perf/docs).

### 8.1 Decisions made during implementation (2026-09-26)
- Marker strokes follow **CairoMakie** (stroke centered on the outline), not GLMakie (stroke outside),
  so PNG/SVG exports match the publication-quality reference.
- Linked axes share one set of limits: the union of their data, with the largest margin of the
  non-tight members (tight only if all members are tight). This matches what Makie shows.
- Web toolchain is repo-local: `wasm-bindgen-cli` 0.2.129 installed into `.tools/` (gitignored).
- **Axis3** is a port of Makie 0.24's `axis3d.jl` (camera `calculate_matrices`, viewmode `:fitzoom`
  default, protrusions 30, decorations on the far panels / viewer-facing edges), checked against
  `tests/fixtures/axis3.json`. 3D plots (`lines`, `scatter`, `surface` on an `Axis3`) are GPU
  primitives in their own render passes with a multisampled depth buffer: surfaces write depth,
  lines and markers only test against it (they are hidden behind surfaces but draw over each
  other in plot order, as in CairoMakie). Surfaces use Makie's default FastShading. The SVG
  backend (and the CPU PNG path) sorts triangles, segments and markers back to front instead.
