**Critique of the final sciplot design: prioritized issues and concrete fixes**

I checked two claims directly:
- **Makie heatmap intervals.** `~/.julia/packages/Makie/Iy6pu/src/conversions.jl:419-438` shows that Makie does not treat a `0..1` interval as outer cell edges. The design says it does.
- **wgpu TRANSIENT_ATTACHMENT.** The docs.rs page for wgpu 30.0.1 confirms `TextureUsages::TRANSIENT_ATTACHMENT` exists. It needs Clear or DontCare on load and `StoreOp::Discard` on store, and it is a no-op where the platform doesn't support it.

I also confirmed these in the local sources:
- `update_state_before_display!` exists (`figureplotting.jl:413`).
- `Legend(pos, [ax1, ax2]; unique)` accepts an array of axes (`legend.jl:1015-1020`), and `unique` groups by (plot type, label).
- `hide*decorations!` takes the keywords label, ticklabels, ticks, grid, minorgrid and minorticks (`axis.jl:1122-1158`).

---

## P0: must change before implementation planning

**P0-1. The heatmap interval meaning is attributed to Makie, but Makie does something else (affects S4 and S5).**
- In `conversions.jl:419-438` (`CellGrid`, `EndPointsLike`), Makie takes the interval `a..b` or the tuple `(a, b)` as the centres of the first and last cells. It then widens by half a step: the edges become `a − Δ/2 .. b + Δ/2`, with `Δ = (b − a)/(n − 1)`.
- Only an explicit `Makie.EndPoints` value is used as outer edges without change.
- The design says "f64 `a..=b` = OUTER CELL EDGES (Makie `0..1` interval)". A Makie user porting `heatmap(0..Lx, 0..Ly, n)` would get a grid shifted by half a cell and scaled slightly, and nothing would warn them.
- **Fix:**
  - Make `a..=b` follow Makie: first and last cell centres.
  - Add `Edges(a, b)` (the equivalent of Makie's `EndPoints`) for outer edges. This is what a finite-volume simulation needs.
  - Rewrite S4 and S5 as `heatmap!(ax, Edges(0.0, lx), Edges(0.0, ly), Field::new(..))`.
  - If you would rather keep edge semantics for `..=`, list it as a deviation (D13) and remove the claim that Makie does this.
  - Either way, add a golden layout/limits fixture: `heatmap(0..1, 0..1, rand(4, 3))` should give limits `(-1/6, 7/6) × (-0.25, 1.25)`.

**P0-2. The storage model for live data contradicts itself (the Float32Convert rebase, pools, `push`, and hover).** This is the core of S5 and of the 1M-point target.
- **(a) Axis transforms aren't known when the setter runs.** Rule 3 converts f64→f32 on the caller's thread, but §3.3 says the buffer holds `f32((T(x) − origin)·scale)`. The per-axis `origin` and `scale` are only known at render time, and so is `T`, since the user can change `xscale` after plotting. A rebase also "re-uploads that axis's plots", which needs the original f64 data, but only `DrawList.cpu: Arc<[[f64;2]]>` implies that copy exists.
  - **Fix:** plot positions are stored as the f64 master (`Arc<Vec<[f64;2]>>`, 16 MB per 1M points). The render thread does transform, rebase and f32 conversion at upload time, which takes about 1–2 ms per 1M points and happens only on `data_rev` or rebase changes.
  - Values (heatmap z, colour values) can still be converted on the caller's thread. Store them as `(v − off)·k` with `off`/`k` taken from the extrema in f64 (see P2-11).
- **(b) The pool can't be reused as specified.** `Arc<[f32]>` cannot go back into a pool without a copy, because `Arc::from(Vec)` reallocates and there is no way back to a `Vec`.
  - **Fix:** use `Arc<Vec<T>>`, and recycle through `Arc::try_unwrap` once the snapshot and the DrawList memo have dropped their clones.
- **(c) `push` under the lock is O(n).** Appending to a shared `Arc` needs `make_mut`, which clones the whole buffer whenever a snapshot still holds a reference. That happens every frame. It breaks the "O(1) under lock" rule and the lock-hold < 50 µs target.
  - **Fix:** use chunked, append-only storage, such as `Vec<Arc<[f64; CHUNK]>>` plus a tail `Vec`. The snapshot clones the chunk list and the tail length. A tail upload reads the new chunks only.
- **(d) Pooling conflicts with hover.** The design says "Big CPU buffers go back to the plot's pool after upload", but the heatmap inspector reads "the CPU f32 mirror".
  - **Fix:** the latest buffer is the mirror and is never pooled. Only the buffer it replaced is recycled. State this invariant explicitly.

---

## P1: will compile wrongly, surprise a Makie user, or fail in the field

**P1-1. The `Scalar` impls don't compile.** `trait Scalar: Copy + Send + Sync + 'static` combined with `impl<'a, T: Scalar> Scalar for &'a T` is rejected, because `&'a T` is not `'static`. That breaks the `slice::Iter`, `Take<slice::Iter>` and `Rev<slice::Iter>` impls of `Data1D`.
- **Fix:** drop the `Send + Sync + 'static` supertraits. Values are copied into f64 or f32 immediately, so nothing needs them.
- Add a doctest for `lines!(ax, t.iter(), t.iter().rev())`.

**P1-2. Makie's `lines(f[1, 2], x, y)` has no equivalent.** Creating an Axis and a plot at a grid position in one call is one of the most common Makie idioms. The design only has "new Figure" one-liners, and `lines!(fig.at(..), ..)` panics when there is no Axis.
- **Fix:** add non-mutating methods on `GridPosition`: `fig.at(1, 2).lines(x, y)`, `.scatter`, `.heatmap_xy` and so on. Each creates the Axis and returns the plot handle, and `.axis()` on that handle gives the new Axis.
- Keyword form: `kw!(fig.at(1, 2).lines(&x, &y); color = RED)`.

**P1-3. Index 0 silently prepends a row or column (Rust footgun).** Allowing `usize` in `fig.at(i, j)` means that `for (i, ..) in enumerate()` calls `fig.at(0, 0)`. That prepends a row and a column instead of failing.
- **Fix:** 0 and negative values panic with `#[track_caller]` and the message "grid positions are 1-based". Prepending becomes explicit, e.g. `fig.at(Prepend, ..)` or `fig.layout().insert_row(1)`, and `Label!(fig.at(Prepend, ..), "Title")` handles the super-title case.
- Apply the same validation to `colsize`, `rowsize` and `*gap_at`.

**P1-4. `set_theme` is thread-local, but Makie's `set_theme!` is global.** A figure created in a `show_live` worker, a rayon job or a test thread would silently ignore `set_theme(paper)`.
- **Fix:**
  - `set_theme`, `update_theme` and `reset_theme` write a process-global `RwLock<Theme>`.
  - `with_theme` pushes a thread-local override on top of it.
  - `Figure::new` resolves the thread-local override first, then the global theme.

**P1-5. No PNG without a GPU.** Simulation users often run on cluster nodes, in Linux CI, or over ssh with no Vulkan driver. In those places `save("x.png")` returns `Err(NoGpuAdapter)`, whereas CairoMakie works everywhere.
- **Fix:** add a CPU fallback. Render the SVG backend's output through `resvg`/`tiny-skia`, moving `resvg` from dev-dependencies to an optional, default-on `cpu-png` feature. Use it automatically when no adapter is found, and log it once.
- The SVG backend is already the CPU reference, so the extra cost is small. The same path makes `backend_consistency` testable in CI.
- Optionally try `force_fallback_adapter` (lavapipe) first.

**P1-6. `wait_frame` and `frame_due` can hang forever.**
- If the window is minimized, occluded (`CurrentSurfaceTexture::Occluded`/`Timeout`) or on another Space, `rendered_rev` never advances. A worker blocked in `wait_frame()` then hangs.
- **Fix:**
  - `wait_frame` returns immediately when the screen is not visible or closed.
  - Add `wait_frame_timeout(Duration) -> bool`.
  - The renderer marks `rendered_rev = published_rev` whenever it skips a frame because the surface is occluded.
  - Add a `window_smoke` case that minimizes the window during S5.

**P1-7. GPU state is shared across threads, but the design never says how.**
- `render_rgba`/`save` can run from the worker ("movie frames from a sim") while the main thread renders the window. Both use the global `Gpu`.
- The glyph atlas has LRU eviction and re-rasterization, and the pipeline cache is keyed by format. Neither has a stated ownership or locking model.
- **Fix:**
  - Each render context (each Screen, and each export call) owns its own atlas and per-plot GPU caches.
  - Only `Device`, `Queue`, pipelines and colormap LUTs are shared, behind `OnceLock` or `Mutex`.
  - Add a test that exports PNGs in a loop from the worker while S5 is displayed.

**P1-8. Milestone order puts the user's main motivation last.** Live fields with pan and zoom (S5) are the reason sciplot exists, yet interaction arrives in M8 and live updates in M9. M2 ("full API surface: every attribute table, every macro, themes, trybuild for everything") is also too large, and it will churn as M3–M7 reveal what is actually needed.
- **Fix:**
  - Split M2:
    - **M2a:** data traits, the attribute-table macro for Axis, Lines, Scatter and Heatmap, the plot macros, handles and `batch`.
    - **M2b:** the rest of the attribute tables and macros, added in the milestone that first renders each feature.
  - Pull a "live minimum" into the end of M6: `show_live` + `set_data` + wake + redraw, basic scroll zoom and right-drag pan, heatmap hover, and a perf smoke test.
  - M8 and M9 then only polish (rect zoom, jitter guard, pump mode, `frame_due`, pools).
  - Run the 1M-scatter pan perf smoke test at M1 or M2, not at M10. It is the test that validates the snapshot/lock architecture.

**P1-9. Layout jitter in S5.**
- `ax_d.follow(true)` changes the y-tick label widths as data grows. That changes the column gap `lefts[3] + rights[2]`, so `remaining_w` changes, the `Relative(0.35)` column resizes and the centring shift moves. The heatmap then shifts sideways every few frames.
- An automatic colorrange on a live heatmap causes the same problem through the Colorbar tick labels.
- **Fix:**
  - Expose `yticklabelspace`/`xticklabelspace` (Makie's attributes) explicitly and use `yticklabelspace = 50` in S5.
  - Optionally add an sciplot live mode in which ticklabelspace only grows (monotone), with a doc note.

**P1-10. The macOS colorspace shim must survive reconfiguration.**
- Walking the NSView's sublayers is fragile, and `surface.configure` on resize may reset the layer.
- **Fix:**
  - Get the layer through `surface.as_hal::<wgpu::hal::api::Metal, _>` (no view walk). Reapply after every `configure`.
  - For M1 check (3), the Digital Color Meter must be set to "Display in sRGB". In "native values" mode, #0072B2 never reads (0, 114, 178) on a P3 panel, even when everything is correct.
- Use `objc2::MainThreadMarker::new()` for the macOS main-thread check. It is safe, so `mainthread.rs` no longer needs `unsafe`.

---

## P2: quality, polish, and cuts for a minimalist v1

1. **Over-engineered for v1, cut or defer these:**
   - rotated-glyph rasterization with 1° angle bins: draw an upright atlas glyph on a rotated quad with linear sampling, which research §5 already said is acceptable;
   - tiled render above 16384 px, plus the 20000×15000 test (a 1.2 GB RGBA buffer): return an error that suggests a lower dpi;
   - storage row bands: they are also wrong for y-fastest (C-order ndarray) strides, where bands would have to be x-columns;
   - `rasterize(true)` for SVG, which needs the GPU inside the SVG path;
   - the inspector for bars and bands;
   - `LinearTicks` and `MultiplesTicks`;
   - the nalgebra feature;
   - the `inspector_label` closure.
   Keep irregular and log-axis heatmaps.
2. **Pipeline formats.** Surface `Bgra8Unorm` and offscreen `Rgba8Unorm` mean every pipeline is compiled twice. Render the offscreen target as `Bgra8Unorm` too and swizzle on readback, so there is one pipeline set.
3. **Keyword typo help.** Makie users will type `lines!(ax, x, y, color = RED)` with a comma. Add a final macro arm, `($t:expr, $($rest:tt)*) => compile_error!("use `;` before keyword arguments: lines!(ax, x, y; color = RED)")`, and pin it with trybuild.
4. **Pin inference-sensitive literals in doctests:** `(BLUE, 0.5)` (float var → f32 through `IntoColorSpec`), `markersize = 7`, `linewidth = 2`, `padding = (0, 5, 5, 0)`, `fig.at(1, 1)` (i32/usize fallback) and `(4.0*INCH, 3.0*INCH)`. Add `rust-toolchain.toml` so the trybuild `.stderr` files don't churn with rustc updates.
5. **`#[must_use]` on the non-mutating free functions only** (`scatter(..)` etc.), with the message "creates a Figure; call .save() or .show()". Setters stay unmarked. Without it, `scatter(&x, &y);` silently does nothing.
6. **`show()` alongside `display()`.** Define `show()` as "returns when the windows opened by this call close", while existing pumped screens keep rendering. There is one persistent `App` in the thread-local, and `display()` + `show()` together gets a test.
7. **`show_live` edge cases:**
   - A worker that never polls `is_open()` keeps the process alive after the window closes. Document this.
   - A worker panic surfaces only when the window closes. Also log it immediately and show "(simulation panicked)" in the window title.
8. **Torn frames outside `batch` on a displayed figure.** A keyword chain applied to a figure that is already shown (e.g. `heatmap!(..; colormap, colorrange)` after `display()`) can present one intermediate frame. Either auto-batch the setters in each `Axis!`/`heatmap!` macro expansion (wrap it in `fig.batch`) or document it.
9. **SVG heatmaps.** `image-rendering="pixelated"` is not honoured by every consumer (some Inkscape and LaTeX→PDF paths smooth the image).
   - Also write `style="image-rendering:optimizeSpeed"`, and upsample small fields by an integer factor (so each cell is at least 4 px) before embedding.
   - Check that resvg nearest-samples the image in `backend_consistency`.
10. **Log minors, D2.** "When majors skip decades, only the skipped decades" is ambiguous. Specify it: minors go at 10ⁿ for the skipped n (matplotlib-like). Add fixtures for 1e-3..1e9.
11. **Value range in f32.** Densities up to 1e21 m⁻³ are fine in f32, but astrophysical 1e40 or cross-sections of 1e-40 overflow or go subnormal. Store values as `(v − off)·k` in f32 with the colorrange mapped the same way (as in P0-2a). Makie clamps at floatmax.
12. **Verification additions:**
    - gate `window_smoke` behind `SCIPLOT_WINDOW_TESTS=1`;
    - inject scale factors 1.0 and 2.0 under the `testing` feature;
    - inspector tests on log and reversed axes;
    - a Makie golden fixture for the interval semantics (P0-1);
    - a C-order vs F-order ndarray test for the orientation plus the row-band path;
    - fuzz the layout solver with empty grids, NaN sizes and conflicting Aspect settings;
    - generate seeded SVG-snapshot data without libm transcendentals (use polynomials or xorshift) so snapshots are byte-stable across OSes.
13. **Cycling in S3.** Lines and scatter have separate per-function counters, so "model" and "measurement" are both Wong blue, exactly as in Makie. Give the scatter an explicit `color = WONG[1]` so the shared legend reads clearly, or note the behaviour in the docs.
14. **Device loss.** `static GPU: OnceLock<Result<..>>` can never recover. On `DeviceLost` (for example after an eGPU unplug), rebuild the device and drop all GPU caches.
15. **Index-base consistency.** Grid and `Cycled` are 1-based, while inspector `[i, j]` and `Field` are 0-based. That is defensible, but `colsize(col: i32)` should take `impl IntoSpan`-style validation so every grid-indexing method goes through one 1-based checked path with the same error text.

---

## Coverage check against S1–S8 and the user's decisions

- **Covered:** every decision and every scenario has code.
- **Semantic gaps:**
  - S4/S5 interval edges (P0-1);
  - S5 jitter (P1-9);
  - S5 hover on pooled data (P0-2d);
  - PNG export on machines without a GPU (P1-5).
- **Compiles as written:** assuming P1-1 is fixed, I traced the S1–S8 code through the macro arms, the `__target(&$t)` auto-ref (`&&Axis` → `AsAxis for &Axis`), `LegendSource for [&Axis; N]`, the integer-literal fallbacks and the `show_live` borrow and `Send` bounds. It compiles in principle; pin it with P2-4.