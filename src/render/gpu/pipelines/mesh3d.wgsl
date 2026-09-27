// 3D triangle meshes (Axis3 surfaces) with depth writes and Makie's FastShading: ambient light
// plus one directional light, Blinn-Phong, evaluated per pixel in world space (the formula of
// CairoMakie's `_calculate_shaded_vertexcolors`).
//
// Provenance: shading algorithm adapted from CairoMakie 0.15.14 `src/mesh.jl`
// (`_calculate_shaded_vertexcolors`), evaluated per pixel. MIT licensed; see
// THIRD_PARTY_NOTICES.md.

struct Mesh3dU {
    v: View3,
    s: vec4<f32>,      // local -> world scale; w: ambient
    eye: vec4<f32>,    // camera position in local coordinates; w: light color
    light: vec4<f32>,  // world direction the light travels; w: 1 = shading on
    ns: vec4<f32>,     // data-space normal -> world (componentwise); w: shininess
    color: vec4<f32>,  // premultiplied (color_mode 0)
    color_mode: u32,   // 0 uniform, 1 per-vertex premultiplied RGBA8, 2 values -> colormap
    diffuse: f32,
    specular: f32,
    _p: u32,
    cm: CMap,
};
@group(1) @binding(0) var<uniform> u: Mesh3dU;
@group(1) @binding(1) var lut: texture_2d<f32>;

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) col: u32,
};

struct MeshV {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) c: vec4<f32>,     // premultiplied color, or (value, 0, 0, 0)
};

@vertex
fn vs_mesh3d(v: MeshIn) -> MeshV {
    var o: MeshV;
    o.pos = u.v.m * vec4<f32>(v.pos, 1.0);
    o.local = v.pos;
    o.normal = v.normal;
    if (u.color_mode == 1u) {
        o.c = unpack4x8unorm(v.col);
    } else if (u.color_mode == 2u) {
        o.c = vec4<f32>(bitcast<f32>(v.col), 0.0, 0.0, 0.0);
    } else {
        o.c = u.color;
    }
    return o;
}

@fragment
fn fs_mesh3d(in: MeshV) -> @location(0) vec4<f32> {
    if (!in_box(u.v, in.local)) { discard; }
    var c = in.c;
    if (u.color_mode == 2u) { c = premul(cmap_lookup(in.c.x, u.cm, lut)); }
    if (u.light.w != 0.0) {
        let n = in.normal * u.ns.xyz;
        let ln = length(n);
        if (ln > 0.0) {
            let N = n / ln;
            let L = u.light.xyz;
            let v = normalize((in.local - u.eye.xyz) * u.s.xyz);
            let diff = max(dot(L, -N), 0.0);
            let H = normalize(L + v);
            let spec = pow(max(dot(H, -N), 0.0), u.ns.w);
            let lc = u.eye.w;
            c = vec4<f32>((u.s.w + lc * diff * u.diffuse) * c.rgb + lc * u.specular * spec * c.a, c.a);
        }
    }
    if (c.a <= 0.0) { discard; }
    return c;
}
