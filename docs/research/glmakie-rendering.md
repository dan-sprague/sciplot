# GLMakie rendering and a wgpu/WGSL port plan for ezviz

Every claim about GLMakie or Makie below comes from the local sources. Paths are under `~/.julia/packages/GLMakie/hxEgI` (GLMakie) and `~/.julia/packages/Makie/Iy6pu/src` (Makie). A few wgpu and winit facts come from web lookups, listed under Sources at the end. No files were created or changed.

## 0. Recommendations in brief

- **Coordinate frame:** do all 2D work in framebuffer physical pixels (origin top-left, y down, the same frame as `@builtin(position)`).
  - Each plot gets one f64-computed affine on the CPU, uploaded as f32: `fb_px = local*s + t`.
  - The viewport is always the full target. Axis clipping is done with `set_scissor_rect` only.
- **Render pass:** one render pass per frame, 4x MSAA, color target `TRANSIENT_ATTACHMENT` (Apple memoryless), resolved into the surface or an offscreen Rgba8Unorm texture. No depth buffer: draw in painter's order sorted by (z, insertion), as GLMakie does.
- **Anti-aliasing:** lines, markers and text use analytic SDF alpha AA (GLMakie's method). Fills (bars, bands, polys, backgrounds) are plain triangles and rely on MSAA. No FXAA.
- **Color handling:**
  - All color math stays in sRGB-encoded space: non-sRGB `Bgra8Unorm` surface, `Rgba8Unorm` offscreen, non-sRGB colormap and atlas textures. This matches GLMakie's RGBA8 FBO and Cairo.
  - Use premultiplied alpha with `BlendState::PREMULTIPLIED_ALPHA_BLENDING`.
  - Set the CAMetalLayer colorspace to sRGB yourself (see pitfalls).
- **Pipelines:** `line` (strip and segment modes), `marker`, `heatmap`, `mesh2d`, `text`. Point data lives in storage buffers and is pulled by instance/vertex index. No wgpu Features are required for the baseline.
- **Float precision:** a per-axis f64 rebasing scheme (Makie's Float32Convert criterion), plus the f64-computed per-frame affine.
- **Render loop:** event-driven winit loop (`ControlFlow::Wait`, EventLoopProxy wakeups, a latest-wins mailbox per plot). Render on demand; Fifo presentation caps the frame rate.

---

## 1. Lines

### 1.1 How GLMakie does it

Files: `assets/shader/lines.vert`, `lines.geom`, `lines.frag`, `src/glshaders/lines.jl`, `src/plot-primitives.jl` (`generate_indices`, `draw_atomic(::Lines)`).

**Input and NaN breaks.**
- `generate_indices` scans the points and emits `GL_LINE_STRIP_ADJACENCY` indices per contiguous non-NaN run, in the form `0 A B C D E F 0`. The neighbor slot is filled with a duplicated index (the NaN point or the first point) when there is no real neighbor.
- A per-vertex `valid_vertex` float is 0 for NaN, 1 for valid, and 2 for a "ghost" vertex.
- Closed loops (first ≈ last and ≥3 segments) get ghost neighbors (`E A B C D E F B`), so both ends form a proper joint without redrawing a segment.
- The geometry shader derives `isvalid[0..3]` from the valid flags and from "same index as the neighbor". It drops the primitive if `isvalid[1]` or `isvalid[2]` is false. A run end becomes a cap (`f_capmode = linecap`); otherwise it is a joint (`joinstyle`).

**Pixel space.** The vertex shader applies projection·view·model, or the CPU pre-projects when dashes are on. The geometry shader converts all four points to pixels with `screen_space(v) = (0.5*v.xy/v.w + 0.5) * px_per_unit * resolution`. Here `resolution` is the scene viewport in Makie units, so pixels are physical. Line width is `px_per_unit * linewidth`.

**Width and AA constants.**
- `AA_RADIUS = 0.8` physical px is the half-width of the smoothstep.
- The quad is padded by `AA_THICKNESS = 4*AA_RADIUS = 3.2` px (2x for linesegments).
- `halfwidth = 0.5*max(0.8, thickness)`.
- Lines thinner than 0.8 px keep the 0.8 px width and fade instead: `alpha *= min(1, thickness/0.8)`. This avoids flicker and darkening.

**Joins.** With `v0, v1, v2` the directions of the previous, current and next segments:
- The miter normal is `normalize(n0+n1)`. For turns over 90° it switches to `sign(dot(v0,n1))*normalize(v0-v1)`, because the sum degenerates.
- A joint is truncated if `dot(v0,v1) < miter_limit` (bevel instead uses `< 0.99`, so it bevels any real bend).
  - `gl_miter_limit = cos(pi - miter_limit)`, set in Makie `backend-functionality.jl:8`.
  - The theme default `miter_limit = pi/3` gives -0.5: miter joints become bevels when the inner angle is under 60°. That equals SVG `stroke-miterlimit = 1/sin(60°/2) = 2`.
- Round joins are truncated under the same test and are drawn with a circle SDF.

Joint extrusion, `mat2 extrusion[end][side]`, measured in halfwidths along v1:
- **Non-truncated joint:** `t = dot(miter_n, v1)/dot(miter_n, n1)`, with `±t` per side. The quad corner is placed on the miter line: `(hw+AA)/miter_offset * miter_n`.
- **Truncated joint:** the quad extends past the point by `hw*max(1,|e|) + AA` along v1.
- `shape_factor` shrinks very short segments so the corner vertices of the two ends cannot cross.

**How overlap at joints is avoided (important for translucent lines).**
- **Non-truncated joints:** both segments' quads end exactly on the shared miter line. They meet edge to edge and do not overlap.
- **Truncated joints:** the quads overlap, and the fragment shader discards fragments on the far side of the miter line:
  ```glsl
  discard_sdf1 = dot(gl_FragCoord.xy - f_linepoints.xy, f_miter_vecs.xy);
  if (quad_sdf.x > 0 && discard_sdf1 > 0) || (quad_sdf.y > 0 && discard_sdf2 >= 0) discard;
  ```
  - The joint point and miter vector are `flat` outputs, computed identically in both segments. The source comment explains why it does not compare interpolated SDFs.
  - Comparing against the exact `gl_FragCoord` makes the two segments partition the pixels exactly. Segment B uses `-m` and `>`, segment A uses `m` and `>=`, so each pixel goes to exactly one of them.
  - A steep "inner truncation" term, `max(sdf, min(quad_sdf.x+1, 100*discard_sdf-1))`, keeps AA from opening a seam along that cut.
- There is no stencil and no OIT. Blending is plain `SrcAlpha/OneMinusSrcAlpha` (alpha `Zero/One`), with a LEQUAL depth test in which everything sits at z=0.
- Self-crossings, back-tracking, and overlap between non-adjacent segments still blend twice. Cairo, which strokes one path, does not.

**Fragment SDF.**
- `quad_sdf = (dot(VP1,-v1), dot(VP2,v1), dot(VP1,n1))`: signed distance behind p1, beyond p2, and across the line.
- Each end then applies one of:
  - round: `min(length(sdf.xz)-hw, sdf.x)`
  - square: `sdf.x - hw`
  - butt, miter or bevel: `max(sdf.x - f_extrusion, f_truncation)`, where `f_truncation` is the bevel's flat-cut SDF
- Then `sdf = max(sdf, |sdf.z| - hw)`, the join-cut terms, and the dash term.
- Coverage: `alpha *= smoothstep(-0.8, 0.8, -sdf)`.
- Colors are flat per end and interpolated in the fragment shader by the normalized along-line distance (`linestart`, `linelength`). Scalar color values are interpolated first, then colormapped.

**Dashes.**
- `linestyle` is a cumulative pattern in units of linewidth:
  - `:dash = [0,3,6]`
  - `:dot = [0,1,3]`
  - `:dashdot = [0,3,6,7,10]`
  - gaps are `:normal` (dot gap 2, dash gap 3), `:dense`, or `:loose`
  - see `conversions.jl` `line_diff_pattern` and `line_pattern`
- It becomes a periodic 1D SDF texture of 100 Float16 samples (`linestyle_to_sdf`, `gappy`; "on" is negative). The shader samples it at `(cum_px - quad_sdf.x + 0.5)/(linewidth*pattern_length)` and scales by linewidth.
- The cumulative length is in screen pixels and is computed on the CPU (`sumlengths`) from CPU-projected points. So dashed lines are re-projected and re-uploaded on every camera change; solid lines take the `FAST_PATH`.
- `process_pattern` checks the dash state at each joint. It either grows or shrinks the segment so a dash does not break across a bevel, or it "freezes" the pattern around the joint (`f_pattern_overwrite`).

**linesegments** (`line_segment.geom`) uses the same fragment shader with no neighbors: caps on both ends and `f_extrusion = 0`.

### 1.2 Port to wgpu without geometry shaders

- **Buffers:**
  - `pts: array<vec2<f32>>` (local f32 coordinates)
  - an optional `cum: array<f32>` of screen-pixel arc length (dashed lines only)
  - optional per-vertex colors or values
- **Draw:** one instance per segment, `draw(0..4, 0..n_seg)` with TriangleStrip. Each of the 4 vertices recomputes the whole geometry-shader math from `p[i-1], p[i], p[i+1], p[i+2]`; the redundant ALU is irrelevant.
- **Vertex-pulling alternative:** `draw(0..6*n_seg)` as a TriangleList with `seg = vid/6`. It is equivalent; benchmark both on the M3.
- **NaN:** detect it in the shader by testing the bits of the loaded coordinates. The f64→f32 conversion on the CPU (not fast-math) carries NaN through. Detect degenerate segments the same way.
- **Loops:** add a `closed` flag that wraps neighbor indices. This is needed for poly outlines and legend patches.
- **Bit-identical joint data:** compute all three directions with the same `normalize(b - a)`, so neighboring instances produce identical flat joint data. GLMakie mixes `/len` and `normalize()`.
- **linesegments:** a `mode` uniform switches to disjoint pairs (`i1 = 2*seg`, no neighbors) for ticks, grid lines and spines.
- **Dashes:**
  - Compute `cum_px` in f64 on the CPU whenever the view changes, as GLMakie does. Use a compute prefix-sum only if dashed lines reach about 10^6 points.
  - Evaluate the pattern analytically from ≤8 breakpoints in the uniform. That is Makie's `gappy`, and it needs no texture.
  - Port `process_pattern` later as a quality refinement.
- **Guard band:** clip segments in the vertex shader to a band around the target (for example ±16k px) before extruding. Joint and cap SDFs interpolated from vertices at around 1e9 px lose all precision near the visible end. GLMakie only handles the w<0 and clip-plane cases.

### 1.3 Translucent lines: what we should do

- **v1:** port GLMakie's partition exactly (non-truncated joints meet on the miter line; truncated joints use the flat-data discard). It is single-pass, exact at every joint, and works with MSAA, because discard is per pixel and the other segment covers those samples.
- Self-crossing darkening then matches GLMakie. Our SVG backend will match Cairo (no darkening), which is the same split as GLMakie vs CairoMakie.
- **Optional later mode for Cairo parity with uniform-color alpha<1 lines:** render coverage only into an R8/R16F target with `BlendOperation::Max`, then composite `color*coverage`. It needs an extra pass, so the MSAA target cannot be transient. Make it opt-in.
- **Rejected:** stencil "write once". It fights SDF AA (a fringe fragment claims a pixel before the core does, leaving notches).

---

## 2. Scatter markers

### How GLMakie does it

Files: `sprites.vert`, `sprites.geom`, `distance_shape.frag`, `dots.*`, `src/glshaders/particles.jl`, and Makie `utilities/texture_atlas.jl` and `conversions.jl`.

**Shape selection.** Shapes are `CIRCLE, RECTANGLE, ROUNDED_RECTANGLE, DISTANCEFIELD, TRIANGLE, ELLIPSE`.
- Only `Circle`/`Rect` (the GeometryBasics types) and image markers are procedural.
- Every symbol marker (`:circle`, `:rect`, `:diamond`, `:utriangle`, `:cross`, `:xcross`, `:star5`, …) is a BezierPath sampled from the shared Float16 SDF glyph atlas (`DISTANCEFIELD`). So is the default `:circle`.

**Sizes.** The paths come from `DEFAULT_MARKER_MAP` (a 1x1 box scaled by 0.75), and path coordinates × markersize = pixels. Values in marker units (multiply by markersize × px_per_unit):

| Marker | Geometry | Size at markersize 9 |
|---|---|---|
| `:circle` | r = 0.3525 | 6.35 px diameter |
| `:rect` | half-side 0.3157 | |
| `:diamond` | the same square rotated 45° | |
| `:cross` / `:+` | arms half-length 0.375, half-width 0.1245 | |
| `:xcross` | the cross rotated 45° | |
| `:utriangle` | vertices (0, 0.485), (±0.36375, −0.2425) | |
| `:pentagon`, `:hexagon` | radius 0.375 | |
| `:star5` | outer 0.45, inner 0.21 | |
| `Circle` / `Rect` types | fill the whole markersize | |

The theme default is `markersize = 9`. The quad (`quad_scale`) is enlarged by the atlas padding factor so the unpadded path spans `markersize × bbox`.

**Geometry.**
- The sprite geometry shader emits a quad of `markersize*ppu`, plus a buffer of `0.8 + stroke + glow` px (converted back to sprite units through the Jacobian of the projection).
- `f_uv` spans [-a, 1+a].
- The fragment shader scales the SDF into pixels via `f_viewport_from_u_scale`.

**AA.**
- Procedural shapes: `ANTIALIAS_RADIUS = 1/√2` px.
- Atlas shapes: the radius comes from SDF derivatives (`aspect_corrected_local_aa_radius`).

**Stroke is outside the shape.**
- The band is `sdf ∈ [-stroke, 0]` when the SDF is positive inside.
- Color transitions from fill to stroke, and the outer half of the band fades to transparent stroke color.
- CairoMakie instead strokes centered on the path (`Cairo.set_line_width`), so the backends differ.

**Per-point attributes.**
- Color is a uniform or a per-point buffer.
- Scalar color values are colormapped in the vertex shader (`util.vert _color`) with low/high/nan clipping.
- Size can be per point (`Vec2f`, so anisotropic sizes are possible). Anisotropic circles use `ELLIPSE`.
- Rotation is per point as a quaternion.
- `markerspace = :pixel` is the default. `:data` sizes are scaled by `f32c_scale`.

**Other details.**
- No pixel snapping anywhere.
- `FastPixel` uses `GL_POINTS` with `gl_PointSize`. wgpu points are always 1 px.

### For ezviz

- Instanced quads with analytic SDFs (exact Euclidean distance, AA of 1/√2 px), using Makie's exact geometry in marker units. Analytic is cheaper and sharper than the atlas and looks identical.
- Primitives: circle, box, rotated box, cross as the union of two boxes, and iq's `sdTriangle`. Add a generic polygon SDF with ≤12 vertices from a uniform for stars and n-gons.
- Per-point color as packed RGBA8 `u32` (`unpack4x8unorm`), per-point f32 size, optional per-point shape id or angle.
- Broadcast a single value by indexing `buf[i*stride]` with stride 0, so one pipeline serves every combination.
- Keep GLMakie's outer stroke. In SVG it is `paint-order:stroke` with a 2× stroke width.
- Do not snap data markers, since snapping jitters in animation. Snapping is fine for legend markers.

---

## 3. Heatmap and image

### How GLMakie does it

Files: `heatmap.vert`, `heatmap.frag`, `glshaders/image_like.jl`, `plot-primitives.jl:882`.

**Edges.** Cell centers become edges on the CPU (`conversions.jl:321 edges`: midpoints plus linear extrapolation at both ends). Vectors of length n+1 are taken as edges as-is.

**Geometry.**
- Heatmap: one instanced quad per cell, `(nx)(ny)` instances. Vertex positions come from `texelFetch` of the x and y edge textures, which handles irregular grids. uv = `index/(dims-1)`, so each cell samples exactly its own texel. `interpolate = false` by default.
- `image` (regular grid) is a single textured quad with `interpolate = true` by default.

**Values and colors.**
- The value texture is Float32.
- `interpolate = true` uses hardware linear filtering on the values, then colormaps. The result is bilinear between cell centers, clamped at the edges, and NaN spreads to neighbors.
- The colormap is a 1D texture of `to_colormap`, which gives 256 samples for PlotUtils gradients (`conversions.jl:1659`). It uses linear or nearest filtering depending on `color_mapping_type`, with a half-texel remap: `i01 = (1 - 1/N)*t + 0.5/N`.
- Colorrange and colorscale (log etc.) are applied on the CPU (`scaled_color`, `scaled_colorrange`). Auto colorrange is NaN-aware extrema (`distinct_extrema_nan`).
- Defaults: `lowclip`/`highclip` are the first/last colormap colors, and `nan_color = :transparent`.

**Large data.** GLMakie does nothing: `glTexImage` errors past `GL_MAX_TEXTURE_SIZE` (`GLExtendedFunctions.jl:211`). Its docs say "heatmap is slower than image".

### For ezviz (fragment-driven lookup, one quad)

- **Quad:** draw one quad = heatmap bbox ∩ axis rect, computed in f64 on the CPU, so vertices stay bounded.
- **Regular edges:** the fragment maps its pixel center to a fractional cell index with one FMA per axis. The coefficients `frag*a + b` are computed in f64 per frame. This is precision-proof at any zoom, and the cost is O(pixels), not O(cells).
- **Irregular edges** (including a regular grid on a log axis): binary search in an `edges` storage buffer, about 12 steps for 4096.
- **Nearest:** index `z[iy*nx + ix]`, x fastest (Makie's `z[i,j]`).
- **Interpolate:** manual bilinear of 4 loads at `f - 0.5` with clamped indices, which reproduces GLMakie's clamp-to-edge. NaN is detected with a bit test and mapped to `nan_color`, like GLMakie; NaN-aware renormalization is an option.
- **Data in a storage buffer, not a texture:**
  - no 2D dimension limit
  - no filterability issue (we do not use hardware filtering)
  - live updates are a single `write_buffer`, with no 256-byte row rules
  - default binding limit is 128 MiB (32M f32). Request `adapter.limits()` for more.
  - beyond that, split into row bands, each with its own buffer and quad, plus 1 halo row when interpolating
- **Colormap:** a 256×1 2D texture (Rgba8Unorm non-sRGB, or Rgba16Float), linear clamp sampler, `textureSampleLevel`, same half-texel remap. Categorical maps use nearest.
- **Colorscale:** apply log10 or sqrt in the shader through a uniform enum (`log2(x)*0.30103`) instead of a CPU pass over large live fields. The colorrange is transformed on the CPU.
- **Optional bandwidth halving:** store an f16 field in `array<u32>` and decode with `unpack2x16float`, which is core WGSL and needs no feature.
- **Later:** a min/max or mean pyramid for data much larger than the screen, to avoid nearest-sampling moiré.
- **Reuse:** Colorbar is the same pipeline with an N×1 grid; `image` (RGBA) is the same quad with an Rgba8Unorm texture.

---

## 4. Filled polygons: bars, bands, poly

### How GLMakie does it

- Poly and bar geometry is triangulated on the CPU (GeometryBasics earcut). Band builds a strip mesh between the lower and upper curves.
- These draw with `mesh.vert`/`mesh.frag`. Strokes are drawn as separate `lines!` (theme `patchstrokewidth = 0`).
- **Anti-aliasing is FXAA, not MSAA.**
  - `fxaa = true` is the generic default and applies to mesh, poly (and so barplot), band fill, and heatmap.
  - `lines`, `linesegments`, `scatter`, `text` and `image` set `fxaa = false` because they use SDF AA.
  - The flag is stored in the high bit of each pixel's object id. `postprocess.frag` sets luma to 1 to exempt those pixels, and `fxaa.frag` runs full-screen.
- There is no MSAA option. The only supersampling comes from `px_per_unit` (2 on Retina).
- CairoMakie uses analytic coverage, so adjacent shapes show conflation seams (see the comment around `CairoMakie/overrides.jl:432` about band gaps).

### For ezviz

- **Fills:** use a `mesh2d` pipeline (indexed TriangleList; vertex `{pos: Float32x2 local, color: Unorm8x4 or f32 value}`) with 4x MSAA for AA. On Apple GPUs a tile-resolved MSAA target is nearly free when it is transient. MSAA gives no seams between adjacent bars or hist bins, and polygons that share an edge rasterize correctly.
- **Geometry sources:**
  - bars/hist: 4 vertices and 6 indices per rect, generated on the CPU
  - band: direct strip
  - general poly: `earcut` (pure Rust), or `lyon_tessellation` if we need robust self-intersection handling
  - strokes: the line pipeline with `closed = true`
- **FXAA:** skip it. It softens text and lines, and GLMakie has to mask it per object for that reason.

---

## 5. Text

### How GLMakie does it

- One global SDF atlas, shared with the bezier markers (`TextureAtlas(resolution=2048, pix_per_glyph=64, glyph_padding=12, downsample=5)`):
  - glyphs are rendered by FreeType at 320 px and thresholded
  - an exact signed distance field is computed and downsampled ×5, then stored as Float16
  - the atlas is disk-cached, and it errors when full
- Text is drawn by the sprite pipeline with `shape = DISTANCEFIELD`:
  - glyph positions are laid out on the CPU with FreeType metrics
  - each glyph quad is `fontsize × padded glyph box`, in pixel markerspace, × ppu
  - rotation is a per-glyph quaternion about z, with glyph origins rotated on the CPU
  - AA comes from SDF derivatives

### Would a coverage atlas suffice?

Yes, and it is better for us. Plot text is a handful of fixed sizes (tick labels, labels, titles, legends), axis-aligned or at 90°.
- Rasterize grayscale coverage at the exact pixel size (`fontsize*ppu`) into an R8Unorm atlas, keyed by (font, glyph id, size_px, subpixel-x bin 0..3). Snap the baseline to whole pixels. Draw instanced quads with nearest sampling when axis-aligned, linear when rotated.
- Small text (14 px) comes out crisper than 64 px SDFs, and the look is closer to Cairo's grayscale coverage.
- 90° rotation stays exact when snapped. Arbitrary angles get a slight bilinear blur, which is acceptable.
- A changed ppu (export at 2 vs a window at 1) just rasterizes a new size.
- Use SDF only if we ever want continuously scaling text in data markerspace.
- Crates:
  - rasterizing: `swash` (subpixel offsets, variable fonts) or `fontdue`
  - shaping/kerning: `rustybuzz`
  - packing: `etagere`
  - Embed TeX Gyre Heros (GUST license).
- Log tick labels such as "10⁻³" need a small rich-text layout: a superscript run at a smaller size with a raised baseline, which is how Makie formats them.

---

## 6. Clipping, draw order, transparency, color space

**GLMakie.**
- Each render object draws with `glViewport` set to its scene's area × ppu (`rendering.jl`), so primitives are clipped to the axis by the clip volume.
- Per-scene backgrounds are cleared with scissor.
- Axis scene areas are rounded to whole units (`makielayout/helpers.jl:50 round_to_IRect2D`).
- The render list is stable-sorted globally by the plot's z translation. Axis conventions:

  | Element | z |
  |---|---|
  | background | -100 |
  | grid | -10 |
  | plots | 0 |
  | ticks | +10 |
  | spines | +20 |

- Grid lines live in the clipped axis scene; ticks and spines live in the unclipped block scene.
- Blending is `glBlendFuncSeparate(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ZERO, ONE)` (`GLRender.jl`), with depth test LEQUAL and depth writes on. `transparency = true` switches to weighted-blended OIT, which is only relevant in 3D.
- The framebuffer is RGBA8 (N0f8) without sRGB conversion. Blending, FXAA and colormap interpolation all happen on sRGB-encoded values, which is also what Cairo does.

**For ezviz.**
- The viewport is the full target. Call `set_scissor_rect` per draw item: the axis rect for plots and grid, the figure rect for decorations. Everything goes in one pass.
- Painter's order: `(z, seq)`, no depth buffer. The GPU blends in primitive order within a draw, so overlapping translucent markers composite in index order, like Cairo.
- Use premultiplied "over" for both color and alpha. On an opaque background it matches GLMakie exactly. It is also correct for transparent-background PNGs (un-premultiply on readback), and it is the correct input for MSAA resolve.
- Use non-sRGB formats everywhere so that on-screen, PNG (GPU) and SVG (browser, gamma-space) look alike.
- **Snapping decoration lines:** Makie does not snap. At ppu = 2, integer-unit layout already makes 1-unit spines crisp; at ppu = 1 they straddle a pixel boundary and blur. Add a `snap` flag for spines, ticks and grid that rounds the line center to a pixel center for odd pixel widths, or to a pixel edge for even widths.

---

## 7. Float32 precision

**Makie** (`float32-scaling.jl`, `makielayout/blocks/axis.jl:64 update_axis_camera`):
- Each Axis scene has a `Float32Convert(resolution = 1e4)` holding a `LinearScaling`.
- On every limits change, it applies `transform_func` (log etc.) to the limits in f64, then calls `update_limits!`. That rescales so the current limits map to [-1, 1] when either condition holds:
  - fewer than 1e4 distinct f32 values span the visible range: `delta < 1e4 * eps(Float32) * max(|lo|, |hi|)`
  - the range falls outside roughly `floatmin*1e4 .. floatmax/1e4`
- When it rescales, every plot re-runs `positions_transformed_f32c` on the CPU (transform, then model, then f32c, all in f64, then converted to Float32) and re-uploads.
- The ortho projection is built in Float32 from the converted limits. Pans and zooms that stay inside the safe range only change the projection uniform.
- `markerspace = :data` sizes are scaled by `f32c_scale`.
- Colors and values are converted to Float32 (`smallfloat_convert`, clamped to ±floatmax).

**For ezviz:**
- Use the same per-axis rebase criterion and hysteresis. Store `local_f32 = f32((T(x) - origin) * scale)`, computed in f64.
- Per frame, compute the final `local → fb_px` affine `(sx, sy, tx, ty)` in f64 from the axis limits, axis pixel rect and ppu, then upload it as f32. This is strictly better than Makie's f32 projection, because translation never loses bits.
- Rebasing is rare. Live simulation data is re-converted on every upload anyway.
- Log and other scales are applied on the CPU before conversion, as in Makie.
- Heatmap cell lookup uses its own f64-derived pixel → index coefficients (section 3).
- Values stay f32. As an option, subtract a colorrange origin for data with a huge offset and a small variation.

---

## 8. Render loop

**GLMakie** (`screen.jl`):
- Defaults: `render_on_demand = true`, `framerate = 30`, `vsync = false`.
- `on_demand_renderloop` wakes every 1/framerate. Each tick it:
  - calls `pollevents`
  - calls `poll_updates`, which pulls each plot's `gl_renderobject` compute node; dirty inputs re-upload buffers and set `screen.requires_update`
  - renders and swaps only if `requires_update`
- Scene observables (camera, viewport, background) also set `requires_update`.
- The vsync and fps loops render unconditionally.

**For ezviz:**
- Event-driven winit loop: 0.30.13 is the latest stable (`ApplicationHandler`); 0.31 is in beta.
  - Use `ControlFlow::Wait`, which is 0% CPU when idle.
  - Plot setters write the latest data into a per-plot mailbox (a latest-wins slot or triple buffer) and set dirty bits. If no wake is pending (one AtomicBool), they call `EventLoopProxy::send_event(Wake)`, and the handler calls `request_redraw()`.
  - On `RedrawRequested`: drain the mailboxes, upload only what changed (write in place if capacity allows, otherwise grow 1.5× and rebuild the bind group), relayout if needed, render, present.
- **Frame rate:** `PresentMode::AutoVsync` caps it at the display rate (120 Hz ProMotion). An optional `max_fps` uses `WaitUntil`. A simulation producing 1000 updates/s gets at most 120 renders, always of the latest state.
- **Threading:** macOS needs the event loop on the main thread, so the simulation runs on another thread holding `Send + Clone` plot handles.
- `EventLoop::new` may be called only once per process. Use `run_app_on_demand` so `fig.show()` can be called repeatedly. `pump_app_events` gives a GLMakie-like non-blocking mode, but on macOS a live window resize is modal and stalls it.

---

## 9. Headless export, px_per_unit, HiDPI

**GLMakie:**
- `px_per_unit` defaults to the window scalefactor (2.0 on Retina, `screen.jl:421`). The framebuffer is `round(ppu * scene_size)`.
- Every pixel-space attribute is multiplied by ppu in the shaders. The AA radii are not scaled: they stay in physical pixels.
- `colorbuffer` renders into the offscreen FBO at the current ppu (save may set another) and reads it back. That is RGB readback, flipped from GL's bottom-left origin.
- Headless mode is a hidden GLFW window.
- Makie attaches `dpi = 96*ppu` to PNGs (`display.jl:503`).
- CairoMakie defaults: `px_per_unit = 2`, `pt_per_unit = 0.75`.

**For ezviz:**
- True headless: `request_adapter(compatible_surface: None)`, with one shared device reused for export.
- Render target: Rgba8Unorm with `RENDER_ATTACHMENT | COPY_SRC`, size `round(size*ppu)`, as the MSAA resolve target.
- Readback: `copy_texture_to_buffer` with `bytes_per_row = align_up(w*4, 256)`, `map_async` plus a device poll, then strip the padding. Un-premultiply if the background is transparent.
- Write the PNG with pHYs = 96·ppu dpi. No y-flip is needed, because wgpu textures are top-left origin.
- If `size*ppu` exceeds the texture limit (8192 default; the M3 reports 16384, so request `adapter.limits()`), render in tiles. In the pixel-space design a tile is just `t -= tile_origin` on every affine plus a scissor.
- Window: logical size = figure units (CSS px = points), `ppu = window.scale_factor()`.
  - On `Resized`: resize the figure, relayout, and recreate the surface and MSAA textures.
  - On `ScaleFactorChanged`: update ppu; new glyph sizes get rasterized.

---

## 10. Recommended wgpu pipeline set

**Shared state for every pipeline:**
- `MultisampleState { count: 4, mask: !0, alpha_to_coverage_enabled: false }`
- target format = the resolve format (surface `Bgra8Unorm`, offscreen `Rgba8Unorm`)
- no depth-stencil
- `cull_mode: None`, because the y-flip reverses triangle winding
- `blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING)`

**Bind groups:**
- `@group(0)` per frame: `Globals { target_px: vec2f, ppu: f32, _pad }`, a linear-clamp sampler and a nearest-clamp sampler.
- `@group(1)` per plot: one uniform struct (160 B max; separate small buffers, or one ring with 256-byte dynamic offsets), storage buffers, textures. Rebuilt only when a buffer is reallocated.
- Unused storage slots get a 16-byte dummy buffer, because zero-size bindings are invalid.

| Pipeline | Topology / draw | Data | Uniform |
|---|---|---|---|
| `line` | TriangleStrip, `draw(0..4, 0..n_seg)` | `pts: array<vec2f>`, `cum: array<f32>` (dashed only), `vcol: array<vec4f>` (stride 0 or 1), colormap tex | `LineU`: xform, color, linewidth_px, miter_limit, join, cap, color_mode, mode, dash breaks, CMap |
| `marker` | TriangleStrip, `draw(0..4, 0..n)` | `pos: array<vec2f>`, `cols: array<u32>` (RGBA8), `sizes: array<f32>`, optional `shape: array<u32>`, `angle: array<f32>` | `MarkerU`: xform, color, stroke color/px, size_px, shape, strides |
| `heatmap` | TriangleStrip, `draw(0..4)` | `z: array<f32>`, `xedges`/`yedges: array<f32>`, colormap tex (256×1) | `HeatU`: rect_px, index map, local map, dims, interpolate, irregular flags, CMap |
| `mesh2d` | indexed TriangleList | vertex buffer `{Float32x2, Unorm8x4}` (+ `Float32` value variant) | xform, CMap |
| `text` | TriangleStrip, `draw(0..4, 0..n_glyphs)` | `glyphs: array<{pos_px, size_px, uv_rect, color u32, angle}>`, R8Unorm atlas | none beyond globals |

**Frame structure:**
1. Layout.
2. Compute each axis's f64 affine; rebase and re-upload if needed.
3. Write uniforms.
4. Begin one pass: MSAA view with Clear(figure background) and `StoreOp::Discard`, resolve into the surface or offscreen texture.
5. For each draw item sorted by (z, seq): set pipeline, bind groups and scissor, then draw.
6. End the pass and present, or read back.

---

## 11. WGSL sketches

### Shared code (group 0 and colormap)

```wgsl
struct Globals { target_px: vec2<f32>, ppu: f32, _pad: f32 };
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var lin_samp: sampler;                  // linear, clamp-to-edge

fn px_to_clip(px: vec2<f32>) -> vec4<f32> {                   // px: physical, origin top-left, y down
    return vec4<f32>(2.0 * px.x / g.target_px.x - 1.0, 1.0 - 2.0 * px.y / g.target_px.y, 0.0, 1.0);
}
// Metal uses fast-math and WGSL has no isNan: test the bits of values loaded straight from memory.
fn finite_bits(x: f32) -> bool { return (bitcast<u32>(x) & 0x7fffffffu) < 0x7f800000u; }
fn nan_bits(x: f32) -> bool    { return (bitcast<u32>(x) & 0x7fffffffu) > 0x7f800000u; }

struct CMap { range: vec2<f32>, n: f32, _pad: f32, lowclip: vec4<f32>, highclip: vec4<f32>, nan_color: vec4<f32> };
fn cmap(v: f32, cm: CMap, tex: texture_2d<f32>) -> vec4<f32> {
    if (nan_bits(v)) { return cm.nan_color; }
    if (v < cm.range.x) { return cm.lowclip; }
    if (v > cm.range.y) { return cm.highclip; }
    let t = (v - cm.range.x) / (cm.range.y - cm.range.x);
    let s = (1.0 - 1.0 / cm.n) * t + 0.5 / cm.n;              // first..last texel centre (GLMakie)
    return textureSampleLevel(tex, lin_samp, vec2<f32>(s, 0.5), 0.0);  // Level: no uniformity constraint
}
```

### Lines (port of `lines.geom` and `lines.frag`)

```wgsl
const AA_RADIUS: f32 = 0.8;   // px
const AA_PAD: f32 = 3.2;      // 4 * AA_RADIUS
const BUTT: u32 = 0u; const SQUARE: u32 = 1u; const ROUND: u32 = 2u; const BEVEL: u32 = 3u; // MITER = 0

struct LineU {
    xform: vec4<f32>,          // fb_px = local * xy + zw   (f64 on CPU)
    color: vec4<f32>,          // straight alpha (color_mode 0)
    linewidth_px: f32, miter_limit: f32, joinstyle: u32, linecap: u32,   // miter_limit = cos(pi - angle)
    color_mode: u32, mode: u32, n_breaks: u32, pattern_len: f32,          // mode 0 lines, 1 linesegments
    breaks: array<vec4<f32>, 2>,                                          // cumulative, linewidth units
    cm: CMap,
};
@group(1) @binding(0) var<uniform> u: LineU;
@group(1) @binding(1) var<storage, read> pts: array<vec2<f32>>;
@group(1) @binding(2) var<storage, read> cum: array<f32>;         // screen-px arc length at each point
@group(1) @binding(3) var<storage, read> vcol: array<vec4<f32>>;  // rgba, or (value,0,0,0) for colormap
@group(1) @binding(4) var cmap_tex: texture_2d<f32>;

struct LineV {
    @builtin(position) pos: vec4<f32>,
    @location(0) quad_sdf: vec3<f32>,
    @location(1) trunc: vec2<f32>,
    @location(2) start_len: vec2<f32>,
    @location(3) @interpolate(flat) ext_w: vec4<f32>,      // extrusion.xy, halfwidth, alpha_weight
    @location(4) @interpolate(flat) linepts: vec4<f32>,
    @location(5) @interpolate(flat) miter_vecs: vec4<f32>,
    @location(6) @interpolate(flat) c1: vec4<f32>,
    @location(7) @interpolate(flat) c2: vec4<f32>,
    @location(8) @interpolate(flat) capmode: vec2<u32>,
    @location(9) @interpolate(flat) cum0: f32,
};

fn perp(v: vec2<f32>) -> vec2<f32> { return vec2<f32>(-v.y, v.x); }
fn sgn_nz(x: f32) -> f32 { return select(-1.0, 1.0, x >= 0.0); }
fn to_px(q: vec2<f32>) -> vec2<f32> { return q * u.xform.xy + u.xform.zw; }
fn vcolor(i: u32) -> vec4<f32> { return select(vcol[i], u.color, u.color_mode == 0u); }

@vertex
fn vs_line(@builtin(vertex_index) vid: u32, @builtin(instance_index) seg: u32) -> LineV {
    var o: LineV;                                          // zero-init => degenerate if we bail
    let n = arrayLength(&pts);
    let i1 = select(seg, 2u * seg, u.mode == 1u);
    let i2 = i1 + 1u;
    let a = pts[i1]; let b = pts[i2];
    if (!(finite_bits(a.x) && finite_bits(a.y) && finite_bits(b.x) && finite_bits(b.y))) { return o; }
    let ia = max(i1, 1u) - 1u;  let ib = min(i2 + 1u, n - 1u);    // select() evaluates both arms
    let pa = pts[ia]; let pb = pts[ib];
    let ok0 = u.mode == 0u && i1 > 0u     && finite_bits(pa.x) && finite_bits(pa.y);
    let ok3 = u.mode == 0u && i2 + 1u < n && finite_bits(pb.x) && finite_bits(pb.y);
    let p1 = to_px(a); let p2 = to_px(b);
    let p0 = select(p1, to_px(pa), ok0);
    let p3 = select(p2, to_px(pb), ok3);
    // (guard-band clipping of p1/p2 against the target +- 16k px goes here)

    let seg_len = length(p2 - p1);
    if (seg_len < 1e-4) { return o; }
    let hw = 0.5 * max(AA_RADIUS, u.linewidth_px);
    // same normalize() everywhere => neighbouring instances produce bit-identical joint data
    let v1 = normalize(p2 - p1);
    let v0 = select(v1, normalize(p1 - p0), ok0 && any(p1 != p0));
    let v2 = select(v1, normalize(p3 - p2), ok3 && any(p3 != p2));
    let n0 = perp(v0); let n1 = perp(v1); let n2 = perp(v2);

    let cosang = vec2<f32>(dot(v0, v1), dot(v1, v2));
    let mn1 = select(normalize(n0 + n1), sgn_nz(dot(v0, n1)) * normalize(v0 - v1), cosang.x < 0.0);
    let mn2 = select(normalize(n1 + n2), sgn_nz(dot(v1, n2)) * normalize(v1 - v2), cosang.y < 0.0);
    let lim = select(u.miter_limit, 0.99, u.joinstyle == BEVEL);
    let tr0 = ok0 && cosang.x < lim;          // truncated joint at p1
    let tr1 = ok3 && cosang.y < lim;          // truncated joint at p2
    let mv1 = -perp(mn1); let mv2 = -perp(mn2);
    let mo1 = dot(mn1, n1); let mo2 = dot(mn2, n1);

    // extension along v1 (in halfwidths) at p1 (e0) / p2 (e1); .x = -n side, .y = +n side
    var e0: vec2<f32>; var e1: vec2<f32>;
    if (tr0) { e0 = vec2<f32>(-abs(mo1 / dot(mv1, n1))); } else { let t = dot(mn1, v1) / mo1; e0 = vec2<f32>(-t, t); }
    if (tr1) { e1 = vec2<f32>( abs(mo2 / dot(mn2, v1))); } else { let t = dot(mn2, v1) / mo2; e1 = vec2<f32>(-t, t); }
    let w = hw + AA_PAD;
    var sf = vec2<f32>(1.0);                  // shrink short segments so joint vertices don't cross
    if ((ok0 && ok3) || u.linecap == BUTT) {
        sf = vec2<f32>(max(0.0, seg_len / max(seg_len, w * (e0.x - e1.x))),
                       max(0.0, seg_len / max(seg_len, w * (e0.y - e1.y))));
    }

    // flat data, identical for all 4 vertices
    let dummy = normalize(vec2<f32>(-1.0));   // with linepts = -1e12 the discard test never fires
    o.linepts    = vec4<f32>(select(vec2<f32>(-1e12), p1, tr0), select(vec2<f32>(-1e12), p2, tr1));
    o.miter_vecs = vec4<f32>(select(dummy, -mv1, tr0), select(dummy, mv2, tr1));
    o.ext_w      = vec4<f32>(select(0.0, 1e12, ok0), select(0.0, 1e12, ok3), hw, min(1.0, u.linewidth_px / AA_RADIUS));
    o.capmode    = vec2<u32>(select(u.linecap, u.joinstyle, ok0), select(u.linecap, u.joinstyle, ok3));
    o.c1 = vcolor(i1); o.c2 = vcolor(i2);
    o.cum0 = cum[min(i1, arrayLength(&cum) - 1u)];

    // this vertex: x = end (p1/p2), y = side (-n/+n); strip order (0,0),(0,1),(1,0),(1,1)
    let x = vid >> 1u; let y = vid & 1u;
    let sx = f32(x) * 2.0 - 1.0; let sy = f32(y) * 2.0 - 1.0;
    let e  = select(e0, e1, x == 1u);
    let ey = select(e.x, e.y, y == 1u);
    let sfy = select(sf.x, sf.y, y == 1u);
    var off: vec2<f32>;
    if (select(tr0, tr1, x == 1u) || !select(ok0, ok3, x == 1u)) {
        // cap or truncated joint: extend along v1, overlap is resolved per pixel in the FS
        off = sfy * ((hw * max(1.0, abs(ey)) + AA_PAD) * sx * v1 + sy * w * n1);
    } else {
        // miter joint: end exactly on the miter line => no overlap with the neighbour
        off = sy * sfy * w / select(mo1, mo2, x == 1u) * select(mn1, mn2, x == 1u);
    }
    let p = select(p1, p2, x == 1u) + off;
    let vp1 = p - p1; let vp2 = p - p2;
    o.quad_sdf = vec3<f32>(dot(vp1, -v1), dot(vp2, v1), dot(vp1, n1));
    o.trunc = vec2<f32>(
        select(-1.0, dot(vp1, sign(dot(mn1, -v1)) * mn1) - hw * abs(mo1), tr0),
        select(-1.0, dot(vp2, sign(dot(mn2,  v1)) * mn2) - hw * abs(mo2), tr1));
    let e0y = select(e0.x, e0.y, y == 1u); let e1y = select(e1.x, e1.y, y == 1u);
    o.start_len = vec2<f32>(sfy * hw * e0y, max(1.0, seg_len - sfy * hw * (e0y - e1y)));
    o.pos = px_to_clip(p);
    return o;
}

fn brk(j: u32) -> f32 { return u.breaks[j / 4u][j % 4u]; }
fn pattern_sdf(s: f32) -> f32 {               // s in linewidths; < 0 => dash "on" (Makie gappy)
    let x = s - floor(s / u.pattern_len) * u.pattern_len;
    var d = 1e9;
    for (var j = 0u; j + 1u < u.n_breaks; j++) {
        let a = brk(j); let b = brk(j + 1u);
        if (x >= a && x <= b) { let m = min(x - a, b - x); d = select(m, -m, (j & 1u) == 0u); }
    }
    return d;
}

@fragment
fn fs_line(in: LineV) -> @location(0) vec4<f32> {
    let hw = in.ext_w.z;
    // pixel-exact split of overlapping truncated joints (flat data identical in both segments)
    let d1 = dot(in.pos.xy - in.linepts.xy, in.miter_vecs.xy);
    let d2 = dot(in.pos.xy - in.linepts.zw, in.miter_vecs.zw);
    if ((in.quad_sdf.x > 0.0 && d1 > 0.0) || (in.quad_sdf.y > 0.0 && d2 >= 0.0)) { discard; }

    var sdf: f32;
    if (in.capmode.x == ROUND)       { sdf = min(length(in.quad_sdf.xz) - hw, in.quad_sdf.x); }
    else if (in.capmode.x == SQUARE) { sdf = in.quad_sdf.x - hw; }
    else                             { sdf = max(in.quad_sdf.x - in.ext_w.x, in.trunc.x); }
    if (in.capmode.y == ROUND)       { sdf = max(sdf, min(length(in.quad_sdf.yz) - hw, in.quad_sdf.y)); }
    else if (in.capmode.y == SQUARE) { sdf = max(sdf, in.quad_sdf.y - hw); }
    else                             { sdf = max(sdf, max(in.quad_sdf.y - in.ext_w.y, in.trunc.y)); }
    sdf = max(sdf, abs(in.quad_sdf.z) - hw);                         // width
    sdf = max(sdf, min(in.quad_sdf.x + 1.0, 100.0 * d1 - 1.0));      // steep AA along the split
    sdf = max(sdf, min(in.quad_sdf.y + 1.0, 100.0 * d2 - 1.0));
    if (u.n_breaks > 1u) {
        let s_px = in.cum0 - in.quad_sdf.x;                          // arc length from line start
        sdf = max(sdf, 2.0 * hw * pattern_sdf(s_px / (2.0 * hw)));
    }
    let t = clamp((-in.quad_sdf.x - in.start_len.x) / in.start_len.y, 0.0, 1.0);
    var c = mix(in.c1, in.c2, t);
    if (u.color_mode == 2u) { c = cmap(c.x, u.cm, cmap_tex); }
    let a = c.a * in.ext_w.w * smoothstep(-AA_RADIUS, AA_RADIUS, -sdf);
    if (a <= 0.0) { discard; }
    return vec4<f32>(c.rgb * a, a);                                  // premultiplied
}
```

### SDF markers

```wgsl
struct MarkerU {
    xform: vec4<f32>, color: vec4<f32>, stroke_color: vec4<f32>,
    size_px: f32, stroke_px: f32, shape: u32, col_stride: u32,
    size_stride: u32, _p0: u32, _p1: u32, _p2: u32,
};
@group(1) @binding(0) var<uniform> m: MarkerU;
@group(1) @binding(1) var<storage, read> mpos: array<vec2<f32>>;
@group(1) @binding(2) var<storage, read> mcol: array<u32>;     // RGBA8, bytes r,g,b,a
@group(1) @binding(3) var<storage, read> msize: array<f32>;

struct MarkerV {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,                                   // px from centre, y up
    @location(1) @interpolate(flat) fill: vec4<f32>,             // premultiplied
    @location(2) @interpolate(flat) size: f32,
};

@vertex
fn vs_marker(@builtin(vertex_index) vid: u32, @builtin(instance_index) i: u32) -> MarkerV {
    var o: MarkerV;
    let p = mpos[i];
    if (!(finite_bits(p.x) && finite_bits(p.y))) { return o; }
    let size = select(m.size_px, msize[i * m.size_stride], m.size_stride != 0u);
    let c = select(m.color, unpack4x8unorm(mcol[i * m.col_stride]), m.col_stride != 0u);
    let half = 0.5 * size + m.stroke_px + 1.0;                   // + outer stroke + AA margin
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u)) * 2.0 - 1.0;
    let center = p * m.xform.xy + m.xform.zw;
    o.pos = px_to_clip(center + corner * half);
    o.q = vec2<f32>(corner.x, -corner.y) * half;
    o.fill = vec4<f32>(c.rgb * c.a, c.a);
    o.size = size;
    return o;
}

fn sd_box(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}
fn rot45(p: vec2<f32>) -> vec2<f32> { return vec2<f32>(p.x + p.y, p.y - p.x) * 0.70710678; }
fn sd_tri(p: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> f32 {   // iq
    let e0 = p1 - p0; let e1 = p2 - p1; let e2 = p0 - p2;
    let w0 = p - p0;  let w1 = p - p1;  let w2 = p - p2;
    let q0 = w0 - e0 * clamp(dot(w0, e0) / dot(e0, e0), 0.0, 1.0);
    let q1 = w1 - e1 * clamp(dot(w1, e1) / dot(e1, e1), 0.0, 1.0);
    let q2 = w2 - e2 * clamp(dot(w2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(min(vec2<f32>(dot(q0, q0), s * (w0.x * e0.y - w0.y * e0.x)),
                    vec2<f32>(dot(q1, q1), s * (w1.x * e1.y - w1.y * e1.x))),
                    vec2<f32>(dot(q2, q2), s * (w2.x * e2.y - w2.y * e2.x)));
    return -sqrt(d.x) * sign(d.y);
}
// q in marker units (markersize = 1), y up; < 0 inside. Geometry = Makie DEFAULT_MARKER_MAP.
fn marker_sdf(shape: u32, q: vec2<f32>) -> f32 {
    switch shape {
        case 0u: { return length(q) - 0.3525; }                                   // :circle
        case 1u: { return sd_box(q, vec2<f32>(0.3157)); }                         // :rect
        case 2u: { return sd_box(rot45(q), vec2<f32>(0.3157)); }                  // :diamond
        case 3u: { return min(sd_box(q, vec2<f32>(0.375, 0.1245)), sd_box(q, vec2<f32>(0.1245, 0.375))); } // :cross
        case 4u: { let r = rot45(q); return min(sd_box(r, vec2<f32>(0.375, 0.1245)), sd_box(r, vec2<f32>(0.1245, 0.375))); } // :xcross
        case 5u: { return sd_tri(q, vec2<f32>(0.0, 0.485), vec2<f32>(-0.36375, -0.2425), vec2<f32>(0.36375, -0.2425)); } // :utriangle
        case 6u: { return length(q) - 0.5; }                                      // Circle type
        default: { return sd_box(q, vec2<f32>(0.5)); }                            // Rect type
    }
}

@fragment
fn fs_marker(in: MarkerV) -> @location(0) vec4<f32> {
    let d = marker_sdf(m.shape, in.q / in.size) * in.size;         // px
    let aa = 0.70710678;                                            // GLMakie marker AA radius
    let sw = m.stroke_px;
    let cover = 1.0 - smoothstep(sw - aa, sw + aa, d);              // fill U outer stroke
    let k = select(0.0, smoothstep(-aa, aa, d), sw > 0.0);          // fill -> stroke
    let s = vec4<f32>(m.stroke_color.rgb * m.stroke_color.a, m.stroke_color.a);
    let out = mix(in.fill, s, k) * cover;
    if (out.a <= 0.0) { discard; }
    return out;
}
```

### Colormapped heatmap (one quad, fragment lookup)

```wgsl
struct HeatU {
    rect_px: vec4<f32>,        // quad = heatmap bbox ∩ axis rect (x0,y0,x1,y1), f64 on CPU
    imap: vec4<f32>,           // regular: frac cell index = frag.xy * imap.xy + imap.zw (f64 on CPU)
    lmap: vec4<f32>,           // irregular: local coord = frag.xy * lmap.xy + lmap.zw
    dims: vec2<u32>,           // nx, ny cells
    interpolate: u32,
    irregular: u32,            // bit0 x, bit1 y
    cm: CMap,
};
@group(1) @binding(0) var<uniform> h: HeatU;
@group(1) @binding(1) var<storage, read> z: array<f32>;        // nx*ny, x fastest (Makie z[i,j])
@group(1) @binding(2) var<storage, read> xedges: array<f32>;   // nx+1 local coords, ascending
@group(1) @binding(3) var<storage, read> yedges: array<f32>;
@group(1) @binding(4) var cmap_tex: texture_2d<f32>;

@vertex
fn vs_heat(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    let t = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    return px_to_clip(mix(h.rect_px.xy, h.rect_px.zw, t));
}

// storage-array pointer parameters need an extension, so one function per edge array
fn search_x(xl: f32) -> f32 {
    var lo = 0u; var hi = h.dims.x;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) >> 1u;
        if (xedges[mid] <= xl) { lo = mid; } else { hi = mid; }
    }
    return f32(lo) + (xl - xedges[lo]) / (xedges[lo + 1u] - xedges[lo]);
}
// search_y: identical with yedges / h.dims.y

fn zval(ix: i32, iy: i32) -> f32 {
    let x = u32(clamp(ix, 0, i32(h.dims.x) - 1));
    let y = u32(clamp(iy, 0, i32(h.dims.y) - 1));
    return z[y * h.dims.x + x];
}

@fragment
fn fs_heat(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var f = frag.xy * h.imap.xy + h.imap.zw;
    if ((h.irregular & 1u) != 0u) { f.x = search_x(frag.x * h.lmap.x + h.lmap.z); }
    if ((h.irregular & 2u) != 0u) { f.y = search_y(frag.y * h.lmap.y + h.lmap.w); }
    var v: f32;
    var bad: bool;
    if (h.interpolate == 0u) {
        v = zval(i32(floor(f.x)), i32(floor(f.y)));
        bad = nan_bits(v);
    } else {                                        // bilinear between cell centres, clamp at edges
        let gg = f - vec2<f32>(0.5);
        let i0 = vec2<i32>(floor(gg));
        let w = fract(gg);
        let a = zval(i0.x, i0.y);     let b = zval(i0.x + 1, i0.y);
        let c = zval(i0.x, i0.y + 1); let d = zval(i0.x + 1, i0.y + 1);
        bad = nan_bits(a) || nan_bits(b) || nan_bits(c) || nan_bits(d);   // NaN spreads, like GLMakie
        v = mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
    }
    let col = select(cmap(v, h.cm, cmap_tex), h.cm.nan_color, bad);
    return vec4<f32>(col.rgb * col.a, col.a);
}
```

For a regular grid, `imap` is computed on the CPU (per axis, f64). With `edge0` the first edge, `dx` the cell width, and `origin` the local-coordinate origin: `imap.x = 1/(sx*dx)` and `imap.z = (origin - edge0)/dx - tx/(sx*dx)`. `sy` is negative because of the y-flip, so rows come out in the right order automatically.

---

## 12. wgpu/WGSL pitfalls

1. **R32Float is not filterable** without `Features::FLOAT32_FILTERABLE`. That feature is available on Metal (macOS / Apple9+, so the M3 has it), but use manual bilinear via loads anyway: it is portable and NaN-aware. With textures, use `textureLoad`; with our storage buffer, plain indexing.
2. **Readback alignment:** `copy_texture_to_buffer` needs `bytes_per_row` aligned to 256 (`COPY_BYTES_PER_ROW_ALIGNMENT`). Strip the padding per row. `queue.write_texture` has no such requirement. Buffer writes must be multiples of 4 bytes.
3. **Default limits:** `Limits::default()`:

   | Limit | Default |
   |---|---|
   | `max_texture_dimension_2d` | 8192 |
   | `max_storage_buffer_binding_size` | 128 MiB |
   | `max_buffer_size` | 256 MiB |
   | `max_uniform_buffer_binding_size` | 64 KiB |
   | `max_bind_groups` | 4 |
   | immediates (`max_immediate_size`, the old push constants) | 0 |

   Request `adapter.limits()` (the M3 supports 16384 textures) or tile or band as described.
4. **NaN checks:** WGSL removed `isNan`/`isInf`, and Metal compiles with fast-math, so `x != x` and range tricks are unreliable. Use bit tests on raw loaded values, or CPU-side flags.
   - Constant expressions that evaluate to NaN are shader-creation errors, so do not write `bitcast<f32>(0x7fc00000u)` as a constant.
5. **Uniformity:** `textureSample` needs uniform control flow. Use `textureSampleLevel` everywhere, which is also valid in vertex shaders (scatter colormap lookup).
6. **`select()` evaluates both arms:** clamp indices (`max(i,1u)-1u`) instead of relying on short-circuiting, and watch for u32 underflow.
7. **Storage pointers:** pointer parameters in the storage address space need the `unrestricted_pointer_parameters` extension. Write one function per array.
8. **Flat varyings:** integer varyings must be `@interpolate(flat)`. The default is `max_inter_stage_shader_variables = 16`, and the line shader uses 10.
9. **Uniform layout rules:** no `vec3` in uniforms; arrays need a 16-byte stride (`array<vec4<f32>,N>`); dynamic offsets are 256-aligned. Storage bindings cannot be zero-sized, so bind a dummy.
10. **One MSAA sample count per pass:**
    - Every pipeline in a pass must match the attachment sample count and format. Counts 1 and 4 are guaranteed; 2 and 8 need adapter format features.
    - Integer targets such as an object-id buffer cannot be resolved, so hover picking needs a separate non-MSAA pass. Prefer CPU picking: invert the f64 affine and read a CPU mirror of the heatmap; nearest-point search in pixels for scatter and lines.
    - `TRANSIENT_ATTACHMENT` requires Clear or DontCare plus `StoreOp::Discard`, which means one pass per frame.
11. **Surface format:** macOS surfaces offer an sRGB and a non-sRGB BGRA8 format, among others. Pick the non-sRGB one explicitly (`caps.formats.iter().find(|f| !f.is_srgb())`). Use `CompositeAlphaMode::Opaque` for windows.
12. **P3 oversaturation:** wgpu's Metal backend has left `CAMetalLayer.colorspace = nil` for sRGB surfaces, which oversaturates on P3 displays such as the M3 MacBook's.
    - The fix (explicit `kCGColorSpaceSRGB`) was approved Sept 18, 2026 (gfx-rs/wgpu#10286) and is not in 30.0.1.
    - Until it ships, set the layer's colorspace to sRGB yourself via objc2 on the NSView's layer, or via `surface.as_hal`.
13. **Coordinate conventions:** NDC y is up while framebuffer and `@builtin(position)` y is down (top-left origin, pixel centers at +0.5). Readback is top-row first, so no flip is needed, unlike GLMakie.
14. **Primitive size:** point primitives are always 1 px and line primitives always 1 px wide. Everything else must be quads.
15. **Versions and API drift:** the latest wgpu is 30.0.1 (Aug 22, 2026); none is in the local cargo cache. Names have changed across majors (for example `Maintain` became `PollType`, push constants became immediates), so check the 30.x docs when writing host code.
16. **winit:** `EventLoop::new` works once per process, and on macOS the loop must run on the main thread. `pump_app_events` stalls during modal live-resize on macOS.

---

## 13. Key source locations

- GLMakie shaders, under `~/.julia/packages/GLMakie/hxEgI/assets/shader/`:
  - `lines.geom` (joints, truncation, extrusion, `shape_factor`, flat discard data, `process_pattern`), `lines.frag` (cap/joint SDFs, discard split, AA, dashes, colormap), `line_segment.geom`
  - `sprites.geom` (AA buffer, `viewport_from_u_scale`), `distance_shape.frag` (procedural shapes, AA radius 1/√2, outer stroke)
  - `heatmap.vert`/`heatmap.frag`, `fragment_output.frag`, `postprocessing/postprocess.frag` (FXAA mask via id high bit)
- GLMakie Julia side, under `.../GLMakie/hxEgI/src/`:
  - `plot-primitives.jl`: `generate_indices` (~line 595), Lines draw_atomic (line 685), scatter (274–408), heatmap (882–951)
  - `glshaders/lines.jl` (`sumlengths` for dash lengths), `glshaders/particles.jl`, `glshaders/image_like.jl`
  - `rendering.jl` (z-sort, per-scene viewport, clear, render order), `GLAbstraction/GLRender.jl` (`enabletransparency`), `GLAbstraction/GLRenderObject.jl` (depth/blend state)
  - `screen.jl` (ScreenConfig defaults, `on_demand_renderloop` at 1098, px_per_unit/scalefactor at 421, colorbuffer), `glwindow.jl` (FBO formats), `postprocessing.jl`
- Makie, under `~/.julia/packages/Makie/Iy6pu/src/`:
  - `float32-scaling.jl`, `makielayout/blocks/axis.jl:64` (`update_axis_camera`), `makielayout/helpers.jl:50` (`round_to_IRect2D`)
  - `backend-functionality.jl:8` (`gl_miter_limit`), `utilities/utilities.jl:600` (`linestyle_to_sdf`), `conversions.jl` (linestyles ~1200–1320, `DEFAULT_MARKER_MAP` ~1747, `edges` 321, `to_colormap` 1659)
  - `utilities/texture_atlas.jl` (atlas parameters, `rescale_marker`, `marker_to_sdf_shape`), `compute-plots.jl:160–240` (colormap, clip colors, colorrange), `theming.jl:40–60` (defaults)
- CairoMakie: `~/.julia/packages/CairoMakie/uIOIH/src/scatter.jl` (centered marker strokes), `screen.jl:93` (px_per_unit 2, pt_per_unit 0.75).

Sources:
- [wgpu on crates.io (30.0.1)](https://crates.io/api/v1/crates/wgpu)
- [winit on crates.io (0.30.13 stable, 0.31 beta)](https://crates.io/api/v1/crates/winit)
- [wgpu Features (FLOAT32_FILTERABLE etc.)](https://docs.rs/wgpu/latest/wgpu/struct.Features.html)
- [wgpu Limits](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html)
- [COPY_BYTES_PER_ROW_ALIGNMENT](https://docs.rs/wgpu/latest/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html)
- [TextureUsages (TRANSIENT_ATTACHMENT)](https://docs.rs/wgpu/latest/wgpu/struct.TextureUsages.html)
- [SurfaceColorSpace](https://docs.rs/wgpu/latest/wgpu/enum.SurfaceColorSpace.html)
- [gfx-rs/wgpu PR #10286 (Metal sRGB colorspace fix)](https://github.com/gfx-rs/wgpu/pull/10286)
- [gpuweb issue #2270 (isNan/isInf unreliable under fast-math)](https://github.com/gpuweb/gpuweb/issues/2270)
- [No wgpu fast-math knob (zenforks-cubecl issue #7)](https://github.com/imazen/zenforks-cubecl/issues/7)