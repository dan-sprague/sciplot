# Reference for examples/text_check.rs: the same figure rendered by CairoMakie.
# Run: julia --project=tools tools/text_check.jl   (writes out/text_check_makie.png)
using CairoMakie

t = collect(0.0:10.0)
x = @. 5.0 * cos(0.8 * t) * exp(-0.1 * t)

fig = Figure()
ax = Axis(fig[1, 1]; title = "Harmonic oscillator", xlabel = "time t (s)", ylabel = "displacement x (mm)")
scatter!(ax, t, x)

mkpath("out")
save("out/text_check_makie.png", fig; px_per_unit = 2)

fig = Figure()
ax = Axis(fig[1, 1];
    title = rich("E = mc", superscript("2"), rich(" (rest energy)"; color = Makie.wong_colors()[6])),
    xlabel = rich("wavenumber k (μm", superscript("−1"), ")"),
    ylabel = rich("E(k) = C k", superscript("−5/3"), ", α = 0.5 ± 0.1"))
scatter!(ax, t, x)
save("out/text_check_rich_makie.png", fig; px_per_unit = 2)
println("wrote out/text_check_makie.png and out/text_check_rich_makie.png")
