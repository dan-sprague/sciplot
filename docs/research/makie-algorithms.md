# Makie algorithms to port to ezviz: ticks, label formatting, autolimits, layout, interaction, legend, colorbar, hist/bar/band

I read everything below from the local sources. I did not run Julia, because loading packages can write compile caches and logs, and this task was read-only. The tick test vectors in §1.4 come from a line-by-line Python port of the PlotUtils code that I ran locally. That port reproduces PlotUtils' own unit tests. Before using them as golden tests, confirm them in Julia (§1.4 has the command).

**Path aliases**
- `PU` = `~/.julia/packages/PlotUtils/J9gzB/src`
- `MK` = `~/.julia/packages/Makie/Iy6pu/src`
- `ML` = `MK/makielayout`
- `GLB` = `~/.julia/packages/GridLayoutBase/K7sJt/src`
- `SB` = `~/.julia/packages/StatsBase/2Znv8/src`
- Font: `~/.julia/artifacts/ad4e594b35357bcfafa2ed97db3137382a3f09bb/fonts/TeXGyreHerosMakie-{Regular,Bold,Italic,BoldItalic}.otf`

**Coordinate convention**: Makie layout and pixel space use a y-up, bottom-left origin. Grid rows are numbered from the top. Winit and wgpu window coordinates are y-down, so flip once, at the boundary.

---

## 1. Ticks

### 1.1 How the Axis gets its ticks
- Each axis side is a `LineAxis`. On any change to limits, ticks, tickformat or scale it calls `get_ticks(dim_convert, ticks, scale, tickformat, vmin, vmax)`. See `ML/lineaxis.jl:440-445`.
  - With no dim conversion this falls through to `get_ticks(ticks, scale, formatter, vmin, vmax)` (`MK/dim-converts/dim-converts.jl:76-78`).
- The generic path computes values, then labels: `tickvalues = get_tickvalues(ticks, scale, vmin, vmax)` and `labels = get_ticklabels(formatter, tickvalues)` (`ML/lineaxis.jl:592-596`).
- Default for a linear axis (`automatic` ticks, `identity` scale): `WilkinsonTicks(5, k_min = 3)` (`ML/lineaxis.jl:599`).
  - Unknown scales fall back to the same (`:602`).
- Default formatter: `format_ticks_auto` (`ML/lineaxis.jl:817`, see §2).
- `WilkinsonTicks(k_ideal; k_min=2, k_max=10, Q, weights...)` checks `0 < k_min <= k_ideal <= k_max` (`ML/ticklocators/wilkinson.jl:16-33`).
  - It calls `PlotUtils.optimize_ticks(Float64(vmin), Float64(vmax); extend_ticks=false, strict_span=true, span_buffer=nothing, k_min, k_max, k_ideal, Q, weights)` and keeps only the tick vector (`:37-53`).
  - It does **not** pass `scale`, so `is_log_scale = false` even when it is used inside `LogTicks`.
- Other tick specs:
  - A vector of numbers (`ML/lineaxis.jl:797`).
  - A `(values, labels)` tuple, which errors if the lengths differ (`:607-614`).
  - A function `(vmin, vmax) -> values` or `-> (values, labels)` (`:616-625`).
  - `LinearTicks(n)` (`:789`), `MultiplesTicks` (`:833-845`).
- **Important:** the default tick count does not depend on the axis size in pixels (`k_ideal` is fixed at 5). Ticks depend only on the limits. So protrusions do not depend on the layout, and one layout pass is enough (§4.8).

### 1.2 `optimize_ticks` (Wilkinson, extended as in Gadfly/PlotUtils): `PU/ticks.jl:141-349`

```
const Q  = [(1.0,1.0),(5.0,0.9),(2.0,0.7),(2.5,0.5),(3.0,0.2)]   // (step mantissa, niceness score), ORDER MATTERS
weights: granularity=1/4, simplicity=1/6, coverage=1/3, niceness=1/4
Makie: k_min=3, k_max=10, k_ideal=5, strict_span=true, extend_ticks=false, span_buffer=None, base=10

fn optimize_ticks(xmin, xmax) -> (ticks, vmin, vmax):
  rtol = 1000*eps(f64)
  if isapprox(xmin, xmax, rtol) { return fallback_ticks(...) }          // :157-160
  for pass in 1..=2 {                                                     // :170-201
     strict = if pass==1 {strict_span} else {false}
     (hs, best, bmin, bmax) = typed(xmin, xmax, strict)
     if hs == -inf { if strict { warn("No strict ticks found"); continue } else { return fallback_ticks(...) } }
     return (best, bmin, bmax)
  }

fn bounding_order_of_magnitude(x, base) -> i32:     // :9-30  smallest b with x <= base^b (bisection)
  a=1; while x < base^a { a-=1 }
  b=1; while x > base^b { b+=1 }
  while a+1 < b { c = (a+b)/2 /*trunc toward 0*/; if x < base^c {b=c} else {a=c} }
  return b

fn typed(xmin, xmax, strict):                       // :205-349
  xspan = xmax - xmin
  z = bounding_order_of_magnitude(xspan, 10)
  num_digits = bounding_order_of_magnitude(max(|xmin|,|xmax|), 10) + max_postdecimal_digits(Q)   // =1 for default Q (2.5)
  high = -inf; best = []; bmin=xmin; bmax=xmax
  while 2*k_max * 10^(z+1) > xspan {                 // z DEScends
    sig = max(1, num_digits - z)
    for k in k_min..=2*k_max {                       // k ascending
      for (q, qscore) in Q {                         // Q order
        tickspan = q * 10^z;  if tickspan < eps { continue }
        span = (k-1)*tickspan; if span < xspan { continue }
        r_f = (xmax - span)/tickspan; if !finite { continue }
        r = ceil(r_f) as i64
        nice_scale = true   // (log-scale branch: qscore=0 unless tickspan integer; unused by Makie)
        while r*tickspan <= xmin {                   // r ascending
          S = [(r+i)*tickspan for i in 0..k]
          S[0]   = vmin = round_sigdigits(S[0], sig)     // Julia round(x; sigdigits, base=10), ties-to-even
          S[k-1] = vmax = round_sigdigits(S[k-1], sig)
          if strict { vmin=max(vmin,xmin); vmax=min(vmax,xmax); buf=0*(vmax-vmin)
                      S.retain(|s| vmin-buf <= s <= vmax+buf) }
          len = S.len()
          has_zero = r <= 0 && |r| < k               // uses UNFILTERED run (0 may be filtered out!)
          s = if has_zero && nice_scale {1} else {0}
          g = if 0 < len < 2*k_ideal { 1 - |len-k_ideal| / k_ideal } else {0}
          c = if len > 1 { 1.5*xspan / ((len-1)*tickspan) } else {0}
          score = ((gw*g + sw*s) + cw*c) + nw*qscore  // left-to-right f64 sum
          if strict && span > xspan { score -= 10000 }
          if span >= 2*xspan        { score -= 1000 }
          if score > high && k_min <= len <= k_max {  // STRICT '>' : first found wins ties
             high=score; best=S; bmin=vmin; bmax=vmax }
          r += 1
        }
      }
    }
    z -= 1
  }
  return (high, best, bmin, bmax)

round_sigdigits(x, n): if x==0 {0} else { h = 1 + floor(log10|x|); d = n - h;
   if d>=0 { round_half_even(x*10^d)/10^d } else { round_half_even(x/10^-d)*10^-d } }

fallback_ticks(xmin,xmax,k_min,k_max,strict):           // :39-48
   if !strict && xmin≈xmax { xmin=prevfloat(xmin); xmax=nextfloat(xmax) }
   if k_min != 2 && finite { linspace(xmin,xmax,k_min) } else { [xmin,xmax] }
```

**Port gotchas**
- **The -10000 penalty flattens scores.** Every candidate whose run is longer than the data range gets −10000. After that subtraction, differences of about 1e-16 disappear, and near-ties are decided by loop order: z descending, then k ascending, then Q order, then r ascending, with a strict `>`. Keep that order and use f64 arithmetic exactly as written, or the results will differ.
- **Coverage rewards shorter tick runs.** `c = 1.5·xspan/effective_span` goes up as the ticks cover less. This is why odd outputs such as `[1.2, 1.5, 1.8]` can win.
- **Tick values can have float noise** (e.g. `3*0.1`). The label formatter (§2) handles this by rounding through the shortest f32 representation.

### 1.3 `LinearTicks` (alternative locator, not the default): `ML/ticklocators/linear.jl`
This is matplotlib's MaxNLocator:
- steps `(1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10)`, extended by `_staircase` (`:13-25`);
- `scale_range` offset threshold 100 (`:1-11`);
- final rounding `round(vals, digits = max(0, -floor(log10(min diff)) + 1))` (`:144-145`).

### 1.4 Test vectors (from my Python port; confirm in Julia before relying on them)
The Python port reproduces PlotUtils' own tests:
- `optimize_ticks(-1, 2) == [-1, 0, 1, 2]`
- `optimize_ticks(1e11-1, 1e11+2) == 1e11 .+ (-1:2)`

Both are in `PU/../test/runtests.jl:125,129`.

Makie settings (k_min=3), input = final limits:

| limits | ticks |
|---|---|
| (0.55, 10.45) [lines(1:10)] | 2, 4, 6, 8, 10 |
| (0.5, 10.5) [heatmap 10×10] | 2, 4, 6, 8, 10 |
| (-1.1, 1.1) | -1, -0.5, 0, 0.5, 1 |
| (-0.5, 10.5) | 0, 5, 10 (exact three-way tie with 2.5 and 2 steps; the first found wins) |
| (-0.04, 1.04) | 0, 0.5, 1 |
| (-3.95, 104.95) | 0, 50, 100 |
| (-0.15, 3.15) (log10 of 1..1000 plus margins) | 0, 1, 2, 3 |
| (0.95, 2.05) | 1.2, 1.5, 1.8 |
| (12.3, 98.7) | 25, 50, 75 |

To confirm, and ideally to generate a fixtures file for `cargo test`, run: `julia -e 'using PlotUtils; @show optimize_ticks(-0.5,10.5; k_min=3,k_max=10,k_ideal=5)'`.

### 1.5 Log-scale ticks (`LogTicks`)
- `get_ticks(::Automatic, scale::Union{log10,log2,log}, fmt, vmin, vmax)` uses `LogTicks(WilkinsonTicks(5, k_min=3))` (`ML/lineaxis.jl:632-635`). Then (`:638-646`):

```
ticks_scaled = optimize_ticks(log(vmin), log(vmax))        // plain Wilkinson on exponents; NOT forced to integers
ticks        = base.^ticks_scaled
labels       = rich(base_str, superscript(format_ticks_plain(ticks_scaled), offset=(0.1,0)))  // "10" + sup("2")
```

- `base_str` is `"10"`, `"2"` or `"e"` (`:627-630`).
- Exponents are formatted with `format_ticks_plain` (§2), so they come out as `0, 1, 2`.
- Non-integer exponents can happen. For 1..10 data the exponent ticks are `0, 0.5, 1`, labelled `10^0.0, 10^0.5, 10^1.0` (uniform precision, §2). This is real Makie behaviour.
- Positions are computed in scaled space: `frac = (scale(t) - scale(lo)) / (scale(hi) - scale(lo))` (`:216-220`).
- `get_tickvalues(l::LogTicks, scale, vmin, vmax)` is the same without labels (`:805-808`).
- A custom formatter on a log axis skips the rich `10^n` labels.

**ezviz note (my suggestion, not Makie behaviour):**
- When the log range spans at least 1 decade, force integer exponent steps. Use step `ceil(n_decades / k_ideal)` or run Wilkinson restricted to integer `tickspan`, i.e. Makie's unused `is_log_scale` branch, which sets qscore to 0 for non-integer spans.
- Below 1 decade, fall back to linear Wilkinson in data space with plain labels.

### 1.6 Minor ticks
- Axis defaults: `xminorticks = yminorticks = IntervalsBetween(2)`, with minor ticks and minor grid hidden (`ML/types.jl:684-720`).
  - Minor gridlines exist but are hidden: `xminorgridvisible=false`, color `(0,0,0,0.05)`, width 1 (`:491-505`).
  - Minor values are only computed if minor ticks or minor grid are visible (`ML/lineaxis.jl:459-464`).
- `IntervalsBetween(n, mirror=true)` requires `n >= 2` and produces `n-1` minor ticks per major interval (`ML/types.jl:176-184`).
- **Linear** (`ML/lineaxis.jl:880-910`):

```
if majors.len < 2 { return [] }
if mirror { step=(t[1]-t[0])/n; prepend (t[0]-step) down to vmin by -step }
for each (lo,hi): step=(hi-lo)/n; push lo+step*i for i in 1..n
if mirror { step=(t[last]-t[last-1])/n; append t[last]+step .. vmax }
then filter to within limits (is_within_limits, ±100 eps)
```

- **Log** (`:913-948`): minor spacing inside each major interval is linear in data space. Mirrored intervals use the virtual major `inv(scale(t0) - (scale(t1) - scale(t0)))`, i.e. `t0/10` for consecutive decades.
  - With `IntervalsBetween(9)` and majors at consecutive decades you get the classic 2..9 × 10^k.
  - Makie's actual default, `IntervalsBetween(2)`, puts one minor tick at 5.5 between 1 and 10.
  - The docs examples use `IntervalsBetween(5)` (`ML/blocks/axis.jl:1804-1810`).
  - **ezviz:** add a `LogMinor` default. For each decade `d` from `floor(log10 lo)` to `ceil(log10 hi)`, place `k·10^d` for k = 2..9 inside the limits. When majors skip decades, place minors only at the skipped decades.

### 1.7 Filtering, positions and reversed axes
- Tick values outside the limits (±100·eps) are dropped (`ML/lineaxis.jl:193, 212-214`).
- Pixel position: `px = extent0 + (extent1 - extent0)·frac`.
- **Reversed axes:**
  - The extents are swapped before this computation (`ML/lineaxis.jl:204`), and the camera projection swaps left/right or bottom/top (`ML/blocks/axis.jl:64-88`).
  - Tick values and labels are unchanged.
  - `xlims!(ax, hi, lo)` with `hi > lo` sets `xreversed = true` (`ML/blocks/axis.jl:1228-1252`).
- Tick marks (`ML/lineaxis.jl:170-189`), with `sign = flipped ? -1 : 1`:
  - start: `tick_pos + sign·(tickalign·ticksize − 0.5·spinewidth)` along the normal;
  - end: `start − sign·ticksize`;
  - `tickalign = 0` means fully outside.
  - Defaults: ticksize 5, tickwidth 1, spinewidth 1 (`ML/types.jl:437-473`).
- Grid lines run from each tick position across the viewport height or width. They are drawn at z = −10, behind the plots (`ML/blocks/axis.jl:206-240, 465-487`). Default color `(0,0,0,0.12)`, width 1.

---

## 2. Tick label formatting (`MK/tick_format.jl`, all functions)

The default is `format_ticks_auto(xs)` (`:100-118`).

```
MINUS = "\u{2212}"                                        // :14 ; replaces a LEADING '-' only (:47)
style = if xs.nonempty && max!=min && |log10(max(xs)-min(xs))| > 4 { Scientific } else { Plain }   // :92-96
   // based on the tick RANGE (max−min), not on magnitude: range > 1e4 or < 1e-4

Plain (:55-58):
  p = plain_precision(xs)
  label = fixed(x, p) with leading '-'→U+2212           // Ryu.writefixed ≡ format!("{:.p$}", x)
plain_precision(xs) (:19-32):
  e10min=+INF, e10max=-INF
  for y in xs if finite:
     e10 = if |y| <= 1e-16 {min(e10min,0)} else { shortest_decimal_exponent(y as f32) }  // y = m·10^e10, m integer, shortest (Ryu reduce_shortest)
     e10min=min(e10min,e10); e10max=max(e10max,e10)
  return max(min(-e10min, -e10max+16), 0)
  // e.g. [0,2.5,5,7.5,10]: 2.5=25e-1 → p=1 → "0.0","2.5","5.0","7.5","10.0"; [0,5,10] → p=0; [0.00005,…] → p=5
  // Going through f32 hides f64 noise (0.30000000000000004 → 3e-1).

Scientific (:39-45, :67-88, :100-118):
  mantissas = [x==0 ? 0 : round(10^(frac(log10|x|)), sigdigits=15) for finite x]
  p = plain_precision(mantissas)
  parts = [x==0 ? None : split(format!("{:.p$e}", x)) → (base_str with '-'→U+2212, exponent i32)]
  can_strip = every base has an empty or all-zero fraction
  label = x==0 ? "0" : rich(base', "×10", superscript(exp_str, offset=(0.1,0)))
          base' = can_strip ? strip_trailing_zeros_and_dot(base) : base   // keep "1.50" alongside "2.00" for alignment
          exp_str = exp<0 ? "\u{2212}"+|exp| : exp     // no zero padding
  // e.g. [1.5e-5,2e-5,2.5e-5] → "1.5×10⁻⁵","2.0×10⁻⁵","2.5×10⁻⁵"; [0,5e5,1e6] → "0","5×10⁵","1×10⁶"
  // No common offset or multiplier: each label carries its own exponent.
```

- Nothing explicitly checks that labels are distinct. Distinct labels follow from nice tick values plus the shortest-representation precision.
  - The precision is computed from f32, but the digits are printed from the f64 value, so `1e8+1` and `1e8+2` still print distinctly.
  - **ezviz:** compute the shortest representation in f64. f32 is only needed to match Makie's rounding of noise.
- **Rich-text superscript** (`MK/basic_recipes/text.jl:1132-1143`):
  - size = 0.66 × parent size;
  - baseline raised by 0.4 × parent size;
  - x offset = `offset.x` × sup size, with 0.1 for tick labels.
- **Log labels** are the base string plus a superscript of the plain-formatted exponent: `"10"` + sup `"−2"`. U+2212 comes from the plain formatter.
- `format_ticks_plain` on integer arrays just stringifies them (`:61`).

---

## 3. Autolimits

### 3.1 Pipeline
Relevant code: `ML/blocks/axis.jl:632-717, 805-946, 964-1011`.

**State**
- `ax.limits`: user limits, `(x, y)` where each is `nothing` or `(lo|nothing, hi|nothing)`, or a 4-tuple.
- `targetlimits`: what interactions write.
- `finallimits = adjustlimits(targetlimits)`: after `autolimitaspect`. This drives the camera and ticks.

**`reset_limits!(ax; xauto=true, yauto=true)`**
- Per dimension: if the user limit is fully specified, use it.
- Otherwise `l = auto ? autolimits(ax, dim) : current targetlimits[dim]`. A partial user limit overrides one end.
- Errors if `lo > hi`.
- Sets `targetlimits`.

**`autolimits(ax, dim)`**
```
lims = getlimits(ax, dim)                      // union of plot bboxes (see below) or None
for link in linked_axes(dim): lims = union(lims, getlimits(link, dim))   // union BEFORE margins
if lims: validate against scale domain (log: (0,inf) open → error if <=0)
         lims = expandlimits(lims, margin.lo, margin.hi, scale)
else:    lims = current targetlimits[dim]
```

**`getlimits`** (`:805-846`)
- Skips plots with `xautolimits`/`yautolimits = false`, plots not in data space, and invisible plots.
- The bbox is computed after `transform_func` (e.g. log), then inverse-transformed.
- Non-finite values are ignored (`finite_min`/`finite_max`, `MK/layouting/data_limits.jl:65-75`).
- No finite value gives `None`.

**`expandlimits(lims, m_lo, m_hi, scale)`** (`:775-803`): margins are applied in scaled (transformed) space.
```
(a,b) = sorted; (sa,sb) = scale(a),scale(b); w = sb-sa
lims = inv(sa - w*m_lo), inv(sb + w*m_hi)
if lims.1 - lims.0 == 0 (isapprox atol=0 ⇒ exactly 0):      // single point / hline / vline
   zd = |scale(lims.0)|
   if zd == 0 && scale==identity { (-1, 1) }                  // x=0 → (-1,1)
   else { inv(scale(v)-zd), inv(scale(v)+zd) }                // x=5 → (0,10); x=-3 → (-6,0); log10 v=100 → (1,1e4)
// Edge case bug: log10 with v=1 gives zd=0 and stays singular. ezviz: fall back to (v/10, v*10).
```

**Defaults and domains**
- `xautolimitmargin = yautolimitmargin = (0.05, 0.05)` (`ML/types.jl:554-556`).
- Default limits with no plots: identity (0, 10); log (1, 1000); sqrt (0, 100) (`ML/blocks/axis.jl:1430-1437`).
- Domains: identity (−inf, inf); log (0, inf) open (`:1441-1444`).
- Interactions reject non-finite limits, zero-width limits, and limits outside the domain (`ML/types.jl:784-789`).

**When limits update**
- Adding a plot calls `reset_limits!` only if the scene is already open. Otherwise it runs once before display via `update_state_before_display!` (`MK/figureplotting.jl:567-588`).
- Changing plot data does **not** re-autoscale. Makie users call `autolimits!(ax)` (which sets `limits = (nothing, nothing)`) or `reset_limits!`.
- **ezviz:** add an opt-in `ax.follow(true)` mode for live simulations.

**`autolimitaspect`** (`:964-1011`)
- `correction = asp / ((dx/dy)/(w/h))`.
- If > 1, widen x by `(corr−1)` split in the ratio of `xautolimitmargin`, or 50/50 if the margins are 0. Otherwise widen y by `(1/corr − 1)` the same way.
- Linear space only.

### 3.2 Plot-type specific rules
- **Tight limits** for heatmap/image (plus contourf with integer levels, spy, some triplot/voronoi) (`ML/blocks/axis.jl:751-764`).
  - On `plot!(ax, p)`, `needs_tight_limits(p) && tightlimits!(ax)` (`MK/figureplotting.jl:583`).
  - `tightlimits!` sets both margins to (0, 0) for the whole axis and permanently (`ML/helpers.jl:101-105`).
- **Heatmap data limits** are the cell edges (`MK/compute-plots.jl:983-990`).
  - Centers become edges via `edges(v)` (`MK/conversions.jl:321-342`): midpoints, with the ends mirrored; a single value v gives `[v−0.5, v+0.5]`.
  - Conversion happens when `length(x) == size(z,1)`. `heatmap(z)` uses x = 1:nx and y = 1:ny as centers, so limits are `(0.5, n+0.5)`.
  - `z[i,j]` has i along x.
- **Barplot and hist include 0** because the rectangles span from `fillto` to `y`. `fillto` defaults to `offset = 0`, or `min(y>0)/2` on a log axis (`MK/basic_recipes/barplot.jl:21-36, 134-141`).
  - They are not tight, so the 5% margin shows below 0.
- **Band** includes both curves: its data limits are the mesh vertices.
- There are no other "include zero" rules.

### 3.3 Linked axes
- `linkxaxes!` / `linkyaxes!` / `linkaxes!` build the full transitive set of links, then call `reset_limits!(first)` (`ML/blocks/axis.jl:1015-1035`).
- On any `targetlimits` change (`:563-565` → `update_linked_limits!` `:851-895`), unless `block_limit_linking` is set:
  - links on both axes: copy the whole rect;
  - x-only links: copy x and keep their y;
  - y-only links: copy y and keep their x.
  - The guard flag is set on the target during the copy so the update does not bounce back.
- `xlims!` also writes to linked axes' `limits` (`:1242-1246`).
- Autolimits union the linked axes' data before margins (§3.1).

---

## 4. Layout (GridLayoutBase plus Makie blocks)

### 4.1 Types and defaults
- **Sizes** (`GLB/types.jl:151-171`):
  - `Auto(trydetermine=true, ratio=1)`: shrinks to the largest determinable single-span content; if nothing is determinable, the column shares the leftover space by `ratio`.
  - `Fixed(px)`.
  - `Relative(frac)`: a fraction of the space for columns, after gaps and protrusions.
  - `Aspect(index, ratio)`: column width = ratio × height of row `index`, or the reverse for rows.
  - Gaps are `Fixed | Relative`.
- **Align modes** (`GLB/types.jl:68-135`):
  - `Inside`: protrusions stick out of the bbox.
  - `Outside(pad)`: protrusions plus padding fit inside the bbox.
  - `Mixed(per side: nothing = Inside | f32 = Outside padding | Protrusion(p) = override)`.
- **Defaults**:
  - Figure root layout: `Outside(figure_padding = 16)`, bbox = scene viewport (`MK/figures.jl:118-145`).
  - rowgap = colgap = 18 from the theme (`MK/theming.jl:43-45`, wired in `MK/Makie.jl:392-397`).
  - Nested `GridLayout()`: `Inside`, width/height `Auto`, tellwidth/tellheight true, halign/valign center (`GLB/gridlayout.jl:85-162`).
- **Sides for content placement**: `Inner` (normal), `Left/Right/Top/Bottom/TopLeft...` (placed inside the protrusion gap, e.g. `f[1,1,Top()]`), `Outer`.

### 4.2 How a block reports its size (`GLB/layoutobservables.jl`)
Each block has `width`, `height` (`nothing | Real | Fixed | Relative | Auto`), `tellwidth`, `tellheight`, `halign`, `valign`, `alignmode`, `protrusions` (RectSides) and `autosize` (w or None, h or None).

```
computed_size(attr, auto, tell) = !tell ? None : match attr { None→None, Real/Fixed→v, Relative→None, Auto→auto }   // :182-203
reported.inner = (computed_size(w), computed_size(h)) [+protrusions+pad if Outside]                                   // :119-180
reported.outer = protrusions (Inside) | 0 (Outside) | per-side (Mixed)                                                  // :61-80
computedbbox given suggested cell bbox (:233-359):
   w_target = reported.inner.w ?? (Relative→x*bw | None→bw | Auto→autosize.w ?? bw | Fixed→x)
   (same for h); inner = w_target (minus prot+pad if Outside)
   l = cell.l + halign*(bw - w_target) [+prot.l+pad.l if Outside]; b = cell.b + valign*(bh - h_target) [...]
   halign: left=0, center=.5, right=1; valign: bottom=0, center=.5, top=1  (:1-17)
```

**Block specifics**

- **Axis**
  - `width = height = nothing`, tellwidth/tellheight true (`ML/types.jl:540-552`). It never determines column or row size; it fills the cell.
  - Protrusions (`ML/blocks/axis.jl:113-152`): `bottom = xaxis.protrusion`, or `top` if `xaxisposition = :top`; `left = yaxis.protrusion`; `top += title_h + titlegap` if the title is non-blank; plus `subtitle_h + subtitlegap`.
  - `aspect = AxisAspect(r) | DataAspect()` shrinks the scene area, centered, inside the computed bbox. The layout does not know about it (`ML/helpers.jl:15-55`). For layout-aware aspect use `colsize!(..., Aspect(...))`.
  - Scene area is rounded to integer pixels (`round_to_IRect2D`, `ML/helpers.jl:8-13`).
- **LineAxis protrusion** (`ML/lineaxis.jl:23-46`):
```
tickspace      = ticksvisible && !ticks.empty ? max(0, ticksize*(1-tickalign)) : 0          // :339-341 ; 5
ticklabelspace = ticklabelspace attr == automatic ? ideal : fixed/max_auto                 // :324-336
   ideal = union-bbox height (horizontal axis) / width (vertical axis) of ALL tick label texts  // :306-319
ticklabelgap   = (ticklabelsvisible && ticklabelspace>0) ? ticklabelspace + ticklabelpad : 0
labelspace     = (labelvisible && !blank(label)) ? label_bbox_extent_normal_to_axis + labelpadding : 0
protrusion     = tickspace + ticklabelgap + labelspace          // NOTE: spinewidth NOT included (but label positions include it)
```
  - Positions:
    - tick labels at `spine − (spinewidth + tickspace + ticklabelpad)` (`:150`);
    - axis label at `spine − (spinewidth + tickspace + (ticklabelspace + ticklabelpad) + labelpadding)`, centered along the axis (`:343-367`).
  - Alignment:
    - x tick labels `(:center, :top)`; y tick labels `(:right, :center)` (`:93-132`);
    - x label `(:center, :top)`; y label rotated +π/2 with align `(:center, :bottom)` (`:370-408`).
  - Pads and sizes (`ML/types.jl:397-439`): `xticklabelpad = 2`, `yticklabelpad = 4`, `xlabelpadding = 3`, `ylabelpadding = 5`, `titlegap = 4`, `titlefont = :bold`; all sizes inherit fontsize 14.
  - Title position: `y = top + titlegap + (xaxis on top ? xprot : 0) + subtitlespace`, aligned `(titlealign, :bottom)` (`ML/blocks/axis.jl:90-111`).
- **Colorbar**: `width = Auto`, and `autosize = (size = 12, None)` when vertical (`ML/blocks/colorbar.jl:170-177`).
  - An Auto column containing only a colorbar is 12 px wide.
  - Protrusion on the tick side is `LineAxis.protrusion`: right if `flipaxis` (default), left otherwise (`:458-478`).
- **Legend**: `width = height = Auto`; `tellwidth = automatic`, which means orientation == `:vertical`; `tellheight = automatic`, which means orientation == `:horizontal` (`ML/blocks/legend.jl:46-51`, `ML/types.jl:1511-1517`).
  - `autosize = determinedirsize(inner grid) + margins` (`:194-205`).
  - A vertical legend beside an axis sets the column width but not the row height. It is valign-centered at its natural height.
- **Label**: `autosize = (text_w + pad_l + pad_r, text_h + pad_b + pad_t)`, padding 0, tell both (`ML/blocks/label.jl:19-33`, `ML/types.jl:938-975`).
  - A supertitle `Label(f[0, :])` spans several columns, so it only sets the row height.
- **Nested GridLayout** reports:
  - inner size = `determinedirsize(gl)`, i.e. the sum of column sizes plus gaps, or None if any column is undeterminable (`GLB/gridlayout.jl:1133-1188`);
  - protrusions = max protrusion of its edge-touching children (`:1837-1868`);
  - `Outside` → protrusion 0.
  - It re-solves itself when the parent sets its bbox (`:153-160`).

### 4.3 The solver: `compute_rowcols(gl, suggestedbbox)` (`GLB/gridlayout.jl:874-1048`)
```
content_bbox = suggested minus Outside padding (Inside: unchanged)                          // :777-797
// 1. protrusion grid: per column the max left/right protrusion of content STARTING/ENDING there
maxgrid.lefts[c]   = max over contents with span.cols.start==c of prot.left    (Inner side: reported.outer)
maxgrid.rights[c]  = ... span.cols.stop==c ... prot.right ; tops[r] (span.rows.start), bottoms[r] (span.rows.stop)
   // Side-placed content (f[1,1,Left()]) contributes its determined width as protrusion (:822-826)
leftprot=lefts[0]; rightprot=rights[last]; topprot=tops[0]; bottomprot=bottoms[last]
colgaps[i] = lefts[i+1] + rights[i]   ;  rowgaps[i] = tops[i+1] + bottoms[i]   // protrusion part of each gap
(equalprotrusiongaps → all = max)
remaining_w = W(content) - sum(colgaps) [- leftprot - rightprot if Outside; per side if Mixed]    // :831-866
added gaps: Fixed→x, Relative→x*remaining                                                       // :917-934
space_cols = remaining_w - sum(addedcolgaps); space_rows likewise
(colwidths,rowheights) = compute_col_row_sizes(space_cols, space_rows)                          // §4.4
colwidths = max(colwidths,1); rowheights = max(rowheights,1)                                    // :943-944
finalcolgaps = colgaps + addedcolgaps
gridwidth = sum(colwidths)+sum(finalcolgaps) [+leftprot+rightprot if Outside]
xadj = halign_shift*(W(content) - gridwidth);  yadj = (1 - valign_shift)*(H(content) - gridheight)
xleft[c] = xadj + content.l + cumsum0(colwidths)[c] + cumsum0(finalcolgaps)[c] [+ leftprot if Outside]
xright[c] = xleft[c] + colwidths[c]
ytop[r] = content.t - yadj - cumsum0(rowheights)[r] - cumsum0(finalrowgaps)[r] [- topprot if Outside]
ybottom[r] = ytop[r] - rowheights[r]
```

**`align_to_bbox!`** (`:1084-1106`): for each content item:
- `cell = (xleft[span.cols.start], xright[span.cols.stop], ybottom[span.rows.stop], ytop[span.rows.start])`;
- Inner content gets `cell` as its suggested bbox;
- side content gets the protrusion strip: e.g. Left → `(l - lefts[c], l, b, t)` (`:1905-1939`).

**Why spines line up:** every item in a column receives the same cell, and the cell is the inner (spine) rectangle. Tick labels go into the gaps, and the gaps are sized by the maximum protrusion in that column or row.

### 4.4 `compute_col_row_sizes` (`GLB/gridlayout.jl:1246-1448`)
```
1 Fixed → value;  2 Relative → x*space;  3 Auto(trydetermine) → determinedirsize(col) if Some
     determinedirsize(col) = max over contents that are single-span in that col AND side==Inner
                             of reported.inner.w (None if none report) (:1195-1233)
4 Aspect referring to an already-determined row/col → ratio * that size
5 if no undetermined Aspect cols remain: undetermined Auto cols share (space - sum(determined)) by ratio/sum(ratios)
  same for rows
6 resolve remaining Aspects (error if the referenced side is still undetermined: row AND col aspect deadlock)
7 remaining undetermined Autos share leftover by ratio; error if anything is still undetermined
```

### 4.5 Upward reporting and `resize_to_layout!`
- `update!(gl)` (`GLB/gridlayout.jl:164-208`): `autosize = (determinedirsize(Col), determinedirsize(Row))`, `protrusions = compute_effective_protrusion(each side)`, then the parent is updated, or the root re-solves via `computedbbox`.
- `tight_bbox(gl)` (`:1062-1082`) for Outside:
  - `W = (xright[last] − xleft[0]) + lefts[0] + rights[last] + pad.l + pad.r`, and H likewise.
  - `resize_to_layout!(fig)` runs `update_state_before_display!` (limits, hence ticks and protrusions), then resizes the figure to `tight_bbox` (`MK/figures.jl:195-205`).
  - This is how whitespace is removed when there are Fixed or Aspect columns.
- Grids grow automatically when you index outside them. Zero or negative indices prepend rows/columns (offsets); `f[end+1, 1]` works (`GLB/gridlayout.jl:1997-2019`).

### 4.6 Text metrics (needed to match Makie's spacing)
- Text bbox = union, over glyphs, of `[x, x + hadvance] × [descender, ascender]·fontsize`. This is line-box based, not ink based (`MK/layouting/text_boundingbox.jl:45-68`).
- ascender/descender are hhea values divided by UPM, as FreeType reports them.
- **TeX Gyre Heros Makie** (Regular, Bold and Italic are identical): UPM 1000, ascender 947, descender −218 → line box = 1.165·fontsize (16.31 px at 14).
- Advances: digits 556, `.` 278, `-` 333, `−` (U+2212) 584, `×` 584, space 278, `e` 556.

### 4.7 Worked example
Setup: default 600×450 figure, one Axis, xlabel, ylabel, title, y tick labels up to "−1.0", fontsize 14. Let L = 16.31.
- bottom = 5 + (L + 2) + (L + 3) = 42.62
- left = 5 + (27.64 ["−1.0" = (584+556+278+556)·14/1000] + 4) + (L + 5) = 57.95
- top = L + 4 = 20.31
- right = 0
- Viewport: x from 16 + 57.95 = 73.95 to 584; y from 58.62 to 413.69. After rounding: origin (74, 59), size 510×355.
- Axis + Colorbar in `f[1,2]`: gap = 0 + 0 + 18 = 18 px; colorbar column = 12 px; the colorbar's right protrusion is taken from the right padding budget.

### 4.8 Update order for the port
limits → ticks and labels → measure text → protrusions → solve the grid → viewports → projection.
- Since tick selection does not depend on the viewport size, one pass is enough. `autolimitaspect` and `DataAspect` are the exceptions: iterate at most 2–3 times.
- **Jitter guard during interaction:** while panning or zooming, Makie freezes `ticklabelspace` at its current value and restores it 0.2 s after the last event (`ML/blocks/axis.jl:1059-1081`). Copy this so the layout does not jump.

---

## 5. Interactions (GLMakie defaults) and DataInspector

### 5.1 Axis interactions
Registered by default, in this order (`ML/blocks/axis.jl:53-59`): `:rectanglezoom`, `:limitreset`, `:scrollzoom` = `ScrollZoom(speed 0.1, reset_delay 0.2)`, `:dragpan` = `DragPan(0.2)`. The first interaction that consumes an event wins. Each can be switched off with `deactivate_interaction!`.

**Mouse state machine** (`ML/mousestatemachine.jl:190-203`): a drag starts after 2 px of movement; a double click is two clicks within 0.2 s.

**Scroll zoom** (`ML/interactions.jl:243-310`, only when the mouse is inside the axis scene, `ML/blocks/axis.jl:32-38`):
```
z = (1 - 0.1)^scroll_y            // per event; trackpads give fractional y
f = cursor fraction in [0,1]² of viewport, flipped (1-f) on reversed axes
work in transformed (log) space: T = scale(targetlimits)
new_w = w*z  (unless xzoomlock/yzoomlock)
new_origin = origin + f*(w - new_w)        // zoom about cursor
x key held → zoom x only; y key held → zoom y only   (xzoomkey=Keyboard.x, yzoomkey=Keyboard.y, zoombutton=true ⇒ always)
reject if limits invalid (non-finite, zero width, outside domain)
```

**Drag pan** (`:312-374`):
- Button: **right mouse** (`panbutton = Mouse.right`, `ML/types.jl:606`).
- `Δfrac = (px − prev_px) / viewport` (sign flipped on reversed axes); `origin −= Δfrac · widths`, computed in transformed space.
- Holding `y` (ypankey) keeps x fixed; holding `x` keeps y fixed. `xpanlock`/`ypanlock` do the same permanently.

**Rectangle zoom** (`:159-217`; visual in `ML/types.jl:761-782`):
- **Left drag**, no modifier (`modifier = true`).
- Selection points are clamped to the visible limits.
- Holding `x` restricts the zoom to x; holding `y` restricts it to y (`restrict_y = x pressed`, `restrict_x = y pressed`). `xrectzoom`/`yrectzoom = false` disable a direction.
- Visual: a translucent mesh, color `(:black, 0.2)`, drawn over the region **outside** the selection (outer axis rect minus inner rect, 8 triangles). It is shown only while active.
- On release it sets `targetlimits` if they are valid.

**Limit reset** (`:226-240`):
- **Ctrl + left click** → `reset_limits!`: back to the user `limits`, auto for anything not set.
- **Ctrl + Shift + left click** → `autolimits!`: clears the user limits.
- On macOS this is Control, not Cmd.

**ezviz notes for macOS:**
- Trackpad right-drag is awkward. Consider mapping winit `PinchGesture` to zoom and two-finger scroll to pan as an alternative scheme. That is a deviation from Makie, so make it configurable.

### 5.2 DataInspector (`MK/interaction/inspector.jl`)

**Activation:** off by default. You must call `DataInspector(fig)` explicitly (it is not referenced anywhere in GLMakie `src`).
- ezviz: the user asked for hover readout in `show()`, so turn it on by default there.

**Defaults** (`:240-260`):
- `range = 10` px, `offset = 10`, `apply_tooltip_offset = true`;
- indicator color red, linewidth 2;
- tooltip drawn at depth 9e3.

**Hover:** triggered on mouse move or scroll; stale requests are dropped via a channel (`:283-293`).
- `pick_sorted(root, mouse_px, range)` (`MK/interaction/interactive_api.jl:158-186`) reads the GPU pick buffer in a **square** of ±range px, collects unique `(plot, element_index)` pairs, and sorts them by the smallest pixel distance to the cursor.
- The first inspectable plot whose `show_data` returns true is shown; otherwise the tooltip is hidden (`:306-326`).
- ezviz on CPU: project the visible points into pixel space and search within 10 px (a uniform grid keeps this fast at 1M points), or use a GPU id buffer.

**Content by plot type**

| plot | text (`@sprintf`) | tooltip anchor / offset | indicator |
|---|---|---|---|
| scatter (`:479-501`) | `"x: %0.6f\ny: %0.6f"` (`:7`) of the data point | projected point; offset 0.5·markersize + 2 | none |
| lines (`:548-570`) | same format, of the **closest point on segment (idx−1, idx)** to the cursor ray, i.e. interpolated (`MK/interaction/ray_casting.jl:297-314`) | that point; offset linewidth + 2 | none |
| heatmap (`:625-627`, `show_imagelike` `:637-720`) | `"H[i, j] = %0.3f"` with integer cell indices when `interpolate = false` (default); fractional indices when true (`:39-43`) | mouse position | red outline around the cell (`:765+`) |
| barplot (`:801-835`) | `"x: …\ny: …"` of the bar's input point (swapped for direction :x) | mouse | red rectangle outline |
| band (`:981-1035`) | `"(%0.3f, %0.3f) .. (%0.3f, %0.3f)"` for the vertical segment through the cursor | mouse | red line segment |

- NaN cells whose `nan_color` has alpha 0 hide the tooltip.
- ezviz suggestion for simulation fields: show `x, y` coordinates as well as indices and value, and use the §2 formatter for the value.

**Tooltip placement** (`:461-470`): default placement is `:above`; if `py > 0.75·H` it goes `:below`; if `px < 0.25·W` it goes `:right`; if `px > 0.75·W` it goes `:left`.

**Tooltip style** (`MK/basic_recipes/tooltip.jl:7-52`):
- background white, outline black 1 px;
- `textpadding = (5, 5, 3, 3)` in LRBT order;
- `triangle_size = 7`, `offset = 10`;
- fontsize 14, text justified left;
- excluded from autolimits.

---

## 6. Legend

**Collecting entries**: `get_labeled_plots(ax; merge, unique)` (`ML/blocks/legend.jl:1025-1083`).
- Takes the axis' top-level plots in insertion order that have a `label` attribute.
- A vector `label` expands into one entry per element.
- `unique = true` keeps the first occurrence of each `(plot type, label)`.
- `merge = true` groups all plots with the same label into one entry, in order of first occurrence, and draws their elements on top of each other.
- A `label = "txt" => (; color = …, markersize = …)` pair overrides that entry's attributes.
- `Legend(f[1,2], ax)` errors if there are no labelled plots.
- Multiple axes can be passed as a vector.

**Elements per plot type** (`:723-803`, generic recursion `:887-895`):

| element | used for | attributes taken from the plot |
|---|---|---|
| `LineElement` | lines, linesegments | color (if scalar, else Legend `linecolor`), linestyle, linewidth, linecap, joinstyle, colormap/colorrange, alpha |
| `MarkerElement` | scatter | color, marker, markersize, strokewidth, strokecolor (scalars) |
| `PolyElement` | poly, density, violin/boxplot, and by recursion barplot/hist (their child `poly`) | color, strokecolor, strokewidth, linestyle |
| `PolyElement` for band | band | `polycolor = band.color`, stroke transparent, width 0 |

- Recipes such as scatterlines recurse into their children, so scatterlines gives Line and Marker elements drawn together.
- Text gives no element.
- A color that goes through a colormap falls back to the Legend default color (`extract_color` `:717-721`).

**Patch geometry**
- Each entry has a `Box` of `patchsize = (20, 20)`, with transparent fill and stroke.
- Elements are placed at fraction points of that box: `origin + p · size` (`:416-463`).
  - `linepoints = [(0, 0.5), (1, 0.5)]` → a horizontal line across the full width at mid height.
  - `markerpoints = [(0.5, 0.5)]` → one marker at the center.
  - `polypoints` = the unit square → a filled 20×20 rectangle.
- Defaults are in `ML/types.jl:1545-1622`.

**Layout** (`:78-170`)
- Inner `GridLayout(alignmode = Outside(padding = 6))`.
- Vertical orientation with the title on top: title in row 2g−1, entry subgrid in row 2g.
- Entry n goes to `(row, col) = ((n−1) ÷ nbanks + 1, (n−1) % nbanks + 1)`; columns are `[patch | label]` pairs.
- Gaps: `rowgap = 3`; between patch and label `patchlabelgap = 5`; between banks `colgap = 16`; `titlegap = 8`; `groupgap = 16`.
- Labels: `Label`, halign `:left`, valign `:center`, size 14. Title: bold, size 14.
- Frame: poly with 1 px black stroke, white fill (`framewidth 1`, `framecolor :black`, `backgroundcolor :white`), drawn inside `computedbbox` minus `margin`.
- Legend autosize = inner grid determined size + margin (`:194-205`).
- Size for n entries in one bank: width = 12 + 20 + 5 + max_label_w; height = 12 + n·20 + (n−1)·3, plus title_h + 8 if there is a title. Row height = max(20, L).

**axislegend** (`:1127-1164`)
- `Legend(ax.parent, …; bbox = ax.scene.viewport, margin = (6, 6, 6, 6), halign, valign)`. `bbox` is the plot area, not the grid cell, so the legend is not part of the grid.
- `position = :rt` by default. The first letter is `l/r/c` → halign `left/right/center`; the second is `t/b/c` → valign. A `(h, v)` tuple of numbers in 0..1 also works.
- Placement: `l = vp.l + h·(vp.w − W)` and `b = vp.b + v·(vp.h − H)`, where W and H include the margin. The frame therefore sits 6 px inside the spines.

**Click interaction** (`:365-390`): left click toggles the visibility of that entry's plots; right click toggles all; middle click syncs all. Hidden entries get a shade of `(0.9, 0.9, 0.9, 0.65)`.

---

## 7. Colorbar (`ML/blocks/colorbar.jl`, attributes `ML/types.jl:818-936`)

**`Colorbar(f[1,2], plot)`**
- Takes the plot's `ColorMapping` (colormap, colorrange, scale, lowclip, highclip, nan_color) by recursing into child plots (`:23-139`, `:141-166`).
- Errors if the plot's colors are literal colors rather than values mapped through a colormap.
- Passing `colormap`, `limits`, `lowclip` or `highclip` together with a plot is an error (`colorbar_check`).
- The colorbar stays linked to the plot: changing the plot's `colorrange` updates the colorbar.

**Automatic colorrange** (`MK/compute-plots.jl:208-244`, `MK/layouting/data_limits.jl:86-114`)
- `distinct_extrema_nan` of the finite scaled values; if lo == hi it uses `(lo − 0.5, hi + 0.5)`.
- A user colorrange with lo == hi is widened by `max(0.5, |lo|)`.

**Clipping**
- `lowclip`/`highclip` default to `automatic`: out-of-range values get the first or last colormap color (`MK/compute-plots.jl:190-192`), and no triangle is drawn.
- If set, the plot uses that color and the bar draws an equilateral triangle at that end:
  - base = bar thickness, height = thickness · sin(60°) ≈ 10.4 px for 12 px;
  - the bar shortens by that amount so the frame keeps its total length;
  - the frame border goes around the triangles as well (`:241-420`).

**Bar**
- `size = 12` px thick; `vertical = true`; `flipaxis = true`, so ticks and label are on the right (bottom/top for horizontal) (`ML/types.jl:888-934`).
- Continuous mode samples `nsteps = 100` values evenly over the colorrange and draws them as an interpolated 1×99 image of midpoints (`:220-320`).
  - ezviz: sample the colormap LUT texture directly with linear filtering.
- Border: a 1 px line (`spinewidth`) around the bar and any triangles.

**Ticks**
- An internal `LineAxis` with limits = colorrange exactly (no margins), `ticks = automatic` (`WilkinsonTicks(5, k_min = 3)`), and `scale = the plot's colorscale` (so log10 gives `LogTicks`) (`:423-456`).
- ticksize 5, tickalign 0 (outside), ticklabelpad 3, labelpadding 5.
- No spine line on the tick side; the border is drawn separately.
- Minor ticks `IntervalsBetween(5)`, hidden by default.
- Categorical colormaps: ticks at 1..n labelled with `string(value)`, limits (0.5, n + 0.5).

**Layout** (§4.2): `autosize = (12, None)`; protrusion = the axis protrusion on the tick side (`:458-478`). Height follows the row, i.e. it matches the neighbouring axis spines.

---

## 8. Histogram, barplot, band

### 8.1 hist (`MK/stats/hist.jl`)

**Defaults** (`:111-159`): `bins = 15`, `normalization = :none`, `weights = automatic`, `scale_to = nothing`, `gap = 0` (the barplot default is 0.2), `color = patchcolor` (cycled).

**Edges** `pick_hist_edges` (`:161-177`):
```
if bins is Int: (mi,ma)=extrema(values)   // empty → no bins
   if mi==ma → [mi-0.5, mi+0.5] (1 bin)
   else ma=nextfloat(ma); edges = linspace(mi, ma, bins+1)   // equal width; no Sturges/FD
else edges = bins (must be sorted, error otherwise)
```

**Binning:** StatsBase `fit(Histogram, v, edges)` with `closed = :left`. Bins are [e_i, e_{i+1}); the bin index is `searchsortedlast(edges, x)`; values outside the edges are dropped (`SB/hist.jl:243-258`).
- ezviz: skip NaN values explicitly.

**Normalization** (`SB/hist.jl:462-510`, applied at `MK/stats/hist.jl:35-49`), with `w_i` = count or weight sum and `Δ_i` = bin width:
- `:none` → `w_i`
- `:density` → `w_i / Δ_i`
- `:probability` → `w_i / Σw`
- `:pdf` → `w_i / (Σw · Δ_i)`, so it integrates to 1
- Then `scale_to = s` gives `w · s / max(w)`, and `:flip` negates.

**Output:** `barplot(centers, weights; width = diff(edges), gap = 0, stack/dodge groups…)` (`:179-300`).
- The baseline is 0 (so 0 is in the limits); on a log y axis it is `min(positive)/2`.
- `color = :values` colors by height.
- `stephist` builds a stairs line: `[edge0, …, edgeN, edgeN]` against `[0, w…, 0]` (`:84-104`).

### 8.2 barplot (`MK/basic_recipes/barplot.jl`)
**Defaults** (`:43-130`): `gap = 0.2`, `dodge_gap = 0.03`, `width = automatic`, `fillto = automatic`, `offset = 0`, `direction = :y`, `strokewidth = patchstrokewidth (0)`, `color = patchcolor` cycled via `cycle = [:color => :patchcolor]`.

```
w = width==auto ? (unique finite x sorted; diffs.empty ? 1 : min(diffs)) : width      // :334-339
w_eff = w*(1-gap)                                                                     // :145-159
dodge (i in 1..n_dodge, n_dodge=max(dodge)):
   dw = (1 - (n_dodge-1)*dodge_gap)/n_dodge                                          // :161
   x̂ = x + w_eff*((dw-1)/2 + (i-1)*(dw+dodge_gap));  bar width = w_eff*dw            // :163-165
fillto: auto → offset (0) | log axis → clamp y to min(y>0)/2, fillto = that             // :21-36
stack (ints): per x̂, cumulative sums done separately for positive and negative groups; from/to used as (fillto, y)  // :167-209
rect = (x̂ - |bw|/2, min(fillto, y+offset), |bw|, |y+offset - fillto|); direction :x swaps axes   // :134-141, :381
```
- **Categorical x:** barplot only accepts numbers. Makie maps `Categorical` or Enum values to integers 1..n, **sorted with `sortby = identity`**, and labels the ticks with `string(category)`, one tick per category (`MK/dim-converts/categorical-integration.jl:50-68, 148-160`).
  - Plain strings are not converted automatically in 0.24. The usual idiom is `barplot(1:n, v; axis = (xticks = (1:n, names),))`.
  - ezviz: accept `&[&str]`, map to 1..n **in order of appearance**, and set the tick labels. This deviates from Makie's sorting; choose deliberately.
- Bar labels (`bar_labels`, `label_offset = 5`, `flip_labels_at`) are optional and can be deferred.

### 8.3 band (`MK/basic_recipes/band.jl`)
- `band(x, ylow, yhigh)` gives `lower = (x_i, ylow_i)` and `upper = (x_i, yhigh_i)` (`:23-25`).
- Mesh vertices are `[lower; upper]` (n + n). Faces for i in 1..n−1, 1-based: `(i, i+1, n+i)` and `(i+1, n+i+1, n+i)` (`:30-34`).
- If `lower[i]` or `upper[i]` is NaN, both are set to NaN, which cuts out whole quads (`:47-53`).
- `direction = :y` swaps x and y.
- Color: `color = patchcolor`, cycled. `patchcolor` is the Wong palette lerped 80% toward the color from the background, i.e. `0.2·bg + 0.8·c` (`MK/theming.jl:18-27`).
- A per-point color vector of length n is mirrored to both sides; length 2n is used directly.
- Stroke: the outline is `lines(lower ++ NaN ++ upper)` with `strokewidth = patchstrokewidth = 0`, so it is invisible by default (`:81-103`).
- Legend: `PolyElement` with no stroke.

---

## Deviations from Makie I recommend flagging in the ezviz design
1. **Log major ticks:** use integer decades whenever the range spans at least 1 decade; Makie can produce `10^0.5`.
2. **Log minor ticks:** default to 2..9 × 10^k; Makie's default `IntervalsBetween(2)` gives 5.5.
3. **Log singular limits:** a single point at 1 on a log axis stays singular in Makie; expand to `(v/10, v·10)`.
4. **Live data:** add an opt-in autoscale-follow mode, since Makie never re-autoscales on data change.
5. **DataInspector:** on by default in `show()`; for heatmaps show coordinates as well as indices, formatted with the §2 formatter.
6. **Categorical bars:** keep order of appearance rather than sorting.
7. **Tick label precision:** use f64 shortest representation, or keep f32 only to match Makie exactly.
8. **Golden tests:** generate tick and label fixtures by running PlotUtils and Makie's `format_ticks_auto` in the user's Julia 1.12, because tie-breaking depends on exact float behaviour.