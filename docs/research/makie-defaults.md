# Makie 0.24.14 default look: reference for reproduction

Path prefixes used below:
- `M/` = `~/.julia/packages/Makie/Iy6pu/src/`
- `T` = `M/makielayout/types.jl`
- `AX` = `M/makielayout/blocks/axis.jl`
- `LA` = `M/makielayout/lineaxis.jl`
- `BP` = `M/basic_plots.jl`
- `TH` = `M/theming.jl`

**How `@inherit(:fontsize, 16)` resolves.** The attribute takes the theme key if the theme has it; the second number is only a fallback. The default theme sets `fontsize = 14`, so every "size" attribute below that is written as `@inherit fontsize` comes out as **14**, not 16 (`M/makielayout/defaultattributes.jl:1-11`).

## 1. Figure and global theme (`TH:31-154`)

| key | value | cite |
|---|---|---|
| size | (600, 450), units treated as CSS px | TH:63 |
| figure_padding | 16 on all four sides. Applied as `Outside(16)` alignmode on the top GridLayout | TH:43, `M/figures.jl:122-125` |
| rowgap / colgap | 18 / 18. This overrides GridLayoutBase's fallback of 20 | TH:44-45, `M/Makie.jl:392-397`, GridLayoutBase `src/GridLayoutBase.jl:7-8` |
| fontsize | 14 | TH:40 |
| font | `:regular` | TH:33 |
| fonts | regular = "TeX Gyre Heros Makie", bold = "... Bold", italic = "... Italic", bold_italic = "... Bold Italic" | TH:34-39 |
| textcolor | `:black` | TH:41 |
| backgroundcolor | `:white` (figure scene) | TH:46 |
| colormap | `:viridis` | TH:47 |
| marker / markersize / markercolor | `:circle` / 9 / `:black` | TH:48-50 |
| markerstrokecolor / markerstrokewidth | `:black` / 0 | TH:51-52 |
| markerfont | "TeX Gyre Heros Makie" | TH:53 |
| linecolor / linewidth / linestyle | `:black` / 1.5 / nothing (solid) | TH:54-56 |
| linecap / joinstyle / miter_limit | `:butt` / `:miter` / π/3 | TH:57-59 |
| patchcolor / patchstrokecolor / patchstrokewidth | RGBf(0.4, 0.4, 0.4) / `:black` / 0 | TH:60-62 |
| CairoMakie | px_per_unit 2.0, pt_per_unit 0.75, antialias `:best` | TH:94-101 |
| GLMakie | px_per_unit and scalefactor automatic (monitor), framerate 30, render_on_demand true, vsync false, fxaa true, oit true, window title "Makie" | TH:103-134 |
| Axis / legend theme sub-dicts | empty, so block defaults below apply unchanged | TH:65-67 |

Cycling on the "black" defaults: `linecolor` and `markercolor` are black, but the palette cycle overrides them for lines, scatter and h/vlines (see §8).

## 2. Fonts on disk

The fonts live in the MakieAssets artifact (`Makie/Iy6pu/Artifacts.toml` git-tree `ad4e594b…`). Makie loads them through `assetpath("fonts", …)` (`M/Makie.jl:89`), and `to_font` maps the family names directly to these files (`M/conversions.jl:1453-1480`).

Directory: `~/.julia/artifacts/ad4e594b35357bcfafa2ed97db3137382a3f09bb/fonts/`

| file | bytes | sha256 prefix |
|---|---|---|
| TeXGyreHerosMakie-Regular.otf | 170996 | 50a5ca8991bbcfe5 |
| TeXGyreHerosMakie-Bold.otf | 172208 | 0004879bc0033fc1 |
| TeXGyreHerosMakie-Italic.otf | 177112 | 4da39282f6c526a8 |
| TeXGyreHerosMakie-BoldItalic.otf | 178612 | 4f29ae9323014c7d |

**Format.** The files are CFF-flavored OpenType (magic `OTTO`). The font loader must support CFF outlines. The only tables present are CFF, OS/2, cmap, head, hhea, hmtx, maxp, name and post. There is **no GPOS or kern table**, so plain advance-width layout matches Makie.

**Metrics** (identical in all four styles, read from head/hhea/OS2):

| metric | value |
|---|---|
| unitsPerEm | 1000 |
| hhea ascender / descender / lineGap | 947 / -218 / 0 (typo metrics are the same) |
| xHeight | 524 (regular/italic), 540 (bold) |
| capHeight | 729 |

Makie uses `ascender/upem` and `descender/upem` (FreeTypeAbstraction `src/types.jl:254-255`).
- A text line box is 1.165 em. At 14 px that is 16.31 px (ascender 13.258, descender 3.052).
- Line height is `font.height/upem × lineheight` = 1.165 em (`M/layouting/text_layouting.jl:126`).

**Glyph coverage.** The fonts include U+2212 "minus" and "multiply" (×), both of which the tick labels use.

**License.** Section "# TeX Gyre Heros" at `…/fonts/LICENSES.md:360-389`. It is the GUST Font License v1.0, which is LPPL 1.3c or later, with a *requested* (not legally required) rename for derived works.

**Other bundled fonts in the same directory:** DejaVuSans, DejaVuSansMono, FiraMono-Medium, NotoSans, NotoSansSymbols, NotoEmoji, Helvetica.otf, hack_regular.

## 3. Axis (`T:305-760`)

### 3.1 Attribute values

| group | values | cite |
|---|---|---|
| title | `""`, font `:bold`, size 14, color textcolor, titlegap **4**, titlealign `:center`, lineheight 1, visible | T:343-362 |
| subtitle | font `:regular`, size 14, subtitlegap 0 | T:367-379 |
| x/y label | font `:regular`, size 14, color textcolor; **xlabelpadding 3, ylabelpadding 5**; rotation automatic (y label: +π/2) | T:381-403 |
| ticklabels | font `:regular`, size 14, color textcolor; **xticklabelpad 2, yticklabelpad 4**; rotation 0; align automatic (x = (:center,:top), y = (:right,:center)); ticklabelspace automatic | T:405-435, LA:93-132 |
| ticks | size **5**, width **1**, tickalign **0** (outward), color RGBf(0,0,0), which is *not* inherited from textcolor; visible; mirrored false | T:437-459 |
| minor ticks | **not visible**, align 0, size **3**, width 1, color `:black`, locator `IntervalsBetween(2)` | T:684-720 |
| spines | spinewidth **1**, all four visible, all `:black`, trimspine false | T:473, 507-521, 625-630 |
| grid | x and y visible, width **1**, color **RGBAf(0,0,0,0.12)**, style nothing (solid) | T:475-489 |
| minor grid | **off**, width 1, color **RGBAf(0,0,0,0.05)** | T:491-505 |
| backgroundcolor | `:white` | T:632 |
| autolimit margins | x and y (0.05, 0.05), applied in *scaled* space (for example log space) | T:554-556, AX:775-803 |
| aspect / autolimitaspect / limits | nothing / nothing / (nothing, nothing) | T:540, 655, 676 |
| xticks / yticks | `automatic`, which means `WilkinsonTicks(5, k_min=3)` | T:569, 593, LA:599 |
| x/y tickformat | `automatic`, which means `format_ticks_auto` | T:580, LA:815 |
| x/y scale | `identity` | T:739, 758 |
| reversed | xreversed = yreversed = false | T:680-682 |
| axis position | xaxisposition `:bottom`, yaxisposition `:left`, flip_ylabel false | T:618-634 |
| layout | alignmode `Inside()`, tellwidth/tellheight true, width/height nothing | T:545-552, 678 |
| empty-axis limits | (0, 10) for identity, 10^0 to 10^3 for log, (0, 100) for sqrt | AX:1432-1437 |

**WilkinsonTicks parameters** (`M/makielayout/ticklocators/wilkinson.jl:16-24, 37-54`):
- k_ideal 5, k_min 3 (the Axis override; the constructor default is 2), k_max 10
- Q = [(1,1), (5,0.9), (2,0.7), (2.5,0.5), (3,0.2)]
- weights: granularity 1/4, simplicity 1/6, coverage 1/3, niceness 1/4
- calls `PlotUtils.optimize_ticks(...; extend_ticks=false, strict_span=true, span_buffer=nothing)` (PlotUtils `src/ticks.jl:141-156`)
- only ticks inside the limits are kept (LA:193, 212)

**Log scale ticks** (LA:632-646):
- Defaults to `LogTicks(WilkinsonTicks(5, k_min=3))`, run on the log-transformed limits. Exponents can therefore be fractional.
- Labels are `rich("10", superscript(exp; offset=(0.1, 0)))`, with the exponent formatted by `format_ticks_plain`.

**Minor tick values** (LA:912-947):
- `IntervalsBetween(n)` subdivides *linearly in data space* between adjacent majors, with mirror = true (also extends past the first and last major).
- On log axes the step outside the first and last major is computed from the neighbouring decade.
- Makie has no automatic 2..9 decade minors. You need `IntervalsBetween(9)` plus `minorticksvisible = true`.

**Tick label formatter** (`M/tick_format.jl`):
- Plain style unless `|log10(max(ticks) - min(ticks))| > 4`; in that case the scientific rich form `a×10ⁿ` is used (tick_format.jl:92-96, 100-118).
- Plain precision is a uniform number of decimals: the Showoff heuristic on the Float32 Ryu shortest representation (19-32).
- The minus sign is U+2212 (14, 47).
- Scientific mantissas drop the `.0…` padding only if all of them are integral (106-117).
- In rich text, a superscript is 0.66 × size with baseline +0.4 × size; a subscript is 0.66 × size with baseline -0.25 × size (`M/basic_recipes/text.jl:1132-1166`).

### 3.2 Geometry (reproduce exactly)

Draw order (z) and line construction:
- Background poly is at z = -100 (AX:196-197).
- Grid linesegments are at z = -10 and span the full viewport (AX:206-240, 465-487).
- Plots are at z = 0. Ticks are at +10 and spines at +20 (LA:285, 500; AX:455, 463).
- Spine lines are extended by 0.5 × spinewidth at both ends so corners close (LA:60-71, AX:378-410).

Ticks (LA:170-189):
- Each tick starts at `edge ∓ 0.5·spinewidth + tickalign·ticksize` and runs outward by ticksize.
- Tick space is `max(0, ticksize·(1 - tickalign))` = 5 (LA:338-341).

Tick labels (LA:137-168):
- Offset from the spine centre line by `spinewidth + tickspace + ticklabelpad`, which is 8 px for x and 10 px for y.

Axis labels (LA:343-391):
- Gap = `spinewidth + tickspace + ticklabelspace + ticklabelpad + labelpadding`.
- x label: align (:center, :top).
- y label: rotated +π/2, align (:center, :bottom), centred on the spine.

Protrusion (LA:23-46):
- `tickspace + (ticklabelspace + ticklabelpad) + (label extent + labelpadding)`.
- spinewidth is *not* included here, so the label overhangs the protrusion by about 1 px (a Makie quirk).
- The title adds `textheight + titlegap`, and also subtitle height + subtitlegap when a subtitle is set (AX:113-152).

Title position (AX:90-111, 526-537):
- `top(viewport) + titlegap (+ subtitle height)`, align (titlealign, :bottom).

Text boxes and alignment:
- Text bbox is `[0, hadvance] × [descender, ascender]` per glyph (`M/layouting/text_boundingbox.jl:62-68`).
- Vertical align `:top`/`:bottom` uses the ascender of the first line and the descender of the last line, not the ink bounds (`M/basic_recipes/text.jl:1003-1026`).

Worked defaults at 14 px (text height 16.31):

| protrusion | value |
|---|---|
| x axis, no label | 5 + 16.31 + 2 = 23.31 |
| x axis, with label | 23.31 + 16.31 + 3 |
| y axis | 5 + maxlabelwidth + 4 (+ 16.31 + 5 with label) |
| title | 16.31 + 4 |

### 3.3 Limits

- A degenerate (zero-width) range becomes (-1, 1) at 0. Otherwise it becomes ±|value| in scaled space, so a constant 5 gives (0, 10) (AX:786-801).
- Plotting a `Heatmap` or `Image` into an Axis calls `tightlimits!`. This sets **both** autolimitmargins to (0, 0) permanently for that axis (AX:751-752, `M/figureplotting.jl:583`, `M/makielayout/helpers.jl:101-105`).
- `hlines` count only toward y limits, `vlines` only toward x, and `ablines` toward neither (`M/basic_recipes/hvlines.jl:73-81`, `M/basic_recipes/ablines.jl:30`).

### 3.4 Interactions

Registered in AX:53-59 and implemented in `M/makielayout/interactions.jl`.

| action | behaviour | cite |
|---|---|---|
| Rectangle zoom | Left drag, no modifier needed (modifier = true). Holding `x` or `y` restricts the zoom to that dimension. The *outside* of the selection is shaded `(:black, 0.2)` at z = 1000. Selection is clamped to the visible limits. | interactions.jl:159-218, T:212, 762-782 |
| Limit reset | Ctrl + left click calls `reset_limits!` (back to user limits or auto). Ctrl + Shift + left click calls `autolimits!`. | interactions.jl:226-241 |
| Scroll zoom | `ScrollZoom(0.1, 0.2)`: factor `z = 0.9^scroll_y`, anchored at the cursor. `x`/`y` keys restrict it; zoombutton is `true` (always on). | interactions.jl:243-310 |
| Drag pan | `DragPan(0.2)` on the **right** mouse button. `x`/`y` keys restrict it. | interactions.jl:312-373, T:606-616 |
| Label jitter guard | While zooming or panning, ticklabelspace is frozen at its current value and restored 0.2 s after the last event. | AX:1059-1081 |
| Lock flags | xpanlock, ypanlock, xzoomlock, yzoomlock all false; xrectzoom and yrectzoom true | T:461-471 |

**Hover readout (DataInspector)** is **not enabled by default**; you call `DataInspector(fig)`.
- Settings: range 10, offset 10, indicator `:red` with width 2 (`M/interaction/inspector.jl:248-266`).
- Tooltip: white background, outline `:black` with width 1, textpadding (5, 5, 3, 3), triangle 7, placement `:above`, fontsize 14 (`M/basic_recipes/tooltip.jl`).
- Text formats (inspector.jl:4-8, 28, 39-43, 625-631):
  - points: `"x: %0.6f\ny: %0.6f"`
  - heatmap: `H[i, j] = %0.3f`
  - image: `img[x, y] = %0.3f`

## 4. Legend (`T:1499-1698`, `M/makielayout/blocks/legend.jl`)

| key | value | cite |
|---|---|---|
| framevisible / framecolor / framewidth | true / `:black` / 1 | T:1553-1557 |
| backgroundcolor | `:white`. The background poly's `visible` is tied to framevisible, so no frame also means no background | T:1551, legend.jl:69-75 |
| padding (L,R,B,T) | (6, 6, 6, 6), applied as `Outside(padding)` on the inner grid | T:1545, legend.jl:78 |
| margin | (0,0,0,0) for `Legend`; **(6,6,6,6) for `axislegend`** | T:1547, legend.jl:1131-1135 |
| patchsize | (20, 20) | T:1559 |
| patchcolor / patchstrokecolor / patchstrokewidth | `:transparent` / `:transparent` / 1 | T:1561-1565 |
| rowgap / colgap / patchlabelgap | 3 / 16 / 5 | T:1571-1575 |
| labelsize / labelfont / labelcolor / labelhalign / labelvalign | 14 / `:regular` / textcolor / `:left` / `:center` | T:1533-1543 |
| title | font `:bold`, size 14, halign `:center`, position `:top`; titlegap 8; groupgap 16 | T:1519-1531, 1688-1690 |
| orientation / nbanks | `:vertical` / 1 | T:1569, 1686 |
| tellwidth / tellheight | automatic: vertical legend gives (true, false); horizontal gives (false, true) | T:1515-1517, legend.jl:48-49 |
| halign / valign | `:center` / `:center` | T:1507-1509 |
| element geometry | line points (0,0.5)-(1,0.5) (full-width line); marker at (0.5,0.5); poly covers the unit square; legend `linestyle` default `:solid`; alpha 1 | T:1578-1612, 1684 |
| element values | Taken from the plot: color, linewidth, linestyle, marker, markersize, stroke. A Band legend patch is filled with the band colour and no stroke. | legend.jl:723-786 |
| `axislegend` | `position = :rt`. First letter is l/r/c (halign), second is t/b/c (valign). The legend is placed inside `ax.scene.viewport`. | legend.jl:1127-1160 |
| click behaviour | Left click toggles that entry's plots. Right click toggles all. Middle click does a synchronized toggle. Hidden entries get an RGBAf(0.9,0.9,0.9,0.65) shade. | legend.jl:217, 360-380 |

## 5. Colorbar (`T:818-936`, `M/makielayout/blocks/colorbar.jl`)

| key | value | cite |
|---|---|---|
| size | **12**, the bar thickness. Autosize is (12, nothing) when vertical | T:934, colorbar.jl:171-177 |
| vertical / flipaxis | true / **true**, so ticks and label are on the **right** (top when horizontal) | T:888-890 |
| label | `""`, size 14, font `:regular`, labelpadding 5. Rotation automatic gives +π/2 (reads bottom to top), align (:center, :top) on the right; flip_vertical_label false | T:822-834, 892, LA:370-408 |
| ticks / tickformat | automatic, i.e. Wilkinson(5, k_min=3) and `format_ticks_auto` | T:848-850 |
| ticklabels | size 14, pad **3**, rotation 0, align automatic ((:left,:center) on the right side) | T:838-864 |
| ticks | size 5, width 1, align 0, color black | T:844-860 |
| minor ticks | off, size 3, width 1, `IntervalsBetween(5)` | T:922-932 |
| spine | spinewidth 1, black. It is one closed polyline around the bar (including any clip triangles) and uses topspinecolor. The LineAxis spine itself is invisible. | T:866-882, colorbar.jl:376-402, 446 |
| gradient | nsteps 100. Drawn as an `image!` of the 99 midpoints of `LinRange(lo, hi, 100)` with interpolation on (image default). Categorical colormaps use `heatmap!` instead. | T:919, colorbar.jl:288-326 |
| lowclip / highclip | Triangles are drawn only if the value is set (not automatic). Triangle height = bar width × sin(π/3). | colorbar.jl:243-259, 328-374 |
| tellwidth / tellheight | true / true; alignmode `Inside()` | T:898-900, 917 |
| `Colorbar(pos, plot)` | Copies the plot's ColorMapping (colormap, colorrange, lowclip, highclip, scale). Passing any of those as kwargs is an error. | colorbar.jl:141-166 |
| default limits (no plot) | (0, 1) | colorbar.jl:190 |

## 6. Label block (`T:938-975`)

| key | value |
|---|---|
| text | "Text" |
| fontsize | 14 |
| font | `:regular` |
| color | textcolor |
| justification | `:center` |
| lineheight | 1 |
| halign / valign | `:center` / `:center` |
| rotation | 0 |
| padding | (0, 0, 0, 0) |
| width / height | Auto |
| tellwidth / tellheight | true / true |
| word_wrap | false |

## 7. Plot defaults

The shared colormap mixin gives every colormapped plot these values (BP:154-178):
- colormap: inherited, `:viridis`
- colorscale: identity
- colorrange: automatic, meaning `distinct_extrema_nan` (NaN is ignored; lo == hi becomes ±0.5) (`M/layouting/data_limits.jl:108-111`)
- lowclip / highclip: automatic, meaning the first / last colormap colour (`M/compute-plots.jl:187-191`)
- nan_color: `:transparent`
- alpha: 1.0, which multiplies with the colour's own alpha

| plot | defaults | cycle | cite |
|---|---|---|---|
| Lines | color linecolor, linewidth 1.5, linestyle nothing, linecap `:butt`, joinstyle `:miter`, miter_limit π/3, fxaa false | `[:color]` | BP:458-496 |
| LineSegments | same as Lines, without joinstyle | `[:color]` | BP:506-533 |
| Scatter | color markercolor, marker `:circle`, markersize 9, strokecolor `:black`, strokewidth 0, glowwidth 0, rotation Billboard(), markerspace `:pixel`, marker_offset 0, fxaa false | `[:color]` only; marker does **not** cycle | BP:589-642 |
| ScatterLines | Lines attributes plus Scatter's (marker `:circle`, markersize 9, strokewidth 0); color linecolor; markercolor, markercolormap and markercolorrange are automatic, meaning same as the line | `[:color]` | `M/basic_recipes/scatterlines.jl:6-24, 31-47` |
| Heatmap | interpolate **false**; viridis; nan_color transparent; tightens axis limits | none | BP:298-307, AX:752 |
| Image | interpolate **true**; colormap **[:black, :white]** (not viridis); fxaa false; tightens axis limits | none | BP:248-267 |
| Hist | bins 15 (edges `range(min, nextfloat(max), bins+1)`), normalization `:none` (options `:pdf`, `:density`, `:probability`, `:none`), weights automatic, scale_to nothing, **gap 0**, color patchcolor, strokewidth 0, fillto automatic | `[:color => :patchcolor]`, inherited from BarPlot | `M/stats/hist.jl:111-170` |
| BarPlot | gap **0.2**, dodge_gap 0.03, width automatic = min diff of unique x (1 if a single bar), fillto automatic = 0 (log axis: min positive y / 2), offset 0, direction `:y`, strokewidth 0, strokecolor `:black`, color patchcolor, label_offset 5, label_size 14 | `[:color => :patchcolor]` | `M/basic_recipes/barplot.jl:43-130, 21-35, 145-165, 333-338` |
| Band | Mesh attributes: color patchcolor, shading NoShading, direction `:x`, strokewidth 0, strokecolor `:black`, interpolate true | `[:color => :patchcolor]` | `M/basic_recipes/band.jl:10-20`, BP:551-562 |
| HLines / VLines | LineSegments attributes; xmin/ymin 0, xmax/ymax 1 (fractions of the axis) | `[:color]` | `M/basic_recipes/hvlines.jl:9-31` |
| ABLines | LineSegments attributes; spans the current x limits | `[:color]` | `M/basic_recipes/ablines.jl:7-9` |
| HSpan / VSpan | Poly attributes | `[:color => :patchcolor]` | `M/basic_recipes/hvspan.jl:11-35` |
| Poly | color patchcolor, strokecolor `:black`, strokewidth 0, linestyle nothing, shading false | `[:color => :patchcolor]` | BP:833-889 |
| Text | color textcolor, font `:regular`, fontsize 14, align **(:left, :bottom)**, justification automatic, lineheight 1, rotation 0, strokewidth 0, offset (0,0), markerspace `:pixel`, word_wrap_width -1 | none | BP:711-751 |

**Linestyle patterns** are in units of linewidth (cumulative dash boundaries, `M/conversions.jl:1224-1297`):
- normal gaps: dot gap 2, dash gap 3 (dense 1/2, loose 4/6); dash = 3, dot = 1
- `:dash` gives [0, 3, 6]
- `:dot` gives [0, 1, 3]
- `:dashdot` gives [0, 3, 6, 7, 10]
- `:dashdotdot` gives [0, 3, 6, 7, 9, 10, 13]

## 8. Cycling rules (`M/compute-plots.jl:609-672, 800-827`, `T:10-42`)

- The cycle index is counted **per plot function** within the axis scene. A separate counter runs for lines, scatter, barplot, and so on.
- A plot advances the counter only if it did **not** set the cycled attribute explicitly (sentinel `:cycled`).
- `Cycled(i)` picks palette entry i.
- `Cycle(...; covary = false)` by default. When several attributes cycle, this walks the Cartesian product (`CartesianIndices`, first palette fastest).
- `:color => :patchcolor` means the colour is taken from the `patchcolor` palette.

## 9. Palette, colormap and markers

**Wong colours** (`TH:5-16`), in cycle order and all with alpha 1:

| # | name | RGB | hex |
|---|---|---|---|
| 1 | blue | (0, 114, 178) | #0072B2 |
| 2 | orange | (230, 159, 0) | #E69F00 |
| 3 | green | (0, 158, 115) | #009E73 |
| 4 | reddish purple | (204, 121, 167) | #CC79A7 |
| 5 | sky blue | (86, 180, 233) | #56B4E9 |
| 6 | vermilion | (213, 94, 0) | #D55E00 |
| 7 | yellow | (240, 228, 66) | #F0E442 |

**Palette** (`TH:18-27`):
- color: the Wong colours above.
- patchcolor: `lerp(bg, c, 0.8)` = 0.2·bg + 0.8·c. On white this gives #338EC1, #EBB233, #33B18F, #D694B9, #78C3ED, #DD7E33, #F3E968. So bars, hist, band and poly are **opaque, lightened** Wong colours by default.
- marker: [:circle, :utriangle, :cross, :rect, :diamond, :dtriangle, :pentagon, :xcross]
- linestyle: [nothing, :dash, :dot, :dashdot, :dashdotdot]
- side: [:left, :right]
- `theme_dark` regenerates the palette with bg = :gray10.

**Default colormap.** viridis comes from ColorSchemes, 256 RGB entries at `~/.julia/packages/ColorSchemes/3BWhh/data/matplotlib.jl:772-1027`. The first is (0.267004, 0.004874, 0.329415) and the last is (0.993248, 0.906157, 0.143936). Symbol lookup is in `M/conversions.jl:1638-1656`.

**Marker geometry** (needed for pixel matching):
- Symbol markers are BezierPaths scaled by `size_factor = 0.75` (`M/conversions.jl:1725-1734`, literal map at 1753+; base shapes at `M/bezier.jl:813-882`).
- They render through the SDF path (`M/utilities/texture_atlas.jl:443`). Path coordinates are multiplied by markersize (`M/utilities/texture_atlas.jl:613-620`; CairoMakie `src/scatter.jl:90, 386-399`).
- Extents in units of markersize:

| marker | geometry |
|---|---|
| `:circle` | radius 0.3525, so **diameter = 0.705 × markersize = 6.35 px at 9** |
| `:rect` | half-side 0.315718 (side 0.631) |
| `:diamond` | half-diagonal 0.4465 |
| `:utriangle` / `:dtriangle` | half-width 0.36375; apex at +0.485 and base at -0.2425 (centroid-centred) |
| `:cross` / `:+` | arm half-length 0.375, half-thickness 0.1245 |
| `:xcross` | cross rotated 45° |
| `:pentagon` | circumradius 0.375 |

- `marker = Circle` (the type, not the symbol) is an exact circle of diameter = markersize.

## 10. Themes (`M/themes/`)

- **theme_minimal** (`theme_minimal.jl:1-56`)
  - Axis: transparent background, all grids off, only left and bottom spines, ticks and minor ticks hidden, x/y labelpadding 3.
  - Legend: no frame, padding 0.
  - Colorbar: ticks hidden, spinewidth 0, ticklabelpad 5.
- **theme_light** (`theme_light.jl:1-40`)
  - textcolor `:gray50`.
  - Axis: transparent background, grid (:black, 0.07), all spines hidden, ticks and minor ticks hidden, labelpadding 3.
  - Legend and Colorbar: same as minimal.
- **theme_dark** (`theme_dark.jl:1-43`)
  - backgroundcolor `:gray10`, textcolor `:gray45`, linecolor `:gray60`, palette regenerated against gray10 (changes patchcolor).
  - Axis: transparent background, grid (:white, 0.09), no spines, no ticks, labelpadding 3.
  - Legend and Colorbar: same as minimal.
  - Tick and spine colours stay hard-coded black in the Axis defaults (T:453-455, 515-521) but are hidden in this theme.
- **Others:**
  - theme_black: black background, white text, lines and spines, grid RGBAf(1,1,1,0.16).
  - theme_latexfonts: swaps only `fonts` to the Computer Modern texfont set.

## 11. Gotchas for faithful reproduction

- Theme fontsize 14 applies to titles too: the title is bold 14, not 16.
- There are no minor ticks or minor grid by default. The log axis also has no 2..9 minors.
- Bars and hist use the *lightened* patchcolor palette, and the hist gap is 0 while the barplot gap is 0.2.
- Heatmap and Image permanently zero the axis autolimit margins.
- Image's default colormap is greyscale, not viridis.
- Scatter cycles colour only (not marker), and the `:circle` symbol is 0.705 × markersize wide.
- The legend background is hidden whenever the frame is hidden.
- The Colorbar default is on the right with its ticks on the right (flipaxis = true), 12 px wide.
- In the PNG export model, 600×450 units at px_per_unit 2 give a 1200×900 image. In the vector model, pt_per_unit 0.75 gives 450×337.5 pt.