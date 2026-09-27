# Layout reference fixtures for sciplot's GridLayoutBase port (tests/layout.rs).
#
#     julia --project=tools tools/gen_layout_fixtures.jl
#
# Builds CairoMakie figures, runs `update_state_before_display!` (final limits, ticks and hence
# protrusions), and dumps the whole layout tree: for every grid its sizes, gaps, align mode and
# bboxes; for every block its span, side, protrusions, size attributes, autosize, tell flags,
# alignment, reported dimensions and the computed bbox (plus the scene viewport for an Axis).
# All bboxes are Makie's y-up figure units as `[x, y, w, h]`; spans are 1-based with the grid's
# offsets removed. Writes tests/fixtures/layout.json.

using CairoMakie
using JSON
const GLB = Makie.GridLayoutBase

rect(r) = [Float64.(minimum(r))..., Float64.(widths(r))...]
opt(x) = x === nothing ? nothing : Float64(x)
sides4(s) = Float64[s.left, s.right, s.bottom, s.top]

function contentsize(s, gl, dir)
    s isa GLB.Auto && return Dict("type" => "Auto", "trydetermine" => s.trydetermine, "ratio" => s.ratio)
    s isa GLB.Fixed && return Dict("type" => "Fixed", "x" => s.x)
    s isa GLB.Relative && return Dict("type" => "Relative", "x" => s.x)
    # Aspect(i, r) refers to row i (for a column) or column i (for a row), with offsets.
    s isa GLB.Aspect && return Dict("type" => "Aspect", "index" => s.index - GLB.offset(gl, dir), "ratio" => s.ratio)
    error("unknown size $s")
end

gapsize(g) = g isa GLB.Fixed ? Dict("type" => "Fixed", "x" => g.x) : Dict("type" => "Relative", "x" => g.x)

function sizeattr(s)
    s === nothing && return Dict("type" => "Nothing")
    s isa Real && return Dict("type" => "Fixed", "x" => Float64(s))
    s isa GLB.Fixed && return Dict("type" => "Fixed", "x" => s.x)
    s isa GLB.Relative && return Dict("type" => "Relative", "x" => s.x)
    s isa GLB.Auto && return Dict("type" => "Auto")
    error("unknown size attribute $s")
end

function alignmode(al)
    al isa GLB.Inside && return Dict("type" => "Inside")
    al isa GLB.Outside && return Dict("type" => "Outside", "padding" => sides4(al.padding))
    side(v) = v === nothing ? nothing : v isa GLB.Protrusion ? Dict("protrusion" => v.p) : Dict("pad" => v)
    s = al.sides
    Dict("type" => "Mixed", "sides" => [side(s.left), side(s.right), side(s.bottom), side(s.top)])
end

align(a) = Float64(GLB.halign2shift(a))
valign(a) = Float64(GLB.valign2shift(a))

function dims(lo)
    d = lo.reporteddimensions[]
    Dict("inner" => [opt(d.inner[1]), opt(d.inner[2])], "outer" => sides4(d.outer))
end

function dumpgrid(gl::GLB.GridLayout)
    lo = gl.layoutobservables
    content = map(gl.content) do c
        span = Dict(
            "rows" => [c.span.rows.start, c.span.rows.stop] .- GLB.offset(gl, GLB.Row()),
            "cols" => [c.span.cols.start, c.span.cols.stop] .- GLB.offset(gl, GLB.Col()),
        )
        d = Dict{String, Any}("span" => span, "side" => string(nameof(typeof(c.side))))
        x = c.content
        if x isa GLB.GridLayout
            d["kind"] = "GridLayout"
            d["grid"] = dumpgrid(x)
        else
            clo = x.layoutobservables
            d["kind"] = string(nameof(typeof(x)))
            d["protrusions"] = sides4(clo.protrusions[])
            d["autosize"] = [opt(clo.autosize[][1]), opt(clo.autosize[][2])]
            d["width"] = sizeattr(x.width[])
            d["height"] = sizeattr(x.height[])
            # Legend resolves `automatic` tell flags from its orientation into `_tellwidth` etc.
            d["tellwidth"] = hasfield(typeof(x), :_tellwidth) ? x._tellwidth[] : x.tellwidth[]
            d["tellheight"] = hasfield(typeof(x), :_tellheight) ? x._tellheight[] : x.tellheight[]
            d["halign"] = align(x.halign[])
            d["valign"] = valign(x.valign[])
            d["alignmode"] = alignmode(x.alignmode[])
            d["reported"] = dims(clo)
            d["suggestedbbox"] = rect(clo.suggestedbbox[])
            d["computedbbox"] = rect(clo.computedbbox[])
            if x isa Axis
                d["viewport"] = rect(x.scene.viewport[])
            end
        end
        d
    end
    Dict(
        "nrows" => GLB.nrows(gl),
        "ncols" => GLB.ncols(gl),
        "rowsizes" => [contentsize(s, gl, GLB.Col()) for s in gl.rowsizes],
        "colsizes" => [contentsize(s, gl, GLB.Row()) for s in gl.colsizes],
        "rowgaps" => gapsize.(gl.addedrowgaps),
        "colgaps" => gapsize.(gl.addedcolgaps),
        "alignmode" => alignmode(gl.alignmode[]),
        "equalprotrusiongaps" => collect(gl.equalprotrusiongaps),
        "width" => sizeattr(gl.width[]),
        "height" => sizeattr(gl.height[]),
        "tellwidth" => gl.tellwidth[],
        "tellheight" => gl.tellheight[],
        "halign" => align(gl.halign[]),
        "valign" => valign(gl.valign[]),
        "reported" => dims(lo),
        "suggestedbbox" => rect(lo.suggestedbbox[]),
        "computedbbox" => rect(lo.computedbbox[]),
        "content" => content,
    )
end

function dumpfig(name, f)
    Makie.update_state_before_display!(f)
    d = Dict(
        "name" => name,
        "size" => Float64.(collect(widths(f.scene.viewport[]))),
        "layout" => dumpgrid(f.layout),
        "tight_bbox" => rect(GLB.tight_bbox(f.layout)),
    )
    # The figure size `resize_to_layout!` picks (the scene viewport is integer).
    Makie.resize_to_layout!(f)
    d["resized_size"] = Float64.(collect(widths(f.scene.viewport[])))
    println(name, ": ", d["tight_bbox"], " -> ", d["resized_size"])
    d
end

xy = (1:10, (1:10) .^ 2)
figs = []

# 1. The worked example: one axis with labels and a title.
let f = Figure()
    Axis(f[1, 1]; title = "title", xlabel = "x label", ylabel = "y label")
    lines!(f[1, 1], -1 .. 1, sin)
    push!(figs, dumpfig("single_axis", f))
end

# 2. 2×2 axes with titles.
let f = Figure(size = (800, 600))
    for i in 1:2, j in 1:2
        ax = Axis(f[i, j]; title = "($i, $j)")
        scatter!(ax, xy...)
    end
    push!(figs, dumpfig("grid_2x2", f))
end

# 3. Spans: a wide top axis over two small ones.
let f = Figure()
    Axis(f[1, 1:2]; xlabel = "wide")
    Axis(f[2, 1]; ylabel = "left")
    Axis(f[2, 2])
    Axis(f[1:2, 3]; title = "tall")
    push!(figs, dumpfig("spans", f))
end

# 4. Relative column and row sizes.
let f = Figure()
    for i in 1:2, j in 1:3
        Axis(f[i, j])
    end
    colsize!(f.layout, 1, Relative(0.5))
    rowsize!(f.layout, 2, Relative(1 / 3))
    push!(figs, dumpfig("relative_sizes", f))
end

# 5. Fixed sizes everywhere: the grid is smaller than the figure and centered.
let f = Figure()
    Axis(f[1, 1]); Axis(f[1, 2]); Axis(f[2, 1:2])
    colsize!(f.layout, 1, Fixed(150)); colsize!(f.layout, 2, Fixed(200))
    rowsize!(f.layout, 1, Fixed(120)); rowsize!(f.layout, 2, Fixed(100))
    push!(figs, dumpfig("fixed_sizes", f))
end

# 6. Aspect column: square axes whose column width follows the row height.
let f = Figure(size = (700, 400))
    Axis(f[1, 1]; title = "square"); Axis(f[1, 2])
    colsize!(f.layout, 1, Aspect(1, 1.0))
    push!(figs, dumpfig("aspect_col", f))
end

# 7. Aspect row: row height follows a column width.
let f = Figure(size = (500, 600))
    Axis(f[1, 1]); Axis(f[2, 1])
    rowsize!(f.layout, 1, Aspect(1, 0.5))
    push!(figs, dumpfig("aspect_row", f))
end

# 8. Custom gaps: global and per-gap.
let f = Figure(size = (700, 500))
    for i in 1:3, j in 1:3
        Axis(f[i, j])
    end
    colgap!(f.layout, 40)
    colgap!(f.layout, 2, 5)
    rowgap!(f.layout, 1, 0)
    push!(figs, dumpfig("custom_gaps", f))
end

# 9. Different tick-label widths in one column: spines still line up.
let f = Figure()
    ax1 = Axis(f[1, 1]; ylabel = "small")
    ax2 = Axis(f[2, 1]; ylabel = "large")
    Axis(f[1:2, 2])
    lines!(ax1, 0:10, (0:10) ./ 10)
    lines!(ax2, 0:10, 100000 .* (0:10))
    push!(figs, dumpfig("ticklabel_widths", f))
end

# 10. Heatmap with a vertical Colorbar (fixed-width, tellwidth).
let f = Figure()
    ax, hm = heatmap(f[1, 1], reshape(1:100, 10, 10))
    Colorbar(f[1, 2], hm; label = "value")
    push!(figs, dumpfig("colorbar", f))
end

# 11. Vertical Legend beside an axis (tellheight = false).
let f = Figure()
    ax = Axis(f[1, 1])
    lines!(ax, 1:10; label = "line one")
    scatter!(ax, 1:10; label = "scatter")
    Legend(f[1, 2], ax)
    push!(figs, dumpfig("legend_vertical", f))
end

# 12. Horizontal Legend below two axes (tellwidth = false).
let f = Figure()
    ax = Axis(f[1, 1])
    Axis(f[1, 2])
    lines!(ax, 1:10; label = "first")
    lines!(ax, 2:11; label = "second")
    Legend(f[2, 1:2], ax; orientation = :horizontal)
    push!(figs, dumpfig("legend_horizontal", f))
end

# 13. Super title in a prepended row spanning all columns.
let f = Figure()
    Axis(f[1, 1]); Axis(f[1, 2]; title = "right")
    Label(f[0, :], "Super title"; fontsize = 24, font = :bold)
    push!(figs, dumpfig("supertitle", f))
end

# 14. Panel labels at TopLeft (the S3 idiom).
let f = Figure(size = (700, 500))
    for (i, (r, c)) in enumerate([(1, 1), (1, 2), (2, 1), (2, 2)])
        Axis(f[r, c]; title = r == 1 ? "top" : "")
        Label(f[r, c, TopLeft()], string("abcd"[i]); fontsize = 20, font = :bold,
            padding = (0, 5, 5, 0), halign = :right)
    end
    push!(figs, dumpfig("panel_labels", f))
end

# 15. Axes with a fixed width/height, aligned inside larger cells.
let f = Figure()
    Axis(f[1, 1]; width = 200, halign = :left)
    Axis(f[1, 2]; height = 150, valign = :top, tellheight = false)
    Axis(f[2, 1:2]; width = 300, height = 100, tellwidth = false)
    push!(figs, dumpfig("fixed_axis_align", f))
end

# 16. A nested GridLayout.
let f = Figure(size = (800, 500))
    Axis(f[1, 1]; ylabel = "outer")
    gl = GridLayout(f[1, 2])
    Axis(gl[1, 1]; title = "inner top")
    Axis(gl[2, 1]; xlabel = "inner bottom")
    Axis(gl[1:2, 2])
    rowgap!(gl, 5)
    push!(figs, dumpfig("nested", f))
end

# 17. Custom figure padding and Auto ratios.
let f = Figure(size = (600, 400), figure_padding = (5, 30, 10, 40))
    Axis(f[1, 1]); Axis(f[1, 2]); Axis(f[1, 3])
    colsize!(f.layout, 2, Auto(2))
    colsize!(f.layout, 3, Auto(0.5))
    push!(figs, dumpfig("padding_auto_ratio", f))
end

# 18. Relative gaps.
let f = Figure()
    Axis(f[1, 1]); Axis(f[1, 2]); Axis(f[2, 1]); Axis(f[2, 2])
    colgap!(f.layout, Relative(0.1))
    rowgap!(f.layout, Relative(0.05))
    push!(figs, dumpfig("relative_gaps", f))
end

# 19. A Label in its own Auto(false) column does not set the column width.
let f = Figure()
    Axis(f[1, 1]); Axis(f[2, 1])
    Label(f[1:2, 0], "shared y label"; rotation = pi / 2)
    Label(f[3, 1], "a long caption under the axes that should not shrink the column"; tellwidth = false)
    colsize!(f.layout, 1, Auto(false))
    push!(figs, dumpfig("label_autofalse", f))
end

# 20. Side labels: Left and Bottom placements, plus a Right colorbar-like label.
let f = Figure()
    Axis(f[1, 1]); Axis(f[1, 2])
    Label(f[1, 1, Left()], "left side"; rotation = pi / 2, padding = (0, 10, 0, 0))
    Label(f[1, 1:2, Bottom()], "bottom side"; valign = :bottom)
    Label(f[1, 2, Right()], "R")
    Label(f[1, 2, BottomRight()], "br")
    push!(figs, dumpfig("side_labels", f))
end

# 21. Nested grid with Mixed / Outside align modes and alignment.
let f = Figure(size = (700, 500))
    Axis(f[1, 1]; title = "main")
    gl = GridLayout(f[1, 2]; alignmode = Mixed(left = 0, top = GLB.Protrusion(0)))
    Axis(gl[1, 1]; ylabel = "mixed")
    Axis(gl[2, 1])
    gl2 = GridLayout(f[2, 1:2]; alignmode = Outside(10), height = 120, tellheight = true)
    Axis(gl2[1, 1]; xlabel = "outside")
    push!(figs, dumpfig("nested_alignmodes", f))
end

# 22. Tight figure for resize_to_layout: fixed axes plus a colorbar.
let f = Figure(size = (900, 700))
    ax, hm = heatmap(f[1, 1], reshape(1:25, 5, 5); axis = (width = 250, height = 200, title = "fixed"))
    Colorbar(f[1, 2], hm)
    Axis(f[2, 1]; width = 250, height = 100)
    push!(figs, dumpfig("tight", f))
end

# 23. Nested grid whose columns are all fixed: the grid reports a determined width.
let f = Figure()
    Axis(f[1, 1])
    gl = GridLayout(f[1, 2])
    Axis(gl[1, 1]); Axis(gl[2, 1])
    colsize!(gl, 1, Fixed(120))
    Label(f[0, 1:2], "determined"; tellwidth = false)
    push!(figs, dumpfig("nested_determined", f))
end

out = joinpath(@__DIR__, "..", "tests", "fixtures", "layout.json")
mkpath(dirname(out))
open(out, "w") do io
    JSON.print(io, Dict("makie_version" => string(pkgversion(Makie)), "figures" => figs), 1)
end
println("wrote ", out, " (", length(figs), " figures)")
