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
@group(1) @binding(1) var lut: texture_2d<f32>;

// Per-instance streams (unused ones hold arbitrary data).
struct MarkerIn {
    @location(0) pos: vec2<f32>,
    @location(1) col: u32,     // premultiplied RGBA8 (col_mode 1) or f32 bits of the value (2)
    @location(2) size: f32,    // units (size_stride 1)
};

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
fn vs_marker(@builtin(vertex_index) vid: u32, v: MarkerIn) -> MarkerV {
    var o: MarkerV;
    let p = v.pos;
    if (!(finite_bits(p.x) && finite_bits(p.y))) {
        o.pos = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return o;
    }
    var size = m.size;
    if (m.size_stride != 0u) { size = v.size; }
    size = size * g.ppu;
    var c = m.color;
    if (m.col_mode == 1u) {
        c = unpack4x8unorm(v.col);
    } else if (m.col_mode == 2u) {
        c = premul(cmap_lookup(bitcast<f32>(v.col), m.cm, lut));
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

// Number of outline vertices of a polygon marker (0 for the circles).
fn marker_nverts(shape: u32) -> u32 {
    switch shape {
        case 0u, 12u: { return 0u; }
        case 3u, 4u: { return 12u; }
        case 5u, 6u, 7u, 8u: { return 3u; }
        case 9u: { return 5u; }
        case 10u: { return 6u; }
        case 11u: { return 10u; }
        default: { return 4u; }            // Rect, Diamond, FullRect
    }
}

fn ngon_vertex(i: u32, n: u32, r: f32) -> vec2<f32> {
    let a = 6.28318530718 * f32(i) / f32(n);
    return vec2<f32>(sin(a), cos(a)) * r;
}

// Vertex `i` (mod the vertex count) of a polygon marker's outline in units of markersize, y up:
// the shapes of `marker_sdf` (and of `render/svg/marker.rs`).
fn marker_vertex(shape: u32, i: u32) -> vec2<f32> {
    let k = i % marker_nverts(shape);
    switch shape {
        case 2u: { return ngon_vertex(k, 4u, 0.446496); }     // Diamond: the Rect turned 45 degrees
        case 3u, 4u: {
            // One arm per three vertices, turned clockwise by 90 degrees per arm.
            let t = k % 3u;
            var v = vec2<f32>(0.1245, 0.375);
            if (t == 1u) { v = vec2<f32>(0.1245); } else if (t == 2u) { v = vec2<f32>(0.375, 0.1245); }
            v = rot(v, 1.57079632679 * f32(k / 3u));
            if (shape == 4u) { v = rot(v, -0.785398163); }
            return v;
        }
        case 5u, 6u, 7u, 8u: {
            var tri = array<vec2<f32>, 3>(
                vec2<f32>(0.0, 0.485), vec2<f32>(-0.36375, -0.2425), vec2<f32>(0.36375, -0.2425));
            // D/L/R triangles are the U triangle turned by 180, 90 (ccw) and 90 (cw) degrees.
            var a = 0.0;
            if (shape == 6u) { a = 3.14159265359; } else if (shape == 7u) { a = -1.57079632679; } else if (shape == 8u) { a = 1.57079632679; }
            return rot(tri[k], a);
        }
        case 9u: { return ngon_vertex(k, 5u, 0.375); }
        case 10u: { return ngon_vertex(k, 6u, 0.375); }
        case 11u: { return ngon_vertex(k, 10u, select(0.45, 0.21, (k & 1u) == 1u)); }
        default: {
            // Rect and FullRect: corners counter-clockwise from the bottom left.
            let h = select(0.5, 0.315718, shape == 1u);
            return h * vec2<f32>(select(-1.0, 1.0, k == 1u || k == 2u), select(-1.0, 1.0, k >= 2u));
        }
    }
}

const AA: f32 = 0.70710678;   // px, half-width of the AA ramp

// Coverage of the region `s <= 0` for a signed distance `s` in device px.
fn ramp(s: f32) -> f32 {
    return 1.0 - smoothstep(-AA, AA, s);
}

// Cairo's bevel: a joint turning by more than 120 degrees (miter length over line width above
// CairoMakie's miter limit 2) is cut perpendicular to its bisector, through the ends of the two
// offset edges. `q` is relative to the joint; `d_in`, `d_out` are the edge directions.
fn bevel(q: vec2<f32>, d_in: vec2<f32>, d_out: vec2<f32>, h: f32) -> f32 {
    let c = dot(d_in, d_out);
    if (c >= -0.5) { return 1.0; }
    return ramp(dot(q, normalize(d_in - d_out)) - h * sqrt(0.5 * (1.0 + c)));
}

// Coverage of CairoMakie's marker stroke: the outline stroked centered with half width `h` (px),
// miter joins, miter limit 2. The stroke is tiled by one piece per edge (the band along the edge,
// cut at both joints along their bisectors), so the pieces' coverages add up without seams or
// double blending, as Cairo fills the whole stroke at once. `p` and `size` in device px.
fn poly_stroke(shape: u32, p: vec2<f32>, size: f32, h: f32) -> f32 {
    let n = marker_nverts(shape);
    var prev = marker_vertex(shape, n - 1u) * size;
    var a = marker_vertex(shape, 0u) * size;
    var b = marker_vertex(shape, 1u) * size;
    var cover = 0.0;
    for (var i = 0u; i < n; i = i + 1u) {
        let next = marker_vertex(shape, i + 2u) * size;
        let d0 = normalize(a - prev);
        let d = normalize(b - a);
        let d2 = normalize(next - b);
        let t = dot(p - a, vec2<f32>(-d.y, d.x));
        var c = ramp(t - h) - ramp(t + h);
        c = c * ramp(-dot(p - a, normalize(d0 + d))) * ramp(dot(p - b, normalize(d + d2)));
        c = c * bevel(p - a, d0, d, h) * bevel(p - b, d, d2, h);
        cover = cover + c;
        prev = a;
        a = b;
        b = next;
    }
    return min(cover, 1.0);
}

@fragment
fn fs_marker(in: MarkerV) -> @location(0) vec4<f32> {
    // The shape turns counter-clockwise by `rotation` (Makie, CairoMakie, the SVG backend): look
    // up the unrotated SDF at the point turned back clockwise.
    let q = rot(in.q, m.rotation);
    let d = marker_sdf(m.shape, q / in.size) * in.size;   // device px
    var out = in.fill * ramp(d);
    // CairoMakie: fill, then the stroke centered on the outline, painted over it.
    let h = 0.5 * m.stroke * g.ppu;
    if (h > 0.0) {
        var cs: f32;
        if (marker_nverts(m.shape) == 0u) {
            cs = ramp(d - h) - ramp(d + h);
        } else {
            cs = poly_stroke(m.shape, q, in.size, h);
        }
        out = m.stroke_color * cs + out * (1.0 - m.stroke_color.a * cs);
    }
    if (out.a <= 0.0) { discard; }
    return out;
}
