// Shared by the 3D pipelines (lines3d, markers3d, mesh3d): the Axis3 camera and clip box.

struct View3 {
    m: mat4x4<f32>,       // local -> target clip space (depth 0..1)
    clip_lo: vec4<f32>,   // local clip box; clip_lo.w = 1 enables it
    clip_hi: vec4<f32>,
};

fn finite3(p: vec3<f32>) -> bool { return finite_bits(p.x) && finite_bits(p.y) && finite_bits(p.z); }

// Makie's `clip`: content outside the limits box is hidden.
fn in_box(v: View3, p: vec3<f32>) -> bool {
    if (v.clip_lo.w == 0.0) { return true; }
    return all(p >= v.clip_lo.xyz) && all(p <= v.clip_hi.xyz);
}

// Clip space -> device pixels (origin top-left, y down).
fn clip_to_px(c: vec4<f32>) -> vec2<f32> {
    let n = c.xy / c.w;
    return vec2<f32>((n.x + 1.0) * 0.5 * g.target_px.x, (1.0 - n.y) * 0.5 * g.target_px.y);
}

// A screen-space vertex at device pixel `px` with the depth of clip position `c`.
fn px_at_depth(px: vec2<f32>, c: vec4<f32>) -> vec4<f32> {
    let q = px_to_clip(px);
    return vec4<f32>(q.xy, clamp(c.z / c.w, 0.0, 1.0), 1.0);
}
