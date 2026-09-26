# Generates tests/fixtures/ticks.json: reference values from the local PlotUtils 1.5 / Makie 0.24
# (and Julia Base numerics they depend on) for tests/ticks.rs.
#
#   julia --project=tools tools/gen_tick_fixtures.jl
#
# Floats are written as `repr` strings (shortest round-trip), so the Rust side parses them exactly.
# Labels are lists of [text, kind] spans, kind = "plain" | "sup" | "sub".

using Makie, PlotUtils, JSON, Random

const OUT = joinpath(@__DIR__, "..", "tests", "fixtures", "ticks.json")
rng = MersenneTwister(20260926)

f(x) = repr(Float64(x))
fs(xs) = [f(x) for x in xs]

function spans(l)
    out = Any[]
    walk(x::AbstractString, kind) = push!(out, [String(x), kind])
    function walk(r::Makie.RichText, kind)
        k = r.type === :sup ? "sup" : r.type === :sub ? "sub" : kind
        for c in r.children
            walk(c, k)
        end
    end
    walk(l, "plain")
    return out
end
labels(ls) = [spans(l) for l in ls]

# ---------------------------------------------------------------------------------------------
# Linear ranges

lin_cases = Tuple{Float64, Float64}[]
push!(lin_cases, (0.55, 10.45), (0.5, 10.5), (-1.1, 1.1), (-0.5, 10.5), (-0.04, 1.04), (-3.95, 104.95),
    (-0.15, 3.15), (0.95, 2.05), (12.3, 98.7), (-1.0, 2.0), (1e11 - 1, 1e11 + 2), (1e11 - 2, 1e11 + 2),
    (1e9 - 0.5, 1e9 + 0.5), (1.0, 1.0 + 1e-13), (1.0, 1.0 + 1e-12), (1.0, 1.0 + 1e-10), (1.0, nextfloat(1.0)),
    (3.0, 3.0), (0.0, 0.0), (-1e-300, 1e-300), (0.0, 1e-20), (1e-20, 3e-20), (0.0, 1e-15), (0.0, 1e-14),
    (1e300, 5e300), (-1e308, 1e308), (0.0, 1e308), (1e-310, 3e-310), (-5.0, -1.0), (-1e6, -1e3),
    (-0.3, -0.1), (0.0, 1.0), (0.0, 1e4), (0.0, 1e5), (0.0, 12345.0), (-12345.0, 0.0), (0.0, 1e-4),
    (0.0, 1.5e-4), (0.0, 3e-5), (1e5, 1e5 + 1), (-2.2e-16, 2.2e-16), (0.1, 0.30000000000000004),
    (1e15, 1e15 + 10), (1e16, 1e16 + 64), (6.02e23, 6.03e23), (-1e-5, 1e-5), (0.999, 1.001), (99.0, 101.0))
for n in 2:60
    push!(lin_cases, (1 - 0.05 * (n - 1), n + 0.05 * (n - 1)), (0.5, n + 0.5), (0.0, Float64(n)), (-Float64(n), Float64(n)))
end
for e in -12:12
    push!(lin_cases, (0.0, 10.0^e), (-(10.0^e), 10.0^e), (10.0^e, 3 * 10.0^e))
end
randsign() = rand(rng, Bool) ? 1.0 : -1.0
for i in 1:450
    w = 10.0^(rand(rng) * 20 - 10) * (0.5 + rand(rng))
    c = rand(rng) < 0.3 ? 0.0 : randsign() * 10.0^(rand(rng) * 24 - 12) * rand(rng)
    lo = c - w * rand(rng)
    push!(lin_cases, (lo, lo + w))
end
for i in 1:250  # autolimits: data extrema with 5 % margins
    a = randsign() * 10.0^(rand(rng) * 8 - 4) * rand(rng)
    b = a + 10.0^(rand(rng) * 8 - 4) * rand(rng)
    d = b - a
    push!(lin_cases, (a - 0.05d, b + 0.05d))
end
for i in 1:40  # relative widths close to the fallback threshold
    c = randsign() * 10.0^(rand(rng) * 10 - 5)
    w = abs(c) * eps() * 10.0^(rand(rng) * 6)
    push!(lin_cases, (c, c + w))
end

wilk = Any[]
for (lo, hi) in lin_cases
    try
        vals, labs = Makie.get_ticks(Makie.automatic, identity, Makie.automatic, lo, hi)
        push!(wilk, Dict("lo" => f(lo), "hi" => f(hi), "k_ideal" => 5, "k_min" => 3, "k_max" => 10,
            "values" => fs(vals), "labels" => labels(labs)))
    catch err
        @warn "skipping" lo hi err
    end
end

# Other WilkinsonTicks parameters (Makie wrapper → PlotUtils.optimize_ticks)
param_sets = [(5, 2, 10), (3, 2, 10), (7, 5, 12), (4, 3, 6), (10, 3, 20), (2, 2, 3)]
for i in 1:220
    lo, hi = lin_cases[rand(rng, 1:length(lin_cases))]
    k_ideal, k_min, k_max = param_sets[rand(rng, 1:length(param_sets))]
    try
        vals = Makie.get_tickvalues(Makie.WilkinsonTicks(k_ideal; k_min, k_max), lo, hi)
        push!(wilk, Dict("lo" => f(lo), "hi" => f(hi), "k_ideal" => k_ideal, "k_min" => k_min, "k_max" => k_max,
            "values" => fs(vals)))
    catch err
        @warn "skipping" lo hi err
    end
end
# PlotUtils' own test vectors (default k_min = 2)
for (lo, hi) in [(-1.0, 2.0), (1e11 - 1, 1e11 + 2)]
    vals = PlotUtils.optimize_ticks(lo, hi)[1]
    push!(wilk, Dict("lo" => f(lo), "hi" => f(hi), "k_ideal" => 5, "k_min" => 2, "k_max" => 10, "values" => fs(vals)))
end

# ---------------------------------------------------------------------------------------------
# Label formatting on synthetic tick vectors

fmt = Any[]
fmt_inputs = Vector{Vector{Float64}}()
push!(fmt_inputs, [0.0, 2.5, 5.0, 7.5, 10.0], [0.0, 5.0, 10.0], [0.1, 0.2, 0.30000000000000004],
    [1.5e-5, 2e-5, 2.5e-5], [0.0, 5e5, 1e6], [-1e6, 0.0, 1e6], [1e15 + 0.5, 1e15 + 1.5, 1e15 + 2.5],
    [0.25, 0.75], [1e20, 2e20, 3e20], [1e-17, 2e-17], [0.0, 1e-17], [1e300, 2e300], [-2.5e-10, 0.0, 2.5e-10],
    [1.0e8 + 1, 1.0e8 + 2], [123456.0, 123457.0], [0.0], [42.0], [-0.0, 1.0], [1e39, 2e39],
    [1.25e-7, 1.5e-7, 1.75e-7], [0.001, 0.002, 0.003], [9.5, 10.0, 10.5], [-0.5, 0.0, 0.5])
for i in 1:300
    step = 10.0^rand(rng, -9:9) * rand(rng, [1.0, 2.0, 2.5, 5.0, 3.0, 0.1, 0.3])
    n = rand(rng, 2:10)
    start = rand(rng, -6:6) * step
    if rand(rng) < 0.3
        start += randsign() * 10.0^(rand(rng) * 14 - 2)
    end
    push!(fmt_inputs, [start + i * step for i in 0:(n - 1)])
end
for xs in fmt_inputs
    push!(fmt, Dict("values" => fs(xs), "labels" => labels(Makie.format_ticks_auto(xs))))
end

# ---------------------------------------------------------------------------------------------
# Log ticks (Makie LogTicks)

log_cases = Tuple{Float64, Float64, String}[]
push!(log_cases, (1.0, 1000.0, "log10"), (0.8, 1200.0, "log10"), (1.0, 10.0, "log10"), (1.0, 1.5, "log10"),
    (1e-300, 1e300, "log10"), (0.5, 50.0, "log10"), (1e-3, 1e5, "log10"), (0.7079457843841379, 1412.537544622754, "log10"),
    (1.0, 1024.0, "log2"), (0.3, 70.0, "log2"), (1.0, 1000.0, "log"), (0.01, 5.0, "log"))
for i in 1:260
    lo = 10.0^(rand(rng) * 40 - 20)
    hi = lo * 10.0^(rand(rng) < 0.3 ? rand(rng) * 1.5 + 0.02 : rand(rng) * 30 + 0.05)
    push!(log_cases, (lo, hi, rand(rng) < 0.8 ? "log10" : rand(rng, ["log2", "log"])))
end
scalefn = Dict("log10" => log10, "log2" => log2, "log" => log)
logt = Any[]
for (lo, hi, s) in log_cases
    try
        vals, labs = Makie.get_ticks(Makie.LogTicks(Makie.WilkinsonTicks(5, k_min = 3)), scalefn[s], Makie.automatic, lo, hi)
        push!(logt, Dict("lo" => f(lo), "hi" => f(hi), "scale" => s, "values" => fs(vals), "labels" => labels(labs)))
    catch err
        @warn "skipping log" lo hi err
    end
end

# ---------------------------------------------------------------------------------------------
# Minor ticks (IntervalsBetween), on the limits-filtered majors like the Axis does

within(tv, lo, hi) = Makie.is_within_limits(tv, (lo, hi))
minor = Any[]
for c in wilk[1:min(end, 500)]
    c["k_min"] == 3 || continue
    lo, hi = parse(Float64, c["lo"]), parse(Float64, c["hi"])
    ticks = filter(t -> within(t, lo, hi), parse.(Float64, c["values"]))
    n = rand(rng, [2, 2, 3, 4, 5, 9, 10])
    try
        v = Makie.get_minor_tickvalues(Makie.IntervalsBetween(n), identity, ticks, lo, hi)
        length(v) > 10_000 && continue
        push!(minor, Dict("n" => n, "scale" => "identity", "lo" => f(lo), "hi" => f(hi), "ticks" => fs(ticks), "values" => fs(v)))
    catch err
        @warn "skipping minor" lo hi err
    end
end
for c in logt
    lo, hi = parse(Float64, c["lo"]), parse(Float64, c["hi"])
    ticks = filter(t -> within(t, lo, hi), parse.(Float64, c["values"]))
    n = rand(rng, [2, 5, 9])
    try
        v = Makie.get_minor_tickvalues(Makie.IntervalsBetween(n), scalefn[c["scale"]], ticks, lo, hi)
        length(v) > 10_000 && continue
        push!(minor, Dict("n" => n, "scale" => c["scale"], "lo" => f(lo), "hi" => f(hi), "ticks" => fs(ticks), "values" => fs(v)))
    catch err
        @warn "skipping log minor" lo hi err
    end
end

# ---------------------------------------------------------------------------------------------
# Julia Base numerics used by the port

ranges = Any[]
nice = [0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 2.5, 5.0, 1 / 3, 0.3, 0.05, 1e-3, 1e5]
for i in 1:400
    s = rand(rng) < 0.7 ? rand(rng, nice) * 10.0^rand(rng, -3:3) : 10.0^(rand(rng) * 8 - 4) * rand(rng)
    s *= randsign()
    a = rand(rng) < 0.6 ? rand(rng, -20:20) * abs(s) : randsign() * 10.0^(rand(rng) * 8 - 4) * rand(rng)
    b = a + s * (rand(rng) * 12 - 1)
    r = collect(a:s:b)
    push!(ranges, Dict("kind" => "colon", "a" => f(a), "s" => f(s), "b" => f(b), "values" => fs(r)))
end
for i in 1:150
    a = randsign() * 10.0^(rand(rng) * 10 - 5) * rand(rng)
    b = rand(rng) < 0.5 ? a + a * 1e-13 * rand(rng) : a + randsign() * 10.0^(rand(rng) * 10 - 5)
    rand(rng) < 0.2 && (a = round(a, digits = 2); b = round(b, digits = 1))
    n = rand(rng, 2:12)
    push!(ranges, Dict("kind" => "linspace", "a" => f(a), "b" => f(b), "n" => n, "values" => fs(collect(range(a, b; length = n)))))
end
push!(ranges, Dict("kind" => "linspace", "a" => f(-1e308), "b" => f(1e308), "n" => 3, "values" => fs(collect(range(-1e308, 1e308; length = 3)))))

math = Any[]
for n in -330:330
    push!(math, Dict("f" => "pow10", "x" => string(n), "y" => f(10.0^n)))
end
for i in 1:600
    x = 10.0^(rand(rng) * 600 - 300) * (1 + rand(rng))
    rand(rng) < 0.3 && (x = 10.0^rand(rng, -20:20) * (1 + randsign() * rand(rng, 0:4) * eps()))
    push!(math, Dict("f" => "log10", "x" => f(x), "y" => f(log10(x))))
    x2 = rand(rng) * 2000
    push!(math, Dict("f" => "log2", "x" => f(x2), "y" => f(log2(x2))))
    push!(math, Dict("f" => "log", "x" => f(x2), "y" => f(log(x2))))
    y = rand(rng) < 0.3 ? Float64(rand(rng, -320:308)) : rand(rng) * 600 - 300
    push!(math, Dict("f" => "exp10", "x" => f(y), "y" => f(exp10(y))))
    z = (rand(rng) * 2 - 1) * 700
    push!(math, Dict("f" => "exp", "x" => f(z), "y" => f(exp(z))))
    push!(math, Dict("f" => "exp2", "x" => f(z), "y" => f(exp2(z))))
end
shortest = Any[]
for x in Float64[Inf, 1e39, 3.4e38, 1e-38, 1e-16 * 1.5, 0.1, 0.3, 1 / 3, 100.0, 1e10, 123456789.0, 16777217.0]
    push!(shortest, Dict("x" => f(x), "e10" => Base.Ryu.reduce_shortest(Float32(x))[2]))
end
for i in 1:800
    x = 10.0^(rand(rng) * 70 - 35) * rand(rng)
    rand(rng) < 0.3 && (x = rand(rng, -1000:1000) * 10.0^rand(rng, -12:12))
    x == 0 && continue
    push!(shortest, Dict("x" => f(x), "e10" => Base.Ryu.reduce_shortest(Float32(x))[2]))
end

sig = Any[]
for i in 1:500
    x = randsign() * 10.0^(rand(rng) * 40 - 20) * rand(rng)
    rand(rng) < 0.3 && (x = rand(rng, -100:100) * 10.0^rand(rng, -5:5) + rand(rng, [0.0, 0.5, 0.05]) * 10.0^rand(rng, -5:5))
    n = rand(rng, 1:17)
    push!(sig, Dict("x" => f(x), "n" => n, "y" => f(round(x, sigdigits = n))))
end

data = Dict(
    "generator" => "tools/gen_tick_fixtures.jl",
    "versions" => Dict("julia" => string(VERSION), "Makie" => string(pkgversion(Makie)), "PlotUtils" => string(pkgversion(PlotUtils))),
    "wilkinson" => wilk, "format" => fmt, "log" => logt, "minor" => minor,
    "ranges" => ranges, "math" => math, "shortest" => shortest, "sigdigits" => sig,
)
mkpath(dirname(OUT))
open(OUT, "w") do io
    JSON.print(io, data)
end
println("wrote $(OUT): wilkinson $(length(wilk)), format $(length(fmt)), log $(length(logt)), minor $(length(minor)), ",
    "ranges $(length(ranges)), math $(length(math)), shortest $(length(shortest)), sigdigits $(length(sig))")
