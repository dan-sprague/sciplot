# CairoMakie reference for examples/contour_check.rs (same data, same figures):
#
#   julia --project=tools tools/contour_check.jl
#
# Writes out/contour_check_makie.png and out/contour_check_extend_makie.png, and prints the
# automatic levels, the contourf band colors and the axis limits that the Rust tests assert.

using CairoMakie

function mixture(x, y)
    g(x0, y0, s, a) = a * exp(-((x - x0)^2 + (y - y0)^2) / (2 * s * s))
    return g(-1.0, -0.5, 0.6, 1.0) + g(1.2, 0.8, 0.8, 0.8) + g(0.8, -1.2, 0.4, 0.6)
end

const I_EXT, A, B, EPS = 0.5, 0.7, 0.8, 0.08
fhn(v, w) = (v - v^3 / 3 - w + I_EXT, EPS * (v + A - B * w))

function trajectory(v, w, dt, n)
    vs, ws = [v], [w]
    for _ in 1:n
        k1 = fhn(v, w)
        k2 = fhn(v + 0.5dt * k1[1], w + 0.5dt * k1[2])
        k3 = fhn(v + 0.5dt * k2[1], w + 0.5dt * k2[2])
        k4 = fhn(v + dt * k3[1], w + dt * k3[2])
        v += dt / 6 * (k1[1] + 2k2[1] + 2k3[1] + k4[1])
        w += dt / 6 * (k1[2] + 2k2[2] + 2k3[2] + k4[2])
        push!(vs, v); push!(ws, w)
    end
    return vs, ws
end

mkpath("out")

nx, ny = 120, 100
xs = range(-3, 3, length = nx)
ys = range(-2.5, 2.5, length = ny)
z = [mixture(x, y) for x in xs, y in ys]

fig = Figure(size = (1000, 800))
a = Axis(fig[1, 1], title = "contour, levels = 8")
c8 = contour!(a, xs, ys, z, levels = 8)
b = Axis(fig[1, 2], title = "contourf, levels = 8")
cf = contourf!(b, xs, ys, z, levels = 8)
Colorbar(fig[1, 3], cf)
c = Axis(fig[2, 1], title = "labels = true, dashed")
contour!(c, xs, ys, z, levels = 6, labels = true, linestyle = :dash, colormap = :magma)

n, m = 200, 150
vs = range(-2.5, 2.5, length = n)
ws = range(-1.0, 2.0, length = m)
f = [fhn(v, w)[1] for v in vs, w in ws]
g = [fhn(v, w)[2] for v in vs, w in ws]
d = Axis(fig[2, 2:3], title = "FitzHugh–Nagumo nullclines", xlabel = "v", ylabel = "w")
tv, tw = trajectory(-2.0, -0.5, 0.05, 2000)
lines!(d, tv, tw, color = (:gray, 0.8), linewidth = 1)
contour!(d, vs, ws, f, levels = [0.0], color = :red, linewidth = 2, labels = true,
    labelformatter = _ -> "v' = 0", labelsize = 12)
contour!(d, vs, ws, g, levels = [0.0], color = :blue, linewidth = 2, labels = true,
    labelformatter = _ -> "w' = 0", labelsize = 12)
save("out/contour_check_makie.png", fig, px_per_unit = 2)

fig2 = Figure(size = (900, 400))
e = Axis(fig2[1, 1], title = "levels = 0.1:0.1:0.6, extend auto")
cf2 = contourf!(e, xs, ys, z, levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6], extendlow = :auto,
    extendhigh = :auto, colormap = :plasma)
contour!(e, xs, ys, z, levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6], color = :black, linewidth = 0.75)
Colorbar(fig2[1, 2], cf2)
h = Axis(fig2[1, 3], title = "mode = relative, extendhigh = red")
cf3 = contourf!(h, xs, ys, z, levels = [0.1, 0.3, 0.5, 0.7], mode = :relative, extendhigh = :red)
Colorbar(fig2[1, 4], cf3)
save("out/contour_check_extend_makie.png", fig2, px_per_unit = 2)

# Fixtures for the Rust tests.
println("contour levels = 8: ", collect(c8.zlevels[]))
println("contourf levels = 8: ", cf.computed_levels[])
println("contourf band colors (viridis, 8): ", [(c.r, c.g, c.b) for c in cf.computed_colormap[].colors])
println("extend auto band colors (plasma, 5): ", [(c.r, c.g, c.b) for c in cf2.computed_colormap[].colors])
for (name, ax) in (("contour", a), ("contourf", b), ("contourf extend", e))
    Makie.update_state_before_display!(ax.parent)
    l = ax.finallimits[]
    println("$name limits: x = ", (l.origin[1], l.origin[1] + l.widths[1]), ", y = ", (l.origin[2], l.origin[2] + l.widths[2]))
end
println("wrote out/contour_check_makie.png and out/contour_check_extend_makie.png")
