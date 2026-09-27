# CairoMakie reference for examples/colorbar_check.rs (same data, same figures):
#
#   julia --project=tools tools/colorbar_check.jl
#
# Writes out/colorbar_check_makie.png and out/colorbar_check_clips_makie.png, and prints each
# colorbar's frame box, protrusion and tick values/labels for comparison with the sciplot run.

using CairoMakie

bumps(x, y) = exp(-2 * ((x - 0.6)^2 + 4 * (y - 0.1)^2)) + 0.6 * exp(-(3 * (x + 0.9)^2 + 10 * (y + 0.3)^2))

mkpath("out")

function report(name, cb)
    bb = cb.layoutobservables.computedbbox[]
    ax = cb.axis
    println(rpad(name, 22), "bbox (l, b, w, h) = ", round.(Float64[bb.origin..., bb.widths...], digits = 2),
        "  protrusion = ", round(Float64(ax.protrusion[]), digits = 3),
        "  ticks = ", ax.tickvalues[], " ", ax.ticklabels[])
end

# 1. S4: heatmap (magma) with a labeled colorbar; value-colored scatter with a colorbar;
#    a horizontal colorbar below an axis.
nx, ny = 200, 100
xs = range(-2, 2, length = nx)
ys = range(-1, 1, length = ny)
z = [bumps(x, y) for x in xs, y in ys]
fig = Figure(size = (900, 700))
ax1 = Axis(fig[1, 1], title = "heatmap, magma")
hm = heatmap!(ax1, -2 .. 2, -1 .. 1, z, colormap = :magma)
cb1 = Colorbar(fig[1, 2], hm, label = "amplitude")

n = 200
t = [4pi * i / (n - 1) for i in 0:n-1]
ax2 = Axis(fig[1, 3], title = "scatter, color = values")
sc = scatter!(ax2, t .* cos.(t), t .* sin.(t), color = t .* 10, markersize = 8)
cb2 = Colorbar(fig[1, 4], sc)

ax3 = Axis(fig[2, 1:3], title = "horizontal colorbar below")
lines!(ax3, 0:0.1:10, sin.(0:0.1:10))
cb3 = Colorbar(fig[3, 1:3], colormap = :viridis, limits = (-1, 1), vertical = false, label = "horizontal")
save("out/colorbar_check_makie.png", fig, px_per_unit = 2)
report("heatmap (magma)", cb1)
report("scatter values", cb2)
report("horizontal", cb3)

# 2. lowclip / highclip, flipaxis = false, horizontal on top.
fig2 = Figure(size = (700, 400))
a = Axis(fig2[1, 2], title = "clips")
w = [sin(i / 3) * cos(j / 4) * 1.4 for i in 1:40, j in 1:30]
hm2 = heatmap!(a, w, colorrange = (-1, 1), lowclip = :cyan, highclip = :red, colormap = :RdBu)
cb4 = Colorbar(fig2[1, 3], hm2, label = "clipped")
cb5 = Colorbar(fig2[1, 1], hm2, flipaxis = false, label = "left side")
cb6 = Colorbar(fig2[0, 2], colormap = :plasma, limits = (0, 1000), vertical = false, highclip = :black, label = "top")
save("out/colorbar_check_clips_makie.png", fig2, px_per_unit = 2)
report("clips right", cb4)
report("clips left", cb5)
report("top, highclip", cb6)
println("wrote out/colorbar_check_makie.png and out/colorbar_check_clips_makie.png")
