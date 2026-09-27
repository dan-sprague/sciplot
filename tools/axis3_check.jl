# CairoMakie reference for examples/axis3_check.rs (same data, same figures):
#
#   julia --project=tools tools/axis3_check.jl
#
# Writes out/axis3_{lorenz,surface,cloud}_{a,b}_makie.png.

using CairoMakie

function lorenz(n, dt)
    f(p) = (10.0 * (p[2] - p[1]), p[1] * (28.0 - p[3]) - p[2], p[1] * p[2] - 8.0 / 3.0 * p[3])
    p = (1.0, 1.0, 1.0)
    xs, ys, zs = [p[1]], [p[2]], [p[3]]
    for _ in 1:n
        k1 = f(p)
        k2 = f(ntuple(i -> p[i] + 0.5 * dt * k1[i], 3))
        k3 = f(ntuple(i -> p[i] + 0.5 * dt * k2[i], 3))
        k4 = f(ntuple(i -> p[i] + dt * k3[i], 3))
        p = ntuple(i -> p[i] + dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]), 3)
        push!(xs, p[1]); push!(ys, p[2]); push!(zs, p[3])
    end
    return xs, ys, zs
end

potential(x, y) = (x * x - 1.0)^2 + 0.8 * y * y - 0.3 * x

function cloud(n)
    xs, ys, zs = Float64[], Float64[], Float64[]
    for k in 0:(n - 1)
        z = 1.0 - 2.0 * (k + 0.5) / n
        th = 2.399963229728653 * k
        r = sqrt(1.0 - z * z) * (1.0 + 0.25 * sin(5.0 * th))
        push!(xs, r * cos(th)); push!(ys, r * sin(th)); push!(zs, z)
    end
    return xs, ys, zs
end

const VIEWS = [("a", 1.275pi, pi / 8, 0.0), ("b", 0.3pi, 0.45, 0.5)]

mkpath("out")
lx, ly, lz = lorenz(4000, 0.01)
t = [(i - 1) * 0.01 for i in eachindex(lx)]
nx, ny = 60, 50
gx = range(-1.8, 1.8, length = nx)
gy = range(-1.5, 1.5, length = ny)
v = [potential(x, y) for x in gx, y in gy]
px = range(-1.6, 1.6, length = 200)
py = [0.4 * sin(2x) for x in px]
pz = [potential(x, y) + 0.05 for (x, y) in zip(px, py)]
cx, cy, cz = cloud(600)

for (suffix, az, el, persp) in VIEWS
    fig = Figure()
    ax = Axis3(fig[1, 1], title = "Lorenz attractor", azimuth = az, elevation = el, perspectiveness = persp)
    lines!(ax, lx, ly, lz, color = t, linewidth = 1.0)
    save("out/axis3_lorenz_$(suffix)_makie.png", fig, px_per_unit = 2)

    fig = Figure()
    ax = Axis3(fig[1, 1], title = "double-well potential", xlabel = "x", ylabel = "y", zlabel = "V",
        azimuth = az, elevation = el, perspectiveness = persp)
    s = surface!(ax, gx, gy, v)
    lines!(ax, px, py, pz, color = :red, linewidth = 2.0)
    Colorbar(fig[1, 2], s, label = "V")
    save("out/axis3_surface_$(suffix)_makie.png", fig, px_per_unit = 2)

    fig = Figure()
    ax = Axis3(fig[1, 1], title = "point cloud", aspect = :data, azimuth = az, elevation = el,
        perspectiveness = persp)
    scatter!(ax, cx, cy, cz, color = cz, markersize = 8)
    save("out/axis3_cloud_$(suffix)_makie.png", fig, px_per_unit = 2)
end
println("wrote out/axis3_*_makie.png")
