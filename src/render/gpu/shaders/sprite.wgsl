// Scatter markers as instanced quads with analytic SDFs (Makie marker geometry, GLMakie AA and
// outer stroke).

struct SpriteU {
    xform: vec4<f32>,
    color: vec4<f32>,          // premultiplied, used when col_mode == 0
    stroke_color: vec4<f32>,   // premultiplied
    size: f32,                 // units
    stroke: f32,               // units
    shape: u32,
    col_mode: u32,             // 0 uniform, 1 per-element RGBA8 (premultiplied), 2 values -> colormap
    size_stride: u32,          // 0: uniform size, 1: per-element sizes
    rotation: f32,
    _p0: u32,
    _p1: u32,
    cm: CMap,
};
@group(1) @binding(0) var<uniform> m: SpriteU;
@group(1) @binding(1) var<storage, read> mpos: array<vec2<f32>>;
@group(1) @binding(2) var<storage, read> mcol: array<u32>;
@group(1) @binding(3) var<storage, read> mval: array<f32>;
@group(1) @binding(4) var<storage, read> msize: array<f32>;
@group(1) @binding(5) var lut: texture_2d<f32>;

struct MarkerV {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,                      // device px from the center, y up, unrotated
    @location(1) @interpolate(flat) fill: vec4<f32>,
    @location(2) @interpolate(flat) size: f32,      // device px
};

// Bounding radius of each shape in units of markersize (rotation-safe quad size).
fn shape_radius(shape: u32) -> f32 {
    switch shape {
        case 0u: { return 0.3525; }        // Circle
        case 1u, 2u: { return 0.4465; }    // Rect, Diamond
        case 3u, 4u: { return 0.3951; }    // Cross, XCross
        case 5u, 6u, 7u, 8u: { return 0.485; }
        case 9u, 10u: { return 0.375; }    // Pentagon, Hexagon
        case 11u: { return 0.45; }         // Star5
        case 12u: { return 0.5; }          // FullCircle
        default: { return 0.7072; }        // FullRect
    }
}

@vertex
fn vs_marker(@builtin(vertex_index) vid: u32, @builtin(instance_index) i: u32) -> MarkerV {
    var o: MarkerV;
    let p = mpos[i];
    if (!(finite_bits(p.x) && finite_bits(p.y))) {
        o.pos = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return o;
    }
    var size = m.size;
    if (m.size_stride != 0u) { size = msize[i]; }
    size = size * g.ppu;
    var c = m.color;
    if (m.col_mode == 1u) {
        c = unpack4x8unorm(mcol[i]);
    } else if (m.col_mode == 2u) {
        c = premul(cmap_lookup(mval[i], m.cm, lut));
    }
    let half = shape_radius(m.shape) * size + m.stroke * g.ppu + 1.0;
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u)) * 2.0 - 1.0;
    let center = p * m.xform.xy + m.xform.zw;
    o.pos = px_to_clip(center + corner * half);
    o.q = vec2<f32>(corner.x, -corner.y) * half;
    o.fill = c;
    o.size = size;
    return o;
}

fn sd_box(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn rot(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x + s * p.y, -s * p.x + c * p.y);
}

// iq's exact polygon SDF for a regular polygon/star with n vertices (<= 10) starting at the top.
fn sd_ngon(p: vec2<f32>, n: u32, r_out: f32, r_in: f32, star: bool) -> f32 {
    var verts: array<vec2<f32>, 10>;
    for (var i = 0u; i < n; i = i + 1u) {
        let a = 6.28318530718 * f32(i) / f32(n);
        var r = r_out;
        if (star && (i % 2u) == 1u) { r = r_in; }
        verts[i] = vec2<f32>(sin(a), cos(a)) * r;
    }
    var d = dot(p - verts[0], p - verts[0]);
    var s = 1.0;
    var j = n - 1u;
    for (var i = 0u; i < n; i = i + 1u) {
        let vi = verts[i];
        let vj = verts[j];
        let e = vj - vi;
        let w = p - vi;
        let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c1 = p.y >= vi.y;
        let c2 = p.y < vj.y;
        let c3 = e.x * w.y > e.y * w.x;
        if ((c1 && c2 && c3) || (!c1 && !c2 && !c3)) { s = -s; }
        j = i;
    }
    return s * sqrt(d);
}

fn sd_tri(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a; let e1 = c - b; let e2 = a - c;
    let v0 = p - a; let v1 = p - b; let v2 = p - c;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(min(vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
                    vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
                    vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)));
    return -sqrt(d.x) * sign(d.y);
}

// q in units of markersize, y up; negative inside.
fn marker_sdf(shape: u32, q: vec2<f32>) -> f32 {
    let r45 = rot(q, 0.785398163);
    switch shape {
        case 0u: { return length(q) - 0.3525; }
        case 1u: { return sd_box(q, vec2<f32>(0.315718)); }
        case 2u: { return sd_box(r45, vec2<f32>(0.315718)); }
        case 3u: { return min(sd_box(q, vec2<f32>(0.375, 0.1245)), sd_box(q, vec2<f32>(0.1245, 0.375))); }
        case 4u: { return min(sd_box(r45, vec2<f32>(0.375, 0.1245)), sd_box(r45, vec2<f32>(0.1245, 0.375))); }
        case 5u: { return sd_tri(q, vec2<f32>(0.0, 0.485), vec2<f32>(-0.36375, -0.2425), vec2<f32>(0.36375, -0.2425)); }
        case 6u: { return sd_tri(q, vec2<f32>(0.0, -0.485), vec2<f32>(0.36375, 0.2425), vec2<f32>(-0.36375, 0.2425)); }
        case 7u: { return sd_tri(q, vec2<f32>(-0.485, 0.0), vec2<f32>(0.2425, -0.36375), vec2<f32>(0.2425, 0.36375)); }
        case 8u: { return sd_tri(q, vec2<f32>(0.485, 0.0), vec2<f32>(-0.2425, 0.36375), vec2<f32>(-0.2425, -0.36375)); }
        case 9u: { return sd_ngon(q, 5u, 0.375, 0.375, false); }
        case 10u: { return sd_ngon(q, 6u, 0.375, 0.375, false); }
        case 11u: { return sd_ngon(q, 10u, 0.45, 0.21, true); }
        case 12u: { return length(q) - 0.5; }
        default: { return sd_box(q, vec2<f32>(0.5)); }
    }
}

@fragment
fn fs_marker(in: MarkerV) -> @location(0) vec4<f32> {
    let q = rot(in.q, -m.rotation);
    let d = marker_sdf(m.shape, q / in.size) * in.size;   // device px
    let aa = 0.70710678;
    let sw = m.stroke * g.ppu;
    let cover = 1.0 - smoothstep(sw - aa, sw + aa, d);    // fill plus outer stroke
    let k = select(0.0, smoothstep(-aa, aa, d), sw > 0.0); // fill -> stroke transition
    let out = mix(in.fill, m.stroke_color, k) * cover;
    if (out.a <= 0.0) { discard; }
    return out;
}
