# CairoMakie reference for examples/legend_check.rs: the same four figures, written to
# out/legend_check_*_makie.png. Run: julia --project=tools tools/legend_check.jl
#
# With `dump = true` (default) it also prints each legend's computed bbox (Makie y-up units,
# [x, y, w, h]) so placement can be compared numerically with sciplot's.
using CairoMakie

mkpath("out")
report(name, leg) = println(name, ": legend bbox = ", round.([minimum(leg.layoutobservables.computedbbox[])..., widths(leg.layoutobservables.computedbbox[])...]; digits = 2))

t = range(0, 10, length = 200)
td = range(0.25, 9.75, length = 20)

# S2: sin/cos with axislegend (top right).
fig = Figure()
ax = Axis(fig[1, 1]; xlabel = "t", ylabel = "u")
lines!(ax, t, sin.(t); label = "sin")
lines!(ax, t, cos.(t); label = "cos")
leg = axislegend(ax)
save("out/legend_check_s2_makie.png", fig; px_per_unit = 2)
report("s2", leg)

# S3-like: three axes with lines + scatter, one shared legend (unique) spanning two rows.
fig = Figure(size = (900, 650))
a = Axis(fig[1, 1]; title = "ω = 1", ylabel = "u (V)")
b = Axis(fig[1, 2]; title = "ω = 2")
c = Axis(fig[2, 1:2]; xlabel = "t (s)")
for (k, axk) in enumerate((a, b, c))
    lines!(axk, t, sin.(k .* t); label = "model")
    scatter!(axk, td, sin.(k .* td) .+ 0.1 .* cos.(7 .* td); label = "measurement", markersize = 7)
end
lines!(c, t, 0.5 .* cos.(t); label = "envelope", linestyle = :dash)
leg = Legend(fig[1:2, 3], [a, b, c]; unique = true)
save("out/legend_check_s3_makie.png", fig; px_per_unit = 2)
report("s3", leg)

# Horizontal legend under an axis, with a title.
fig = Figure()
ax = Axis(fig[1, 1])
lines!(ax, t, sin.(t); label = "solid")
lines!(ax, t, sin.(t .- 1); label = "dash", linestyle = :dash)
lines!(ax, t, sin.(t .- 2); label = "dot", linestyle = :dot, linewidth = 3)
scatterlines!(ax, 1:9, 0.2 .* cos.(1:9); label = "scatterlines")
leg = Legend(fig[2, 1], ax, "Styles"; orientation = :horizontal)
save("out/legend_check_horizontal_makie.png", fig; px_per_unit = 2)
report("horizontal", leg)

# Bars, band and stroked markers with an axislegend at the left top, with a title.
fig = Figure()
ax = Axis(fig[1, 1])
band!(ax, t, sin.(t) .- 0.3 .+ 3, sin.(t) .+ 0.3 .+ 3; label = "band")
barplot!(ax, 1:9, 1 .+ 0.1 .* (1:9); label = "bars")
barplot!(ax, 1:9, 0.5 .+ 0.05 .* (1:9); label = "bars 2", strokewidth = 1, strokecolor = :black)
scatter!(ax, 1:9, 2 .+ 0.1 .* (1:9); label = "stroked", marker = :rect, markersize = 12, strokewidth = 1, color = :orange)
leg = axislegend(ax, "Kinds"; position = :lt)
save("out/legend_check_bars_makie.png", fig; px_per_unit = 2)
report("bars", leg)
println("wrote out/legend_check_*_makie.png")
