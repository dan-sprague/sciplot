# ezviz architecture (for contributors)

Read `docs/PLAN.md` first (it wins over `docs/DESIGN.md` where they disagree). Makie reference
sources are local: Makie 0.24.14 `~/.julia/packages/Makie/Iy6pu`, GLMakie 0.13.14
`~/.julia/packages/GLMakie/hxEgI`, CairoMakie 0.15.14 `~/.julia/packages/CairoMakie/uIOIH`,
GridLayoutBase 0.11.3 `~/.julia/packages/GridLayoutBase/K7sJt`, PlotUtils 1.5.0
`~/.julia/packages/PlotUtils/J9gzB`. Research notes with citations: `docs/research/*.md`.

Julia for reference fixtures: `julia --project=tools script.jl` (offline env with CairoMakie, Makie,
PlotUtils, StatsBase, ColorSchemes, JSON, GridLayoutBase; ~5 s to load CairoMakie). Never add packages
or go online. Commit generated JSON under `tests/fixtures/` so `cargo test` never needs Julia.

## Data flow

```
handles (Figure, Axis, Scatter, ...)  -- Arc<FigShared> + id, Clone + Send + Sync
   | setter: convert input off-lock, then FigShared::update(dirty, |st| ...) (short lock, bumps rev, wakes windows)
   v
FigState (src/figure/mod.rs): theme, grid spec, blocks arena, plots arena  -- Clone = cheap snapshot
   | scene::build(&snapshot, size_override, &mut SceneCache)            (src/scene/mod.rs)
   v
resolve attrs -> scene::axis::compute_limits -> ticks::major_ticks -> scene::axis::protrusion
   -> layout::solve -> scene::axis::emit_decorations + scene::plots::emit_plots (PlotImpl::emit)
   v
DrawList (src/scene/drawlist.rs): items sorted by (z, seq); each Item = {z, clip, space, prim}
   |-> render::gpu::Renderer (window surface or offscreen PNG)   src/render/gpu/
   |-> render::svg (M6)                                            src/render/svg/
```

Coordinates: `Space::Figure` = figure units (1 unit = 1 CSS px), origin top-left, y down.
`Space::Data(i)` = axis `i`'s f32 *local* coordinates: `local = (scale(x) - origin) * k`
(`transform::Rebase`, Makie's Float32Convert idea). The GPU maps local -> device px with the per-axis
affine `AxisXform::affine(ppu)` computed in f64, so pan/zoom never re-uploads plot data.

## Conventions

- Attributes: declare with `attrs::attributes!` (Makie names + Makie defaults, each with a dirty
  class). It generates `XAttrs` (Option fields), `XResolved`, handle setters `fn a(&self, v: impl
  Conv<T>) -> Self`, and a theme builder `XTheme`. Add conversions by implementing `attrs::Conv<T>`.
- Setters never panic except on programmer errors at the call site (`#[track_caller]` asserts, e.g.
  length mismatch). Environment failures return `ezviz::Result`.
- Grid positions are 1-based inclusive (Makie); data indices are 0-based.
- Colors are sRGB-encoded, straight alpha (`Color`); GPU colors are premultiplied
  (`Color::to_premul_u32`, `frame::premul`). All blending happens on sRGB-encoded values (non-sRGB
  `Bgra8Unorm` targets), like Cairo/GLMakie.
- Draw order (z): axis background -100, grid -10, plots 0 (+ plot z), ticks 10, spines 20, text 30.

## Extension points

**A new plot type** (`src/plots/<name>.rs`):
1. State struct holding data (`Arc<Vec<...>>`, f64 master copies) + `<Name>Attrs` from `attributes!`.
2. Handle struct `{ sh: Arc<FigShared>, id: PlotId }` + `plot_common!(Handle)` (label, visible, save,
   show, figure(), axis(), unpack(), delete()...).
3. `impl PlotImpl for State` (`src/plots/mod.rs`): `cycle_group`, `color_is_auto`, `data_bounds`
   (scaled space), `tight_limits`, `emit(ctx: &mut scene::PlotCtx)`.
   `PlotCtx` gives `local_points(part, &pts)` (memoized conversion + cacheable buffer),
   `data_buf(part, Arc<Vec<T>>)`, `solid_color(spec, patch)`, `per_point(..)`, `push_data(prim)`.
4. Register: one line in `plot_kinds!{ ... }` in `src/plots/mod.rs`; theme defaults field in
   `theme::Theme` (+ its `merge`) if the plot has attributes; methods on `Axis` (mutating) and
   `GridPosition` (new Axis + plot); a `#[must_use]` free function (new Figure); a `name!` macro in
   `src/macros.rs`; re-exports in `src/lib.rs` and `src/prelude.rs`.

**A new block type** (`src/blocks/<name>.rs`, e.g. Label, Legend, Colorbar):
1. State struct + `<Name>Attrs` via `attributes!`; handle `{ sh, id: BlockId }` + `block_common!(Handle,
   Variant, State)`; constructor `Handle::new(pos: GridPosition, ...)` that resolves the placement under
   the lock (`pos.resolve(st)`) and calls `st.add_block(place, Block::Variant(Box::new(state)))`.
2. `impl BlockImpl for State` (`src/blocks/mod.rs`): `layout(&BlockCtx) -> BlockLayout` (protrusions,
   `BlockSize` width/height, autosize, tellwidth/tellheight, halign/valign) and
   `emit(&BlockCtx, &mut Emitter, rect)`. `BlockCtx` gives the figure snapshot, globals, and (in emit)
   every `AxisFrame` (final rects, limits, ticks). Blocks drawn over an axis (axislegend) return
   `Some(axis_id)` from `inside_axis()` and get the axis rect in `emit`.
3. Register: one line in `block_kinds!{..}`; theme field + `Theme::<name>(|t| ..)` builder + merge line;
   a `Name!` macro in `src/macros.rs`; re-exports.
**Axis3** (`src/blocks/axis3.rs`, lowered by `src/scene/axis3/`) is a block whose `emit` is done by
`scene::axis3::emit` (it needs the `SceneCache`). 3D plot types implement `PlotImpl` (colormaps,
cycling) plus `scene::axis3::Plot3dImpl` (`bounds`, `emit3`) and register in `plot3d()`; they emit
`Prim::Lines3d` / `Markers3d` / `Mesh3d` carrying an `Arc<View3d>` (camera, clip box, lights). The
GPU draws each Axis3's 3D items in a depth-tested pass (`pipelines/view3d.rs`); `render/svg/three_d.rs`
depth-sorts them.
The layout port (src/layout) is GridLayoutBase-exact: see `LayoutItem`, `BlockSize`, `AlignMode`.

**A new GPU pipeline** (`src/render/gpu/pipelines/<name>.rs` + `<name>.wgsl`):
`SHADER`, `layout(device) -> BindGroupLayout`, `pipeline(device, &Layouts, &ShaderModule, format) ->
RenderPipeline` and `prepare(&mut Frame, &Prim..., xform) -> Option<DrawCmd>`. Everything must run on
WebGPU **and WebGL2**: no storage buffers/textures or compute; per-element data comes in through
instance-step vertex buffers (<= 8 buffers, 16 attributes; bind one buffer at several offsets to read
neighbours, as `line` does), big arrays through `R32Float` textures + `textureLoad` (unfilterable;
tile when larger than `max_texture_dimension_2d`, as `field` does). `Frame` (src/render/gpu/frame.rs)
provides `vertex(&Buf)` / `points(&Buf, append, closed)` / `cached(key, rev, ..)` /
`data_texture(..)` (cached by `BufKey` + a `tag`), `transient(bytes, usage)`, `push_uniform(&T) -> dyn
offset` (<= 256 B blocks), `uniform_binding::<T>()`, `lut(&Arc<Vec<Color>>)`, `dummy_tex()`, `ppu`,
`size`. WGSL files get `common.wgsl` prepended (Globals at group 0: `g.target_px`, `g.ppu`,
`lin_samp`; `px_to_clip`, `finite_bits`, `nan_bits`, `CMap`, `cmap_lookup`, `premul`). Pipelines are
built per target format (`Gpu::pipelines(format)`; canvases may be RGBA or BGRA). Register: one field
in `Layouts` and `Pipelines` (+ constructors and `sources()`), one match arm in `Renderer::render_to`
(`src/render/gpu/renderer.rs`). Integer varyings need `@interpolate(flat)`. `tests/portability.rs`
renders on a device with WebGL2 limits and translates every shader to GLSL ES 3.00; keep it green.

## Verifying

- `cargo test` (unit + integration). GPU tests must return early on `Error::NoGpuAdapter`.
- Render PNGs into `out/` (gitignored) and look at them (the Read tool shows images). Compare against
  CairoMakie renders of the same data (`julia --project=tools`), written next to them.
- Windows: `EZVIZ_AUTOCLOSE=<secs>` closes windows automatically; `EZVIZ_WINDOW_DUMP=out/x.png` writes
  the first presented window frame. **Never take desktop screenshots.**
- `ezviz::testing::Offscreen` renders headless frames with upload stats (`RenderStats`).
- Browser (`src/window/web.rs`; the app core `src/window/app.rs` is shared with native windows):
  `tools/web/build.sh <example>...` builds wasm examples into `examples/web/pkg/` (wasm-bindgen CLI
  0.2.129 in `.tools/`); `tools/web/check.sh [--no-build]` serves `examples/web/`, captures every
  page in headless Chrome on WebGPU and WebGL2 (`tools/web/cdp_shot.mjs`), pixel-diffs the static
  page against the native PNG (`tools/web/pngdiff.mjs`) and replays scripted input
  (`tools/web/input_actions.json`). Pages set `document.title = "done:<backend>"` when rendered
  (`examples/web/harness.js`).
