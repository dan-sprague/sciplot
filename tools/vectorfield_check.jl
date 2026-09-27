# CairoMakie reference for examples/vectorfield_check.rs (same fields, same figures):
#
#   julia --project=tools tools/vectorfield_check.jl
#
# Writes out/vectorfield_{quiver,pendulum,vdp}_makie.png (px_per_unit = 2) and prints the
# streamplot statistics (seed count, line points, color range) for comparison with the sciplot run.

using CairoMakie
using CairoMakie.Makie: norm

mkpath("out")

# Damped pendulum: θ' = ω, ω' = -sin θ - 0.2 ω.
pendulum(θ, ω) = Point2(ω, -sin(θ) - 0.2 * ω)
# Van der Pol oscillator, μ = 1.
vdp(x, y) = Point2(y, (1 - x^2) * y - x)

function report(name, p)
    pts = p.line_points[]
    cols = p.line_colors[]
    println(rpad(name, 12), "seeds = ", length(p.arrow_positions[]),
        "  line points = ", length(pts),
        "  color range = ", round.(Float64.(extrema(filter(isfinite, cols))), digits = 6),
        "  first seeds = ", [round.(Float64.(Tuple(q)), digits = 6) for q in p.arrow_positions[][1:3]])
end

# (a) Quiver on a 20 × 20 grid: normalized arrows colored by the field's magnitude.
xs = range(-pi, pi, length = 20)
ys = range(-3, 3, length = 20)
pts = vec([Point2(x, y) for x in xs, y in ys])
dirs = [pendulum(p...) for p in pts]
fig = Figure(size = (600, 450))
ax = Axis(fig[1, 1], title = "damped pendulum: arrows", xlabel = "θ", ylabel = "ω")
ar = arrows2d!(ax, pts, dirs, color = norm.(dirs), normalize = true, lengthscale = 0.25, align = :center)
Colorbar(fig[1, 2], limits = extrema(norm.(dirs)), colormap = :viridis, label = "|f|")
save("out/vectorfield_quiver_makie.png", fig, px_per_unit = 2)

# (b) Streamplot of the same field.
fig = Figure(size = (600, 450))
ax = Axis(fig[1, 1], title = "damped pendulum: streamplot", xlabel = "θ", ylabel = "ω")
sp = streamplot!(ax, pendulum, -pi .. pi, -3 .. 3)
save("out/vectorfield_pendulum_makie.png", fig, px_per_unit = 2)
report("pendulum", sp)

# (c) Van der Pol, magma, thinner lines, smaller arrows, lower density.
fig = Figure(size = (600, 450))
ax = Axis(fig[1, 1], title = "Van der Pol (μ = 1)", xlabel = "x", ylabel = "y")
sp2 = streamplot!(ax, vdp, -3 .. 3, -4 .. 4, colormap = :magma, linewidth = 1, arrow_size = 10, density = 0.8,
    gridsize = (24, 24))
save("out/vectorfield_vdp_makie.png", fig, px_per_unit = 2)
report("vdp", sp2)

# Arrow geometry in isolation: default metrics, long and short arrows (the short ones scale down).
fig = Figure(size = (600, 450))
ax = Axis(fig[1, 1], title = "arrows2d metrics", limits = (0, 10, 0, 6))
arrows2d!(ax, [Point2(1.0, 1.0), Point2(1.0, 2.0), Point2(1.0, 3.0), Point2(1.0, 4.0), Point2(1.0, 5.0)],
    [Vec2(8.0, 0.0), Vec2(0.3, 0.0), Vec2(0.15, 0.0), Vec2(4.0, 0.5), Vec2(0.05, 0.0)])
arrows2d!(ax, [Point2(6.0, 2.0)], [Vec2(2.0, 2.0)], color = :red, shaftwidth = 6, tipwidth = 20, tiplength = 14, align = :center)
arrows2d!(ax, [Point2(6.0, 5.0)], [Vec2(3.0, 0.0)], color = :blue, taillength = 8, tailwidth = 12)
save("out/vectorfield_metrics_makie.png", fig, px_per_unit = 2)

# Legend entries and a solid-colored streamplot (sciplot's legend entries differ on purpose: a line
# with an arrowhead for both plot types).
fig = Figure(size = (600, 450))
ax = Axis(fig[1, 1], title = "legend")
g = range(-2, 2, length = 9)
arrows2d!(ax, g, g, p -> Point2(-p[2], p[1]), lengthscale = 0.2, color = Makie.wong_colors()[2], label = "rotation")
streamplot!(ax, (x, y) -> Point2(x, y), -2 .. 2, -2 .. 2, color = p -> Makie.wong_colors()[1], density = 0.3,
    label = "source")
axislegend(ax)
save("out/vectorfield_legend_makie.png", fig, px_per_unit = 2)
println("wrote out/vectorfield_*_makie.png")
