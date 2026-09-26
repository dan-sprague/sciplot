// Lines and line segments: a port of GLMakie's lines.geom / line_segment.geom and lines.frag to
// instanced 4-vertex triangle strips (one instance per segment). Each instance gets the points
// around its segment (GLMakie's lines_adjacency p0..p3) and its end colors as instance-step
// vertex attributes.
//
// Every vertex recomputes the segment's geometry from its neighbours. Joint data (miter normal,
// truncation, discard line) is derived from segment directions computed by the same expression in
// both segments of a joint, so both agree bit for bit: miter joints meet on the shared miter line
// and truncated joints split their overlap per pixel (no double blending of translucent lines).

const AA_RADIUS: f32 = 0.8;       // px, half-width of the AA smoothstep (GLMakie)
const BUTT: u32 = 0u;
const SQUARE: u32 = 1u;
const ROUND: u32 = 2u;
const BEVEL: u32 = 3u;            // joinstyle: 0 miter, 2 round, 3 bevel
const GUARD: f32 = 8192.0;        // px beyond the target where segments are clipped
const MIN_LEN: f32 = 1e-3;        // px; shorter segments are skipped (exact duplicate points are
                                  // bridged by their neighbours, see line.rs)

struct LineU {
    xform: vec4<f32>,             // device px = local * xy + zw
    color: vec4<f32>,             // premultiplied (color_mode 0)
    width: f32,                   // linewidth in device px
    miter_limit: f32,             // cos(pi - miter_limit)
    joinstyle: u32,
    linecap: u32,                 // 0 butt, 1 square, 2 round
    color_mode: u32,              // 0 uniform, 1 per-point premultiplied RGBA8, 2 values -> colormap
    segments: u32,                // 0 polyline with NaN breaks, 1 independent pairs
    n: u32,                       // number of points
    n_breaks: u32,                // dash pattern boundaries (< 2 = solid)
    pattern_len: f32,             // dash period in linewidths
    closed: u32,                  // 1: the last point repeats the first; join the ends
    _p1: u32,
    _p2: u32,
    breaks: array<vec4<f32>, 4>,  // cumulative dash boundaries in linewidths
    cm: CMap,
};
@group(1) @binding(0) var<uniform> u: LineU;
@group(1) @binding(1) var lut: texture_2d<f32>;

// Per segment k (points k and k + 1). Streams a line doesn't use hold arbitrary data.
struct LineIn {
    @location(0) q0: vec2<f32>,   // the point before p1 (past exact duplicates; across the seam of closed loops)
    @location(1) a: vec2<f32>,    // p1 = point k
    @location(2) b: vec2<f32>,    // p2 = point k + 1
    @location(3) q3: vec2<f32>,   // the point after p2
    @location(4) col1: u32,       // premultiplied RGBA8 (color_mode 1) or f32 bits of the value (2)
    @location(5) col2: u32,
    @location(6) cum1: f32,       // arc length (px) at p1 mod the dash period
};

struct LineV {
    @builtin(position) pos: vec4<f32>,
    // Distance behind p1, beyond p2 (along v1) and across the line, in px.
    @location(0) quad_sdf: vec3<f32>,
    // Flat-cut SDFs of truncated joints at p1 / p2.
    @location(1) trunc: vec2<f32>,
    // Color interpolation: start offset and length along v1.
    @location(2) start_len: vec2<f32>,
    // Extrusion at p1 / p2, half width, alpha weight for sub-AA widths.
    @location(3) @interpolate(flat) ext_w: vec4<f32>,
    // Joint points and miter vectors for the per-pixel discard split (flat: identical in both
    // segments of a joint).
    @location(4) @interpolate(flat) linepts: vec4<f32>,
    @location(5) @interpolate(flat) miter_vecs: vec4<f32>,
    @location(6) @interpolate(flat) c1: vec4<f32>,
    @location(7) @interpolate(flat) c2: vec4<f32>,
    @location(8) @interpolate(flat) capmode: vec2<u32>,
    @location(9) @interpolate(flat) cum0: f32,
    // Directions and lengths of the previous / next segment (body split at truncated joints).
    @location(10) @interpolate(flat) nbr_dir: vec4<f32>,
    @location(11) @interpolate(flat) nbr_len: vec2<f32>,
};

fn perp(v: vec2<f32>) -> vec2<f32> { return vec2<f32>(-v.y, v.x); }
fn sign_nz(x: f32) -> f32 { return select(-1.0, 1.0, x >= 0.0); }
fn finite2(p: vec2<f32>) -> bool { return finite_bits(p.x) && finite_bits(p.y); }
fn to_px(q: vec2<f32>) -> vec2<f32> { return q * u.xform.xy + u.xform.zw; }

// Premultiplied color (or `(value, 0, 0, 0)` for colormapped lines) of a point's stream entry.
fn vcolor(c: u32) -> vec4<f32> {
    if (u.color_mode == 1u) { return unpack4x8unorm(c); }
    if (u.color_mode == 2u) { return vec4<f32>(bitcast<f32>(c), 0.0, 0.0, 0.0); }
    return u.color;
}

// Liang-Barsky on one axis: narrows the parameter range t = (t0, t1); t0 > t1 rejects.
fn clip_axis(t: vec2<f32>, s: f32, d: f32, lo: f32, hi: f32) -> vec2<f32> {
    if (d == 0.0) { return select(t, vec2<f32>(1.0, 0.0), s < lo || s > hi); }
    let ta = (lo - s) / d;
    let tb = (hi - s) / d;
    return vec2<f32>(max(t.x, min(ta, tb)), min(t.y, max(ta, tb)));
}

@vertex
fn vs_line(@builtin(vertex_index) vid: u32, @builtin(instance_index) seg: u32, v: LineIn) -> LineV {
    var o: LineV;   // zero-initialised: an early return gives a degenerate quad
    let strip = u.segments == 0u;
    // Segments mode draws the pairs (k, k + 1) of even k.
    if (!strip && (seg & 1u) == 1u) { return o; }
    let i1 = seg;
    let i2 = i1 + 1u;
    if (i2 >= u.n) { return o; }
    let a = v.a;
    let b = v.b;
    if (!(finite2(a) && finite2(b))) { return o; }
    var p1 = to_px(a);
    var p2 = to_px(b);
    let d12 = p2 - p1;
    let len12 = length(d12);
    if (!(finite2(p1) && finite2(p2)) || len12 < MIN_LEN) { return o; }
    let v1 = normalize(d12);

    // Directions of the neighbouring drawn segments, computed exactly as those segments compute
    // their own v1 (so both sides of a joint agree bit for bit).
    var ok0 = false;
    var ok3 = false;
    var v0 = v1;
    var v2 = v1;
    var len0 = 0.0;
    var len2 = 0.0;
    if (strip) {
        let closed = u.closed == 1u;
        if ((i1 > 0u || closed) && finite2(v.q0)) {
            let d = p1 - to_px(v.q0);
            if (length(d) >= MIN_LEN) {
                v0 = normalize(d);
                len0 = length(d);
                ok0 = finite2(v0);
            }
        }
        if ((i2 + 1u < u.n || closed) && finite2(v.q3)) {
            let d = to_px(v.q3) - p2;
            if (length(d) >= MIN_LEN) {
                v2 = normalize(d);
                len2 = length(d);
                ok3 = finite2(v2);
            }
        }
    }

    var c1 = vcolor(v.col1);
    var c2 = vcolor(v.col2);
    var cum0 = 0.0;
    if (strip && u.n_breaks > 1u) { cum0 = v.cum1; }

    // Guard band: clip far off-screen ends so vertices and interpolated SDFs keep precision.
    // A clipped end becomes a (far away, invisible) cap.
    var t = vec2<f32>(0.0, 1.0);
    t = clip_axis(t, p1.x, d12.x, -GUARD, g.target_px.x + GUARD);
    t = clip_axis(t, p1.y, d12.y, -GUARD, g.target_px.y + GUARD);
    if (t.x > t.y) { return o; }
    if (t.x > 0.0 || t.y < 1.0) {
        let k1 = mix(c1, c2, t.x);
        let k2 = mix(c1, c2, t.y);
        c1 = k1;
        c2 = k2;
        p2 = p1 + t.y * d12;
        p1 = p1 + t.x * d12;
        if (t.x > 0.0) {
            ok0 = false;
            // Keep the dash phase small (f32); it is only approximate this far off-screen.
            let period = u.pattern_len * max(AA_RADIUS, u.width);
            let skipped = t.x * len12;
            cum0 = cum0 + skipped - floor(skipped / period) * period;
        }
        if (t.y < 1.0) { ok3 = false; }
    }
    if (!ok0) { v0 = v1; }
    if (!ok3) { v2 = v1; }

    let seg_len = max(length(p2 - p1), MIN_LEN);
    // Lines thinner than the AA radius keep that width and fade instead (no flicker).
    let hw = 0.5 * max(AA_RADIUS, u.width);
    let pad = select(2.0 * AA_RADIUS, 4.0 * AA_RADIUS, strip);
    let w = hw + pad;
    let n0 = perp(v0);
    let n1 = perp(v1);
    let n2 = perp(v2);

    // Miter normals; n0 + n1 vanishes for 180 degree turns, so sharp turns use v0 - v1.
    let cosang = vec2<f32>(dot(v0, v1), dot(v1, v2));
    var mn1: vec2<f32>;
    if (cosang.x < 0.0) { mn1 = sign_nz(dot(v0, n1)) * normalize(v0 - v1); } else { mn1 = normalize(n0 + n1); }
    var mn2: vec2<f32>;
    if (cosang.y < 0.0) { mn2 = sign_nz(dot(v1, n2)) * normalize(v1 - v2); } else { mn2 = normalize(n1 + n2); }

    // Truncated (beveled) joints: beyond the miter limit, or any real bend for bevel joins.
    let lim = select(u.miter_limit, 0.99, u.joinstyle == BEVEL);
    let tr0 = ok0 && cosang.x < lim;
    let tr1 = ok3 && cosang.y < lim;
    let mv1 = -perp(mn1);
    let mv2 = -perp(mn2);
    let mo1 = dot(mn1, n1);
    let mo2 = dot(mn2, n1);

    // Extension along v1 (in half widths) at p1 (e0) and p2 (e1); .x = -n side, .y = +n side.
    var e0: vec2<f32>;
    var e1: vec2<f32>;
    if (tr0) {
        e0 = vec2<f32>(-abs(mo1 / dot(mv1, n1)));
    } else {
        let s = dot(mn1, v1) / mo1;
        e0 = vec2<f32>(-s, s);
    }
    if (tr1) {
        e1 = vec2<f32>(abs(mo2 / dot(mn2, v1)));
    } else {
        let s = dot(mn2, v1) / mo2;
        e1 = vec2<f32>(-s, s);
    }

    // Shrink short segments so the joint vertices of both ends cannot cross.
    var sf = vec2<f32>(1.0);
    if ((ok0 && ok3) || u.linecap == BUTT) {
        sf = vec2<f32>(
            max(0.0, seg_len / max(seg_len, w * (e0.x - e1.x))),
            max(0.0, seg_len / max(seg_len, w * (e0.y - e1.y))));
    }

    // Flat outputs (identical for the 4 vertices).
    let far = vec2<f32>(-1e12);
    let dummy = vec2<f32>(-0.70710678);   // with a far joint point the discard never fires
    o.linepts = vec4<f32>(select(far, p1, tr0), select(far, p2, tr1));
    o.miter_vecs = vec4<f32>(select(dummy, -mv1, tr0), select(dummy, mv2, tr1));
    o.ext_w = vec4<f32>(select(0.0, 1e12, ok0), select(0.0, 1e12, ok3), hw, min(1.0, u.width / AA_RADIUS));
    o.capmode = vec2<u32>(select(u.linecap, u.joinstyle, ok0), select(u.linecap, u.joinstyle, ok3));
    o.c1 = c1;
    o.c2 = c2;
    o.cum0 = cum0;
    o.nbr_dir = vec4<f32>(v0, v2);
    o.nbr_len = vec2<f32>(len0, len2);

    // This vertex: x = end (p1 / p2), y = side (-n / +n); strip order (0,0) (0,1) (1,0) (1,1).
    let x = vid >> 1u;
    let y = vid & 1u;
    let sx = f32(x) * 2.0 - 1.0;
    let sy = f32(y) * 2.0 - 1.0;
    let e = select(e0, e1, x == 1u);
    let ey = select(e.x, e.y, y == 1u);
    let sfy = select(sf.x, sf.y, y == 1u);
    var off: vec2<f32>;
    if (select(tr0, tr1, x == 1u) || !select(ok0, ok3, x == 1u)) {
        // Cap or truncated joint: extend along v1; overlap is split per pixel in the FS.
        off = sfy * ((hw * max(1.0, abs(ey)) + pad) * sx * v1 + sy * w * n1);
    } else {
        // Miter joint: end exactly on the shared miter line, so neighbours don't overlap.
        off = sy * sfy * w / select(mo1, mo2, x == 1u) * select(mn1, mn2, x == 1u);
    }
    let p = select(p1, p2, x == 1u) + off;
    let vp1 = p - p1;
    let vp2 = p - p2;
    o.quad_sdf = vec3<f32>(dot(vp1, -v1), dot(vp2, v1), dot(vp1, n1));
    o.trunc = vec2<f32>(
        select(-1.0, dot(vp1, sign(dot(mn1, -v1)) * mn1) - hw * abs(mo1), tr0),
        select(-1.0, dot(vp2, sign(dot(mn2, v1)) * mn2) - hw * abs(mo2), tr1));
    let e0y = select(e0.x, e0.y, y == 1u);
    let e1y = select(e1.x, e1.y, y == 1u);
    o.start_len = vec2<f32>(sfy * hw * e0y, max(1.0, seg_len - sfy * hw * (e0y - e1y)));
    o.pos = px_to_clip(p);
    return o;
}

fn brk(j: u32) -> f32 { return u.breaks[j / 4u][j % 4u]; }

// Makie's `gappy` pattern SDF: `s` in linewidths; negative inside a dash.
fn pattern_sdf(s: f32) -> f32 {
    let x = brk(0u) + s - floor(s / u.pattern_len) * u.pattern_len;
    for (var j = 0u; j + 1u < u.n_breaks; j++) {
        let a = brk(j);
        let b = brk(j + 1u);
        if (x >= a && x <= b) {
            let m = min(x - a, b - x);
            return select(m, -m, (j & 1u) == 0u);
        }
    }
    return 0.0;
}

@fragment
fn fs_line(in: LineV) -> @location(0) vec4<f32> {
    let hw = in.ext_w.z;
    // Pixel-exact split of overlapping truncated joints: compare the exact fragment position with
    // flat joint data both segments share (interpolated SDFs would differ between them).
    let d1 = dot(in.pos.xy - in.linepts.xy, in.miter_vecs.xy);
    let d2 = dot(in.pos.xy - in.linepts.zw, in.miter_vecs.zw);
    if ((in.quad_sdf.x > 0.0 && d1 > 0.0) || (in.quad_sdf.y > 0.0 && d2 >= 0.0)) { discard; }
    // The bodies of both segments also overlap on the inner side of a truncated joint (and along
    // the whole overlap of very sharp turns). There, on the neighbour's side of the split line and
    // inside the neighbour's strip, the neighbour draws. (GLMakie blends this region twice.)
    if (d1 > 0.0) {
        let q = in.pos.xy - in.linepts.xy;
        let v0 = in.nbr_dir.xy;
        if (abs(dot(q, perp(v0))) < hw + AA_RADIUS && dot(q, v0) > -in.nbr_len.x) { discard; }
    }
    if (d2 >= 0.0) {
        let q = in.pos.xy - in.linepts.zw;
        let v2 = in.nbr_dir.zw;
        if (abs(dot(q, perp(v2))) < hw + AA_RADIUS && dot(q, v2) < in.nbr_len.y) { discard; }
    }

    // SDF < 0 is inside; max() intersects, min() unites.
    var sdf: f32;
    if (in.capmode.x == ROUND) {
        sdf = min(length(in.quad_sdf.xz) - hw, in.quad_sdf.x);
    } else if (in.capmode.x == SQUARE) {
        sdf = in.quad_sdf.x - hw;
    } else {
        sdf = max(in.quad_sdf.x - in.ext_w.x, in.trunc.x);
    }
    if (in.capmode.y == ROUND) {
        sdf = max(sdf, min(length(in.quad_sdf.yz) - hw, in.quad_sdf.y));
    } else if (in.capmode.y == SQUARE) {
        sdf = max(sdf, in.quad_sdf.y - hw);
    } else {
        sdf = max(sdf, max(in.quad_sdf.y - in.ext_w.y, in.trunc.y));
    }
    sdf = max(sdf, abs(in.quad_sdf.z) - hw);
    // Steep AA along the discard split, so it opens no seam.
    sdf = max(sdf, min(in.quad_sdf.x + 1.0, 100.0 * d1 - 1.0));
    sdf = max(sdf, min(in.quad_sdf.y + 1.0, 100.0 * d2 - 1.0));
    if (u.n_breaks > 1u) {
        let lw = 2.0 * hw;
        sdf = max(sdf, lw * pattern_sdf((in.cum0 - in.quad_sdf.x + 0.5) / lw));
    }

    let f = clamp((-in.quad_sdf.x - in.start_len.x) / in.start_len.y, 0.0, 1.0);
    var c = mix(in.c1, in.c2, f);
    if (u.color_mode == 2u) { c = premul(cmap_lookup(c.x, u.cm, lut)); }
    let out = c * (in.ext_w.w * smoothstep(-AA_RADIUS, AA_RADIUS, -sdf));
    if (out.a <= 0.0) { discard; }
    return out;
}
