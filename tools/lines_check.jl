# CairoMakie reference for examples/line_torture.rs (second figure): sin/cos lines, a dashed line
# and scatterlines with default styling. Run: julia --project=tools tools/lines_check.jl
using CairoMakie

x = range(0, 10, length = 100)
fig = Figure()
ax = Axis(fig[1, 1])
lines!(ax, x, sin.(x))
lines!(ax, x, cos.(x); linewidth = 4)
lines!(ax, x, 0.5 .* sin.(2 .* x); linestyle = :dash, linewidth = 2)
scatterlines!(ax, [1.0, 3.0, 5.0, 7.0, 9.0], [-0.8, 0.6, -0.4, 0.9, -0.9])
mkpath("out")
save("out/lines_makie.png", fig; px_per_unit = 2)
println("wrote out/lines_makie.png")
