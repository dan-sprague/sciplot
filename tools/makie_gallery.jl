# CairoMakie reference renders for the side-by-side gallery (plan §7).
#
#   cargo run --release --example gallery          # ezviz PNG/SVG + target/gallery/data/*.json
#   julia --project=tools tools/makie_gallery.jl   # -> target/gallery/makie/*.png
#   cargo run --release --example compare          # -> target/gallery/index.html
#
# Each function below mirrors the page of the same name in examples/gallery.rs and draws the
# arrays dumped by it (never recomputed here), at px_per_unit = 2 like ezviz's default PNG.
# Optional arguments filter pages by substring, like the gallery example.

using CairoMakie
using JSON

const ROOT = normpath(joinpath(@__DIR__, "..", "target", "gallery"))
const DATA = joinpath(ROOT, "data")
const OUT = joinpath(ROOT, "makie")
const WONG = Makie.wong_colors()

# JSON arrays (null = NaN) -> Vector{Float64}
vec64(a) = Float64[x === nothing ? NaN : Float64(x) for x in a]
function load(name)
    d = JSON.parsefile(joinpath(DATA, name * ".json"))
    Dict{String,Any}(k => (v isa AbstractVector ? vec64(v) : v) for (k, v) in d)
end
function out(name, fig; px_per_unit = 2)
    path = joinpath(OUT, name * ".png")
    save(path, fig; px_per_unit = px_per_unit)
    println("wrote ", path)
end

function s1_scatter(d)
    fig, _, _ = scatter(d["x"], d["y"])
    fig
end

function s1_axis(d)
    fig = Figure()
    ax = Axis(fig[1, 1], title = "uniform samples", xlabel = "x", ylabel = "y")
    scatter!(ax, d["x"], d["y"], markersize = 6, color = (WONG[2], 0.6))
    scatter!(ax, [0.25, 0.5, 0.75], [0.5, 0.5, 0.5], marker = :star5, markersize = 30,
        strokewidth = 1.5, strokecolor = :black)
    fig
end

function s2_lines(d)
    fig = Figure()
    ax = Axis(fig[1, 1], title = "Harmonic oscillator", xlabel = "time t (s)", ylabel = "displacement x (mm)")
    lines!(ax, d["t"], d["s"], label = "sin")
    lines!(ax, d["t"], d["c"], label = "cos", linestyle = :dash)
    axislegend(ax, position = :rt)
    fig
end

function s3_panels(d)
    fig = Figure(size = (900, 650))
    a = Axis(fig[1, 1], title = "ω = 1", ylabel = "u (V)")
    b = Axis(fig[1, 2], title = "ω = 2")
    c = Axis(fig[2, 1:2], title = "ω = 0.5", xlabel = "t (s)", ylabel = "u (V)")
    for (ax, key) in ((a, "1"), (b, "2"), (c, "05"))
        lines!(ax, d["t"], d["model_$key"], label = "model")
        scatter!(ax, d["td"], d["data_$key"], label = "measurement", markersize = 7)
    end
    linkxaxes!(a, b)
    linkyaxes!(a, b)
    hideydecorations!(b, grid = false)
    Legend(fig[1:2, 3], [a, b, c], unique = true, framevisible = false)
    for (pos, s) in ((fig[1, 1, TopLeft()], "A"), (fig[1, 2, TopLeft()], "B"), (fig[2, 1:2, TopLeft()], "C"))
        Label(pos, s, fontsize = 20, font = :bold, padding = (0, 5, 5, 0), halign = :right)
    end
    fig
end

function s4_heatmap(d)
    nx, ny = Int(d["nx"]), Int(d["ny"])
    lx, ly = d["lx"], d["ly"]
    n = reshape(d["n"], nx, ny)
    fig = Figure(size = (800, 950))
    ax1 = Axis(fig[1, 1], title = "flat Vec<f64>", ylabel = "y (mm)")
    hm1 = heatmap!(ax1, 0 .. lx, 0 .. ly, n, colormap = :magma, colorrange = (0, 1.2))
    Colorbar(fig[1, 2], hm1, label = "n (a.u.)")
    ax2 = Axis(fig[2, 1], title = "cell centres", ylabel = "y (mm)")
    hm2 = heatmap!(ax2, d["xc"], d["yc"], n, colormap = :magma, colorrange = (0, 1.2))
    Colorbar(fig[2, 2], hm2, label = "n (a.u.)")
    ax3 = Axis(fig[3, 1], title = "probes", xlabel = "x (mm)", ylabel = "y (mm)")
    sc = scatter!(ax3, d["px"], d["py"], color = d["temp"], colormap = :viridis, markersize = 10)
    Colorbar(fig[3, 2], sc, label = "T (eV)")
    colsize!(fig.layout, 1, Aspect(1, lx / ly))
    linkaxes!(ax1, ax2, ax3)
    hidexdecorations!(ax1, grid = false)
    hidexdecorations!(ax2, grid = false)
    fig
end

function s6_log(d)
    fig = Figure(size = (900, 400))
    ax1 = Axis(fig[1, 1], title = "Kolmogorov spectrum", xlabel = "k (1/m)", ylabel = "E(k)",
        xscale = log10, yscale = log10, xminorticksvisible = true, yminorticksvisible = true,
        xminorgridvisible = true, yminorgridvisible = true)
    lines!(ax1, d["k"], d["e"])
    ax2 = Axis(fig[1, 2], title = "semilog-y", xlabel = "t (s)", ylabel = "signal", yscale = log10,
        yminorticksvisible = true)
    scatterlines!(ax2, d["t"], d["s"], markersize = 5)
    fig
end

function s7_stats(d)
    fig = Figure(size = (1200, 400))
    ax1 = Axis(fig[1, 1], title = "histogram", xlabel = "x", ylabel = "probability density")
    hist!(ax1, d["samples"], bins = 40, normalization = :pdf, label = "samples")
    lines!(ax1, d["xs"], d["pdf"], color = :black, linewidth = 2, label = "N(0, 1)")
    ax2 = Axis(fig[1, 2], title = "solver runtime", ylabel = "time (s)",
        xticks = (1:4, ["CG", "GMRES", "BiCGStab", "Jacobi"]))
    barplot!(ax2, 1:4, d["heights"])
    ax3 = Axis(fig[1, 3], title = "ensemble mean ± 95% CI", xlabel = "t (s)", ylabel = "u")
    band!(ax3, d["t"], d["lo"], d["hi"], color = (WONG[1], 0.3), label = "95% CI")
    lines!(ax3, d["t"], d["mean"], color = WONG[1], label = "mean")
    axislegend(ax1); axislegend(ax3, position = :rb)
    fig
end

function s8_paper(d)
    PT = 96 / 72
    INCH = 96
    set_theme!(theme_minimal())
    update_theme!(fontsize = 12PT, figure_padding = 4PT, linewidth = 1PT,
        Axis = (xticksvisible = true, yticksvisible = true, spinewidth = 0.75PT,
            xtickwidth = 0.75PT, ytickwidth = 0.75PT, xticksize = 3PT, yticksize = 3PT))
    fig = Figure(size = (4INCH, 3INCH))
    ax = Axis(fig[1, 1], xlabel = "time t (ms)", ylabel = rich("n (m", superscript("−3"), ")"))
    lines!(ax, d["t"], d["n"])
    fig  # the theme is reset after saving
end

function theme_minimal_page(d)
    with_theme(theme_minimal()) do
        fig = Figure()
        ax = Axis(fig[1, 1], title = "theme_minimal", xlabel = "x", ylabel = "y")
        lines!(ax, d["x"], d["y1"])
        lines!(ax, d["x"], d["y2"])
        scatter!(ax, d["sx"], d["sy"])
        fig
    end
end

function stress_markers(d)
    markers = [:circle, :rect, :diamond, :cross, :xcross, :utriangle, :dtriangle, :ltriangle,
        :rtriangle, :pentagon, :hexagon, :star5, Circle, Rect]
    fig = Figure(size = (900, 400))
    ax = Axis(fig[1, 1], title = "markers (sizes 8, 16, 28; stroked)", limits = (0, 15, 0, 4))
    for (i, m) in enumerate(markers)
        scatter!(ax, [i], [1.0], marker = m, markersize = 8, color = WONG[1])
        scatter!(ax, [i], [2.0], marker = m, markersize = 16, color = WONG[1])
        scatter!(ax, [i], [3.0], marker = m, markersize = 28, color = WONG[3], strokewidth = 2,
            strokecolor = :black)
    end
    fig
end

function stress_nan(d)
    fig = Figure()
    ax = Axis(fig[1, 1], title = "NaN gaps")
    lines!(ax, d["x"], d["y"], linewidth = 3)
    scatterlines!(ax, d["x"], d["y2"])
    scatter!(ax, d["sx"], d["sy"], markersize = 10)
    fig
end

function stress_offset(d)
    fig = Figure(size = (900, 400))
    a = Axis(fig[1, 1], title = "x = 1e9 + small")
    lines!(a, d["x"], d["y"])
    b = Axis(fig[1, 2], title = "y = 5 + 1e-6 · cos")
    scatter!(b, d["x2"], d["y2"], markersize = 5)
    fig
end

function stress_logheatmap(d)
    nx, ny = Int(d["nx"]), Int(d["ny"])
    fig = Figure()
    ax = Axis(fig[1, 1], title = "log-log heatmap", xscale = log10, yscale = log10)
    heatmap!(ax, d["xe"], d["ye"], reshape(d["z"], nx, ny))
    fig
end

function stress_lines(d)
    fig = Figure(size = (900, 700))
    a = Axis(fig[1, 1], title = "linewidths 0.5 1 2 4 8")
    for (i, w) in enumerate([0.5, 1, 2, 4, 8])
        lines!(a, [0.0, 1.0], [i - 1, i - 0.5], linewidth = w, color = :black)
    end
    b = Axis(fig[1, 2], title = "linestyles")
    xs = d["xs"]
    for (i, s) in enumerate([:solid, :dash, :dot, :dashdot, :dashdotdot])
        lines!(b, xs, (i - 1) .+ 0.3 .* sin.(6 .* xs), linestyle = s, linewidth = 2)
    end
    c = Axis(fig[2, 1], title = "joins: miter, bevel, round; caps: butt, square, round")
    zx = [0.0, 1, 2, 3, 4]
    zy = [0.0, 1, 0, 1, 0]
    for (i, (j, cap)) in enumerate([(:miter, :butt), (:bevel, :square), (:round, :round)])
        lines!(c, zx, zy .+ 1.6 * (i - 1), linewidth = 12, joinstyle = j, linecap = cap)
    end
    e = Axis(fig[2, 2], title = "chirp, 4000 points")
    lines!(e, d["dx"], d["dy"], linewidth = 1)
    fig
end

function stress_text(d)
    fig = Figure()
    ax = Axis(fig[1, 1], title = "text", limits = (0, 4, 0, 4))
    for (i, al) in enumerate([(:left, :bottom), (:center, :center), (:right, :top)])
        x = Float64(i)
        scatter!(ax, [x], [3.0], color = :red, markersize = 6)
        text!(ax, x, 3.0, text = "Align", align = al)
    end
    scatter!(ax, [1.0], [1.5], color = :red, markersize = 6)
    text!(ax, 1.0, 1.5, text = "rotated 45°", rotation = pi / 4, fontsize = 18)
    for (i, (f, s)) in enumerate([(:regular, 10), (:bold, 14), (:italic, 18), (:bold_italic, 24)])
        text!(ax, 2.2, 0.4 + 0.5 * (i - 1), text = "Font 0.5 − 1", font = f, fontsize = s)
    end
    fig
end

function stress_bars(d)
    x = [1, 1, 2, 2, 3, 3]
    h = [1.0, 2.0, 2.0, 1.5, 3.0, 2.5]
    g = [1, 2, 1, 2, 1, 2]
    colors = WONG[g]
    fig = Figure(size = (900, 700))
    a = Axis(fig[1, 1], title = "dodge")
    barplot!(a, x, h, dodge = g, color = colors)
    b = Axis(fig[1, 2], title = "stack")
    barplot!(b, x, h, stack = g, color = colors)
    c = Axis(fig[2, 1], title = "direction = x")
    barplot!(c, 1:4, [4.0, 3.0, 5.0, 1.0], direction = :x, color = WONG[4])
    e = Axis(fig[2, 2], title = "hist, 20 bins, stroked")
    hist!(e, d["samples"], bins = 20, strokewidth = 1, strokecolor = :black, color = (WONG[3], 0.7))
    fig
end

function stress_dense(d)
    fig = Figure()
    ax = Axis(fig[1, 1], title = "50 000 points, alpha 0.1")
    scatter!(ax, d["x"], d["y"], markersize = 3, color = (:black, 0.1))
    fig
end

function stress_marker_strokes(d)
    markers = [:circle, :rect, :diamond, :cross, :xcross, :utriangle, :dtriangle, :ltriangle,
        :rtriangle, :pentagon, :hexagon, :star5, Circle, Rect]
    fig = Figure(size = (900, 420))
    ax = Axis(fig[1, 1], title = "stroked markers (1, 4, translucent 3, rotated 2)", limits = (0, 15, 0, 5))
    for (i, m) in enumerate(markers)
        scatter!(ax, [i], [4.0], marker = m, markersize = 34, color = WONG[1], strokewidth = 1,
            strokecolor = :black)
        scatter!(ax, [i], [3.0], marker = m, markersize = 34, color = WONG[3], strokewidth = 4,
            strokecolor = :black)
        scatter!(ax, [i], [2.0], marker = m, markersize = 34, color = (WONG[2], 0.4), strokewidth = 3,
            strokecolor = (:black, 0.6))
        scatter!(ax, [i], [1.0], marker = m, markersize = 18, rotation = 0.4, color = WONG[6],
            strokewidth = 2, strokecolor = :red)
    end
    fig
end

function stress_strokes(d)
    fig = Figure(size = (900, 700))
    a = Axis(fig[1, 1], title = "barplot, strokewidth 4")
    barplot!(a, 1:4, [3.0, -1.0, 2.0, 0.0], color = (WONG[1], 0.6), strokewidth = 4, strokecolor = :black)
    b = Axis(fig[1, 2], title = "direction = x, gap 0")
    barplot!(b, 1:3, [2.0, 4.0, 3.0], direction = :x, gap = 0, color = WONG[2], strokewidth = 2,
        strokecolor = :red)
    c = Axis(fig[2, 1], title = "hist, strokewidth 2")
    hist!(c, d["samples"], bins = 12, color = WONG[3], strokewidth = 2, strokecolor = (:black, 0.7))
    e = Axis(fig[2, 2], title = "band, strokewidth 3")
    band!(e, d["x"], d["lo"], d["hi"], color = (WONG[4], 0.4), strokewidth = 3, strokecolor = WONG[5])
    fig
end

const PAGES = [
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
    ("stress_marker_strokes", stress_marker_strokes),
    ("stress_strokes", stress_strokes),
]

mkpath(OUT)
for (name, f) in PAGES
    (isempty(ARGS) || any(a -> occursin(a, name), ARGS)) || continue
    if !isfile(joinpath(DATA, name * ".json"))
        println("skip ", name, " (run `cargo run --release --example gallery` first)")
        continue
    end
    ppu = name == "s8_paper" ? 300 / 96 : 2
    try
        out(name, f(load(name)); px_per_unit = ppu)
    catch e
        println("FAILED ", name, ": ", sprint(showerror, e))
    finally
        set_theme!()
    end
end
