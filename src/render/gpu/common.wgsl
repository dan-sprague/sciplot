// Shared definitions, prepended to every pipeline's shader.
//
// Provenance: `cmap_lookup` algorithm adapted from GLMakie 0.13.14 `assets/shader/util.vert` and
// `assets/shader/lines.frag` (`get_color_from_cmap`: lowclip / highclip / nan_color and the
// half-texel LUT remap). MIT licensed; see THIRD_PARTY_NOTICES.md.

struct Globals {
    target_px: vec2<f32>,
    ppu: f32,
    _pad: f32,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var lin_samp: sampler;

// Device pixels (origin top-left, y down) -> clip space.
fn px_to_clip(px: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(2.0 * px.x / g.target_px.x - 1.0, 1.0 - 2.0 * px.y / g.target_px.y, 0.0, 1.0);
}

// Metal compiles with fast-math, so test float bits instead of relying on isnan/isinf.
fn finite_bits(x: f32) -> bool { return (bitcast<u32>(x) & 0x7fffffffu) < 0x7f800000u; }
fn nan_bits(x: f32) -> bool { return (bitcast<u32>(x) & 0x7fffffffu) > 0x7f800000u; }

struct CMap {
    range: vec2<f32>,
    n: f32,
    alpha: f32,
    lowclip: vec4<f32>,
    highclip: vec4<f32>,
    nan_color: vec4<f32>,
};

// Straight-alpha color for value `v` through a 1D LUT texture (n texels), Makie clipping rules.
fn cmap_lookup(v: f32, cm: CMap, lut: texture_2d<f32>) -> vec4<f32> {
    if (nan_bits(v)) { return cm.nan_color; }
    if (v < cm.range.x) { return cm.lowclip; }
    if (v > cm.range.y) { return cm.highclip; }
    let w = cm.range.y - cm.range.x;
    let t = select(0.5, (v - cm.range.x) / w, w > 0.0);
    let s = (1.0 - 1.0 / cm.n) * t + 0.5 / cm.n;
    let c = textureSampleLevel(lut, lin_samp, vec2<f32>(s, 0.5), 0.0);
    return vec4<f32>(c.rgb, c.a * cm.alpha);
}

fn premul(c: vec4<f32>) -> vec4<f32> { return vec4<f32>(c.rgb * c.a, c.a); }
