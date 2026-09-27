// 3D polylines: one instanced 4-vertex quad per segment, extruded in screen space around the
// projected end points; the fragment shader draws a capsule (round caps, so consecutive segments
// join round) with the depth interpolated between the ends. Depth-tested, no depth writes.

const AA: f32 = 0.8;   // px, half-width of the AA smoothstep (GLMakie)

struct Lines3dU {
    v: View3,
    color: vec4<f32>,  // premultiplied (color_mode 0)
    width: f32,        // device px
    color_mode: u32,   // 0 uniform, 1 per-point premultiplied RGBA8, 2 values -> colormap
    n: u32,            // number of points
    _p: u32,
    cm: CMap,
};
@group(1) @binding(0) var<uniform> u: Lines3dU;
@group(1) @binding(1) var lut: texture_2d<f32>;

struct SegIn {
    @location(0) a: vec3<f32>,   // point k
    @location(1) b: vec3<f32>,   // point k + 1
    @location(2) ca: u32,        // their colors (RGBA8) or value bits
    @location(3) cb: u32,
};

struct SegV {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,                          // px from the first end
    @location(1) local: vec3<f32>,
    @location(2) @interpolate(flat) d: vec2<f32>,       // second end - first end (px)
    @location(3) @interpolate(flat) c1: vec4<f32>,
    @location(4) @interpolate(flat) c2: vec4<f32>,
    @location(5) @interpolate(flat) hw_fade: vec2<f32>, // half width (px), alpha for thin lines
};

fn seg_color(c: u32) -> vec4<f32> {
    if (u.color_mode == 1u) { return unpack4x8unorm(c); }
    if (u.color_mode == 2u) { return vec4<f32>(bitcast<f32>(c), 0.0, 0.0, 0.0); }
    return u.color;
}

@vertex
fn vs_line3d(@builtin(vertex_index) vid: u32, @builtin(instance_index) k: u32, v: SegIn) -> SegV {
    var o: SegV;   // zero: a degenerate quad
    if (k + 1u >= u.n || !finite3(v.a) || !finite3(v.b)) { return o; }
    let ca = u.v.m * vec4<f32>(v.a, 1.0);
    let cb = u.v.m * vec4<f32>(v.b, 1.0);
    if (ca.w <= 0.0 || cb.w <= 0.0) { return o; }
    let pa = clip_to_px(ca);
    let pb = clip_to_px(cb);
    let d = pb - pa;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-4) { dir = d / len; }
    let nrm = vec2<f32>(-dir.y, dir.x);
    let hw = 0.5 * max(u.width, AA);
    let r = hw + AA + 0.5;
    let second = (vid & 2u) != 0u;
    let sx = select(-1.0, 1.0, second);
    let sy = select(-1.0, 1.0, (vid & 1u) != 0u);
    let p = select(pa, pb, second) + sx * r * dir + sy * r * nrm;
    o.pos = px_at_depth(p, select(ca, cb, second));
    o.q = p - pa;
    o.local = select(v.a, v.b, second);
    o.d = d;
    o.c1 = seg_color(v.ca);
    o.c2 = seg_color(v.cb);
    o.hw_fade = vec2<f32>(hw, min(1.0, u.width / AA));
    return o;
}

@fragment
fn fs_line3d(in: SegV) -> @location(0) vec4<f32> {
    if (!in_box(u.v, in.local)) { discard; }
    let dd = dot(in.d, in.d);
    var t = 0.0;
    if (dd > 1e-8) { t = clamp(dot(in.q, in.d) / dd, 0.0, 1.0); }
    let dist = length(in.q - t * in.d);
    let a = smoothstep(-AA, AA, in.hw_fade.x - dist) * in.hw_fade.y;
    var c = mix(in.c1, in.c2, t);
    if (u.color_mode == 2u) { c = premul(cmap_lookup(c.x, u.cm, lut)); }
    let out = c * a;
    if (out.a <= 0.0) { discard; }
    return out;
}
