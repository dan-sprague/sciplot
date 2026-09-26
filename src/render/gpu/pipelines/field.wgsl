// Heatmaps: one quad over the cell bounds; each fragment finds its cell (affine map for regular
// grids, binary search over an edges buffer otherwise) and colormaps the value.

struct FieldU {
    rect_px: vec4<f32>,     // quad in device px: x0, y0, x1, y1
    imap: vec4<f32>,        // regular axes: fractional cell index = frag.xy * imap.xy + imap.zw
    lmap: vec4<f32>,        // irregular axes: local coordinate = frag.xy * lmap.xy + lmap.zw
    dims: vec2<u32>,        // nx, ny cells
    row0: u32,              // first row held in `z` (row bands for fields above the binding limit)
    rows: u32,              // rows held in `z`
    interpolate: u32,
    irregular: u32,         // bit 0: x uses xedges, bit 1: y uses yedges
    xdir: f32,              // sign of xedges[nx] - xedges[0]
    ydir: f32,
    cm: CMap,
};
@group(1) @binding(0) var<uniform> h: FieldU;
@group(1) @binding(1) var<storage, read> z: array<f32>;       // x fastest (Makie z[i, j])
@group(1) @binding(2) var<storage, read> xedges: array<f32>;  // nx + 1 local coordinates
@group(1) @binding(3) var<storage, read> yedges: array<f32>;  // ny + 1 local coordinates
@group(1) @binding(4) var lut: texture_2d<f32>;

@vertex
fn vs_field(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    let t = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    return px_to_clip(mix(h.rect_px.xy, h.rect_px.zw, t));
}

// Storage pointers can't be function parameters without an extension: one search per axis.
fn search_x(l: f32) -> f32 {
    var lo = 0u;
    var hi = h.dims.x;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) >> 1u;
        if ((xedges[mid] - l) * h.xdir <= 0.0) { lo = mid; } else { hi = mid; }
    }
    let w = xedges[lo + 1u] - xedges[lo];
    return f32(lo) + select(0.5, clamp((l - xedges[lo]) / w, 0.0, 1.0), w != 0.0);
}

fn search_y(l: f32) -> f32 {
    var lo = 0u;
    var hi = h.dims.y;
    loop {
        if (hi - lo <= 1u) { break; }
        let mid = (lo + hi) >> 1u;
        if ((yedges[mid] - l) * h.ydir <= 0.0) { lo = mid; } else { hi = mid; }
    }
    let w = yedges[lo + 1u] - yedges[lo];
    return f32(lo) + select(0.5, clamp((l - yedges[lo]) / w, 0.0, 1.0), w != 0.0);
}

// Value of cell (ix, iy), clamped to the grid (GLMakie's clamp-to-edge) and to the rows held.
fn zval(ix: i32, iy: i32) -> f32 {
    let x = u32(clamp(ix, 0, i32(h.dims.x) - 1));
    let y = u32(clamp(iy, max(0, i32(h.row0)), min(i32(h.dims.y), i32(h.row0 + h.rows)) - 1));
    return z[(y - h.row0) * h.dims.x + x];
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
