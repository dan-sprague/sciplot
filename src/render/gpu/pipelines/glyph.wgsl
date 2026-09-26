// Text: instanced quads over an R8 coverage atlas (glyphs rasterized at their exact device size
// and subpixel phase). Axis-aligned and quarter-turn glyphs sit on whole device pixels and read
// texels exactly; other angles rotate the quad about the glyph origin and sample linearly.

struct GlyphU {
    atlas_px: vec2<f32>,
    _pad: vec2<f32>,
};

struct GlyphI {
    origin: vec2<f32>,   // glyph origin on the baseline, device px
    off: vec2<f32>,      // slot top-left relative to the origin, upright, y down
    size: vec2<f32>,     // slot size in px (= texels)
    uv: vec2<f32>,       // slot top-left texel
    cs: vec2<f32>,       // cos, sin of the counter-clockwise angle
    color: u32,          // premultiplied RGBA8
    flags: u32,          // 1: sample linearly
};

@group(1) @binding(0) var<uniform> gu: GlyphU;
@group(1) @binding(1) var<storage, read> glyphs: array<GlyphI>;
@group(1) @binding(2) var atlas: texture_2d<f32>;

struct GlyphV {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,                          // texels
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) slot: vec4<f32>,     // texel box, for clamping linear taps
    @location(3) @interpolate(flat) flags: u32,
};

@vertex
fn vs_glyph(@builtin(vertex_index) vid: u32, @builtin(instance_index) i: u32) -> GlyphV {
    let gi = glyphs[i];
    let corner = vec2<f32>(f32(vid & 1u), f32(vid >> 1u));
    let l = gi.off + corner * gi.size;
    let c = gi.cs.x;
    let s = gi.cs.y;
    let d = vec2<f32>(l.x * c + l.y * s, -l.x * s + l.y * c);
    var o: GlyphV;
    o.pos = px_to_clip(gi.origin + d);
    o.uv = gi.uv + corner * gi.size;
    o.color = unpack4x8unorm(gi.color);
    o.slot = vec4<f32>(gi.uv + 0.5, gi.uv + gi.size - 0.5);
    o.flags = gi.flags;
    return o;
}

@fragment
fn fs_glyph(in: GlyphV) -> @location(0) vec4<f32> {
    var cov: f32;
    if ((in.flags & 1u) == 0u) {
        cov = textureLoad(atlas, vec2<i32>(floor(in.uv)), 0).r;
    } else {
        let p = clamp(in.uv, in.slot.xy, in.slot.zw);
        cov = textureSampleLevel(atlas, lin_samp, p / gu.atlas_px, 0.0).r;
    }
    if (cov <= 0.0) { discard; }
    // Coverage as-is, like Cairo's grayscale antialiasing (no gamma adjustment).
    return in.color * cov;
}
