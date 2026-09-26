# CairoMakie reference for examples/heatmap_check.rs (same data, same figure):
#
#   julia --project=tools tools/heatmap_check.jl
#
# Writes out/heatmap_check_makie.png and out/heatmap_check_irregular_makie.png, and prints the
# axis limits of heatmap(0..1, 0..1, 4x3 matrix) that tests/heatmap.rs asserts.

using CairoMakie

bumps(x, y) = exp(-2 * ((x - 0.6)^2 + 4 * (y - 0.1)^2)) + 0.6 * exp(-(3 * (x + 0.9)^2 + 10 * (y + 0.3)^2))

mkpath("out")

# 1. 400x200 field with magma + explicit colorrange; value-colored scatter.
nx, ny = 400, 200
xs = range(-2, 2, length = nx)
ys = range(-1, 1, length = ny)
z = [bumps(x, y) for x in xs, y in ys]
fig = Figure(size = (900, 400))
ax1 = Axis(fig[1, 1], title = "heatmap, magma, colorrange (0, 0.8)")
heatmap!(ax1, -2 .. 2, -1 .. 1, z, colormap = :magma, colorrange = (0, 0.8))
n = 300
t = [4pi * i / (n - 1) for i in 0:n-1]
ax2 = Axis(fig[1, 2], title = "scatter, color = values")
scatter!(ax2, t .* cos.(t), t .* sin.(t), color = t, markersize = 10)
save("out/heatmap_check_makie.png", fig, px_per_unit = 2)

# 2. Irregular edges, NaN, clip colors, log y; interpolation.
xe = [0.0, 1.0, 1.5, 3.0, 3.2, 5.0]
yc = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
nx, ny = length(xe) - 1, length(yc)
w = [(i - 1) + 0.5 * (j - 1) for i in 1:nx, j in 1:ny]
w[2, 3] = NaN
fig2 = Figure(size = (900, 400))
a = Axis(fig2[1, 1], title = "irregular, NaN, clips", yscale = log10)
heatmap!(a, xe, yc, w, colorrange = (1, 6), lowclip = :cyan, highclip = :red)
b = Axis(fig2[1, 2], title = "interpolate = true")
mx, my = 8, 6
v = [sin((i - 1) * 0.8) * cos((j - 1) * 0.9) for i in 1:mx, j in 1:my]
heatmap!(b, v, interpolate = true, colormap = :RdBu)
save("out/heatmap_check_irregular_makie.png", fig2, px_per_unit = 2)

# Limits fixture for the coordinate-conversion test.
f3, ax3, _ = heatmap(0 .. 1, 0 .. 1, reshape(collect(1.0:12.0), 4, 3))
Makie.update_state_before_display!(f3)
lims = ax3.finallimits[]
println("heatmap(0..1, 0..1, 4x3) limits: x = ", (lims.origin[1], lims.origin[1] + lims.widths[1]),
        ", y = ", (lims.origin[2], lims.origin[2] + lims.widths[2]))
println("wrote out/heatmap_check_makie.png and out/heatmap_check_irregular_makie.png")
