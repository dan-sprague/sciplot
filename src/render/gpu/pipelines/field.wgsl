// Heatmaps: one quad over the cell bounds (per tile); each fragment finds its cell (affine map for
// regular grids, binary search over an edges texture otherwise) and colormaps the value.
//
// Provenance: defaults and behaviour follow GLMakie 0.13.14 `assets/shader/heatmap.frag` and
// `src/plot-primitives.jl` (heatmap `interpolate`: bilinear between cell centres, clamped at the
// grid edge; values x fastest); the per-fragment cell lookup is our own. MIT licensed; see
// THIRD_PARTY_NOTICES.md.

struct FieldU {
    rect_px: vec4<f32>,     // quad in device px: x0, y0, x1, y1
    imap: vec4<f32>,        // regular axes: fractional cell index = frag.xy * imap.xy + imap.zw
    lmap: vec4<f32>,        // irregular axes: local coordinate = frag.xy * lmap.xy + lmap.zw
    dims: vec2<u32>,        // nx, ny cells
    xdir: f32,              // sign of xedges[nx] - xedges[0]
    ydir: f32,
    tile: vec4<u32>,        // first column, first row, columns, rows held in `z`
    interpolate: u32,
    irregular: u32,         // bit 0: x uses xedges, bit 1: y uses yedges
    edge_w: vec2<u32>,      // row widths of the edge textures
    cm: CMap,
};
@group(1) @binding(0) var<uniform> h: FieldU;
@group(1) @binding(1) var z: texture_2d<f32>;       // R32Float, x fastest (Makie z[i, j])
@group(1) @binding(2) var xedges: texture_2d<f32>;  // nx + 1 local coordinates, rows of edge_w.x
@group(1) @binding(3) var yedges: texture_2d<f32>;  // ny + 1 local coordinates, rows of edge_w.y
@group(1) @binding(4) var lut: texture_2d<f32>;

@vertex
fn vs_field(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    let t = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    return px_to_clip(mix(h.rect_px.xy, h.rect_px.zw, t));
}

fn xedge(i: u32) -> f32 {
    return textureLoad(xedges, vec2<i32>(i32(i % h.edge_w.x), i32(i / h.edge_w.x)), 0).r;
}

fn yedge(i: u32) -> f32 {
    return textureLoad(yedges, vec2<i32>(i32(i % h.edge_w.y), i32(i / h.edge_w.y)), 0).r;
}

// One search per axis (textures as function arguments don't translate to every backend).
fn search_x(l: f32) -> f32 {
    var lo = 0u;
    var hi = h.dims.x;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) >> 1u;
        if ((xedge(mid) - l) * h.xdir <= 0.0) { lo = mid; } else { hi = mid; }
    }
    let e = xedge(lo);
    let w = xedge(lo + 1u) - e;
    return f32(lo) + select(0.5, clamp((l - e) / w, 0.0, 1.0), w != 0.0);
}

fn search_y(l: f32) -> f32 {
    var lo = 0u;
    var hi = h.dims.y;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) >> 1u;
        if ((yedge(mid) - l) * h.ydir <= 0.0) { lo = mid; } else { hi = mid; }
    }
    let e = yedge(lo);
    let w = yedge(lo + 1u) - e;
    return f32(lo) + select(0.5, clamp((l - e) / w, 0.0, 1.0), w != 0.0);
}

// Value of cell (ix, iy), clamped to the grid (GLMakie's clamp-to-edge) and to the cells held.
fn zval(ix: i32, iy: i32) -> f32 {
    let t = vec4<i32>(h.tile);
    let x = clamp(ix, max(0, t.x), min(i32(h.dims.x), t.x + t.z) - 1);
    let y = clamp(iy, max(0, t.y), min(i32(h.dims.y), t.y + t.w) - 1);
    return textureLoad(z, vec2<i32>(x - t.x, y - t.y), 0).r;
}

@fragment
fn fs_field(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var f = frag.xy * h.imap.xy + h.imap.zw;
    if ((h.irregular & 1u) != 0u) { f.x = search_x(frag.x * h.lmap.x + h.lmap.z); }
    if ((h.irregular & 2u) != 0u) { f.y = search_y(frag.y * h.lmap.y + h.lmap.w); }
    // Pixels at the quad's antialiased border may sit just outside the grid: clamp (also keeps
    // the float -> int conversion in range).
    let n = vec2<f32>(h.dims);
    f = clamp(f, vec2<f32>(0.0), n - vec2<f32>(0.001));
    let own = zval(i32(f.x), i32(f.y));
    var v = own;
    if (h.interpolate != 0u && !nan_bits(own)) {
        // Bilinear between cell centres; NaN neighbours drop out and the weights renormalize.
        let gg = f - vec2<f32>(0.5);
        let b = floor(gg);
        let w = gg - b;
        let i = vec2<i32>(b);
        var acc = 0.0;
        var ws = 0.0;
        for (var k = 0; k < 4; k = k + 1) {
            let dx = k & 1;
            let dy = k >> 1u;
            let zv = zval(i.x + dx, i.y + dy);
            let wk = select(1.0 - w.x, w.x, dx == 1) * select(1.0 - w.y, w.y, dy == 1);
            if (!nan_bits(zv)) {
                acc = acc + wk * zv;
                ws = ws + wk;
            }
        }
        v = acc / ws;
    }
    return premul(cmap_lookup(v, h.cm, lut));
}
