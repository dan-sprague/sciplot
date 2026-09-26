# Reference for examples/fidelity_check.rs: the same figures rendered by CairoMakie, plus a JSON
# dump of Makie's axis geometry (viewport, final limits, tick-label / axis-label / title bounding
# boxes, figure units, y up) so the example can compare positions numerically.
#
# Run: julia --project=tools tools/fidelity_check.jl
# Writes out/fidelity_<name>_makie.png, out/fidelity_makie.json and the committed fixture
# tests/fixtures/fidelity_makie.json (asserted by tests/fidelity.rs).
using CairoMakie
using JSON

mkpath("out")

rect(r) = (x = Float64(r.origin[1]), y = Float64(r.origin[2]), w = Float64(r.widths[1]), h = Float64(r.widths[2]))

function textboxes(p)
    bbs = Makie.string_boundingboxes(p)
    return [rect(b) for b in bbs if all(isfinite, b.widths)]
end

function geometry(ax)
    vp = ax.scene.viewport[]
    lims = ax.finallimits[]
    return Dict(
        "viewport" => rect(vp),
        "limits" => [lims.origin[1], lims.origin[1] + lims.widths[1], lims.origin[2], lims.origin[2] + lims.widths[2]],
        "xticklabels" => textboxes(ax.xaxis.elements[:ticklabels]),
        "yticklabels" => textboxes(ax.yaxis.elements[:ticklabels]),
        "xlabel" => textboxes(ax.xaxis.elements[:labeltext]),
        "ylabel" => textboxes(ax.yaxis.elements[:labeltext]),
        "title" => textboxes(ax.elements[:title]),
        "xtickvalues" => ax.xaxis.tickvalues[],
        "ytickvalues" => ax.yaxis.tickvalues[],
    )
end

results = Dict{String, Any}()

function finish(name, fig, ax)
    save("out/fidelity_$(name)_makie.png", fig; px_per_unit = 2)
    results[name] = geometry(ax)
end

t = collect(0.0:10.0)
xs = @. 5.0 * cos(0.8 * t) * exp(-0.1 * t)

# 1. Default axis with title and labels.
fig = Figure()
ax = Axis(fig[1, 1]; title = "Harmonic oscillator", xlabel = "time t (s)", ylabel = "displacement x (mm)")
scatter!(ax, t, xs)
finish("default", fig, ax)

# 2. Log-log with minor ticks and minor grid.
lx = [10.0^(-1 + 4 * i / 49) for i in 0:49]
ly = @. 100.0 * lx^-1.5
fig = Figure()
ax = Axis(fig[1, 1]; title = "power law", xlabel = "k", ylabel = "E(k)", xscale = log10, yscale = log10,
    xminorticksvisible = true, yminorticksvisible = true, xminorgridvisible = true, yminorgridvisible = true)
lines!(ax, lx, ly)
finish("loglog", fig, ax)

# 3. theme_minimal.
with_theme(theme_minimal()) do
    fig = Figure()
    ax = Axis(fig[1, 1]; title = "minimal", xlabel = "time t (s)", ylabel = "x")
    lines!(ax, t, xs)
    scatter!(ax, t, xs)
    finish("minimal", fig, ax)
end

# 4. DataAspect heatmap (40 × 20 cells centred on 1..40, 1..20).
z = [sin(0.3 * i) * cos(0.4 * j) for i in 1:40, j in 1:20]
fig = Figure()
ax = Axis(fig[1, 1]; title = "DataAspect", aspect = DataAspect())
heatmap!(ax, z)
finish("dataaspect", fig, ax)

# 5. hlines / vlines / ablines.
tt = [10.0 * i / 99 for i in 0:99]
fig = Figure()
ax = Axis(fig[1, 1]; title = "reference lines")
lines!(ax, tt, sin.(tt))
hlines!(ax, [0.5, -0.5]; xmin = 0.1, xmax = 0.9)
vlines!(ax, [2.0, 4.0])
ablines!(ax, -1.0, 0.2)
finish("reflines", fig, ax)

# 6. autolimitaspect = 1 on a unit circle (limits widen, the axis keeps its cell).
th = [2pi * i / 99 for i in 0:99]
fig = Figure()
ax = Axis(fig[1, 1]; title = "autolimitaspect", autolimitaspect = 1)
lines!(ax, cos.(th), sin.(th))
finish("autolimitaspect", fig, ax)

# 7. theme_light and theme_dark (visual only).
for (name, th) in (("light", theme_light()), ("dark", theme_dark()))
    with_theme(th) do
        fig = Figure()
        ax = Axis(fig[1, 1]; title = name, xlabel = "time t (s)", ylabel = "x")
        lines!(ax, t, xs)
        lines!(ax, t, -xs)
        finish(name, fig, ax)
    end
end

for path in ("out/fidelity_makie.json", "tests/fixtures/fidelity_makie.json")
    open(path, "w") do io
        JSON.print(io, results, 1)
    end
end
println("wrote out/fidelity_*_makie.png, out/fidelity_makie.json and tests/fixtures/fidelity_makie.json")
