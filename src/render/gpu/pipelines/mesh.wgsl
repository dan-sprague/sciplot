// Filled triangles (fills, decoration rects). MSAA provides the antialiasing.

struct MeshU { xform: vec4<f32> };
@group(1) @binding(0) var<uniform> mu: MeshU;

struct MeshIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,   // premultiplied
};
struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    let px = v.pos * mu.xform.xy + mu.xform.zw;
    o.pos = px_to_clip(px);
    o.color = v.color;
    return o;
}

@fragment
fn fs_mesh(v: MeshOut) -> @location(0) vec4<f32> {
    return v.color;
}
