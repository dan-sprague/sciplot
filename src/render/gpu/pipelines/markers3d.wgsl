// Screen-space markers at 3D positions (Axis3 scatter): instanced quads with the depth of the
// marker centre, depth-tested (no depth writes). The marker SDFs mirror sprite.wgsl.

struct Markers3dU {
    v: View3,
    color: vec4<f32>,          // premultiplied, used when col_mode == 0
    stroke_color: vec4<f32>,   // premultiplied
    size: f32,                 // device px
    stroke: f32,               // device px
    shape: u32,
    col_mode: u32,             // bits 0-3: 0 uniform, 1 per-element RGBA8, 2 values; bit 4: per-element sizes
    cm: CMap,
};
@group(1) @binding(0) var<uniform> m: Markers3dU;
@group(1) @binding(1) var lut: texture_2d<f32>;

struct Marker3In {
    @location(0) pos: vec3<f32>,
    @location(1) col: u32,     // premultiplied RGBA8 (mode 1) or f32 bits of the value (mode 2)
    @location(2) size: f32,    // units (per-element sizes)
};

struct Marker3V {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,                      // device px from the center, y up
    @location(1) @interpolate(flat) fill: vec4<f32>,
    @location(2) @interpolate(flat) size: f32,      // device px
};

fn shape_radius(shape: u32) -> f32 {
    switch shape {
        case 0u: { return 0.3525; }
        case 1u, 2u: { return 0.4465; }
        case 3u, 4u: { return 0.3951; }
        case 5u, 6u, 7u, 8u: { return 0.485; }
        case 9u, 10u: { return 0.375; }
        case 11u: { return 0.45; }
        case 12u: { return 0.5; }
        default: { return 0.7072; }
    }
}

@vertex
fn vs_marker3d(@builtin(vertex_index) vid: u32, v: Marker3In) -> Marker3V {
    var o: Marker3V;
    if (!finite3(v.pos) || !in_box(m.v, v.pos)) { return o; }
    let c = m.v.m * vec4<f32>(v.pos, 1.0);
    if (c.w <= 0.0) { return o; }
    var size = m.size;
    if ((m.col_mode & 16u) != 0u) { size = v.size * g.ppu; }
    let mode = m.col_mode & 15u;
    var fill = m.color;
    if (mode == 1u) {
        fill = unpack4x8unorm(v.col);
    } else if (mode == 2u) {
        fill = premul(cmap_lookup(bitcast<f32>(v.col), m.cm, lut));
    }
    let half = shape_radius(m.shape) * size + m.stroke + 1.0;
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u)) * 2.0 - 1.0;
    o.pos = px_at_depth(clip_to_px(c) + corner * half, c);
    o.q = vec2<f32>(corner.x, -corner.y) * half;
    o.fill = fill;
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
fn fs_marker3d(in: Marker3V) -> @location(0) vec4<f32> {
    let d = marker_sdf(m.shape, in.q / in.size) * in.size;   // device px
    let aa = 0.70710678;
    let sw = m.stroke;
    // CairoMakie strokes: centered on the outline.
    let cover = 1.0 - smoothstep(0.5 * sw - aa, 0.5 * sw + aa, d);
    let k = select(0.0, smoothstep(-0.5 * sw - aa, -0.5 * sw + aa, d), sw > 0.0);
    let out = mix(in.fill, m.stroke_color, k) * cover;
    if (out.a <= 0.0) { discard; }
    return out;
}
