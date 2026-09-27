# Axis3 reference fixtures for sciplot's port of Makie's Axis3 camera and decorations
# (src/scene/axis3, tests/axis3.rs).
#
#     julia --project=tools tools/gen_axis3_fixtures.jl
#
# For each case: an Axis3 with fixed limits in a figure of the given size, after
# `update_state_before_display!`. Dumps the layout bbox and scene viewport (Makie y-up figure units,
# `[x, y, w, h]`), the camera matrices (`model`, `view`, `projection`, row-major), the eye position,
# the 8 limit corners projected to figure pixels (y up), and every decoration the blockscene draws:
# grid and frame line endpoints (3D), tick segments, tick label positions / texts / alignment,
# axis label positions / rotations / alignment and the title position. Writes
# tests/fixtures/axis3.json.

using CairoMakie
using JSON

rect(r) = [Float64.(minimum(r))..., Float64.(widths(r))...]
rowmajor(m) = [[Float64(m[i, j]) for j in 1:4] for i in 1:4]
pts(v) = [Float64.(collect(p)) for p in v]
angle(q) = 2 * atan(Float64(q[3]), Float64(q[4]))   # quaternion (x, y, z, w) about z

const CASES = [
    Dict("name" => "default", "size" => (600, 450), "kw" => Dict{Symbol,Any}()),
    Dict("name" => "rotated", "size" => (600, 450),
        "kw" => Dict{Symbol,Any}(:azimuth => 0.3pi, :elevation => 0.6, :perspectiveness => 0.5)),
    Dict("name" => "data_fit", "size" => (500, 500),
        "kw" => Dict{Symbol,Any}(:aspect => :data, :viewmode => :fit, :azimuth => 0.8pi, :elevation => 0.2pi)),
    Dict("name" => "equal_stretch", "size" => (700, 400),
        "kw" => Dict{Symbol,Any}(:aspect => :equal, :viewmode => :stretch, :protrusions => (10, 20, 30, 40))),
    Dict("name" => "reversed_below", "size" => (600, 450),
        "kw" => Dict{Symbol,Any}(:xreversed => true, :elevation => -0.3, :azimuth => 1.75pi, :aspect => (1, 2, 1))),
    Dict("name" => "tall", "size" => (300, 600),
        "kw" => Dict{Symbol,Any}(:azimuth => 0.55pi, :elevation => 0.1pi, :perspectiveness => 1.0,
            :title => "Tall", :xlabel => "time", :zlabel => "height")),
]

const LIMITS = (-1.0, 2.0, 0.0, 10.0, -3.0, 5.0)

function dumpcase(c)
    w, h = c["size"]
    fig = Figure(size = (w, h))
    ax = Axis3(fig[1, 1]; limits = LIMITS, c["kw"]...)
    Makie.update_state_before_display!(fig)
    cam = ax.scene.camera
    bs = ax.blockscene
    lims = ax.finallimits[]
    mi, ma = minimum(lims), maximum(lims)
    corners = [Point3d(x, y, z) for x in (mi[1], ma[1]) for y in (mi[2], ma[2]) for z in (mi[3], ma[3])]
    ls3 = [p for p in bs.plots if p isa LineSegments && eltype(p[1][]) <: Point{3}]
    ls2 = [p for p in bs.plots if p isa LineSegments && eltype(p[1][]) <: Point{2}]
    texts = [p for p in bs.plots if p isa Makie.Text]
    dims = map(1:3) do d
        ticks = ls2[d]
        labels = texts[2d - 1]
        label = texts[2d]
        Dict(
            "grid1" => pts(ls3[3d - 2][1][]),
            "grid2" => pts(ls3[3d - 1][1][]),
            "frame" => pts(ls3[3d][1][]),
            "ticks" => pts(ticks[1][]),
            "ticklabel_pos" => pts(labels[1][]),
            "ticklabel_text" => [string(t) for t in labels.text[]],
            "ticklabel_align" => [string.(labels.align[])...],
            "label_pos" => Float64.(collect(label[1][][1])),
            "label_rot" => angle(label.rotation[]),
            "label_align" => [string.(label.align[])...],
        )
    end
    Dict(
        "name" => c["name"],
        "size" => [w, h],
        "attrs" => Dict(string(k) => (v isa Symbol ? string(v) : v) for (k, v) in c["kw"]),
        "limits" => collect(LIMITS),
        "bbox" => rect(ax.layoutobservables.computedbbox[]),
        "viewport" => rect(ax.scene.viewport[]),
        "model" => rowmajor(ax.scene.transformation.model[]),
        "view" => rowmajor(cam.view[]),
        "projection" => rowmajor(cam.projection[]),
        "eyeposition" => Float64.(collect(cam.eyeposition[])),
        "corners" => pts(corners),
        "corners_px" => [Float64.(collect(Makie.project(bs, p))) for p in corners],
        "dims" => dims,
        "title_pos" => Float64.(collect(texts[end][1][][1])),
    )
end

out = [dumpcase(c) for c in CASES]
path = normpath(joinpath(@__DIR__, "..", "tests", "fixtures", "axis3.json"))
open(path, "w") do io
    JSON.print(io, out, 1)
end
println("wrote ", path)
