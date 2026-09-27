//! The backend-neutral output of scene building. The GPU renderer and the SVG writer both consume
//! a `DrawList`; neither knows about axes, plots or layout.
//!
//! Coordinates: `Space::Figure` is figure units with the origin at the top-left and y pointing
//! down. `Space::Data(i)` is the f32 "local" space of axis `i` (see `transform::Rebase`), mapped
//! to the axis rectangle through `AxisXform`.

use crate::color::Color;
use crate::style::{JoinStyle, LineCap, Marker};
use crate::text::Font;
use std::sync::Arc;

/// Axis-aligned rectangle in figure units (y down).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn contains(&self, p: [f64; 2]) -> bool {
        p[0] >= self.x && p[0] <= self.right() && p[1] >= self.y && p[1] <= self.bottom()
    }
}

/// Cache identity for a buffer: GPU backends keep uploads keyed by `(uid, part)` and re-upload
/// only when `rev` changes. `key = None` means "transient, upload every frame".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BufKey {
    pub uid: u64,
    pub part: u8,
    pub rev: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Buf<T> {
    pub key: Option<BufKey>,
    pub data: Arc<Vec<T>>,
}

impl<T> Buf<T> {
    pub fn transient(data: Vec<T>) -> Buf<T> {
        Buf { key: None, data: Arc::new(data) }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// Maps the axis' local coordinates to its rectangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisXform {
    pub rect: Rect,
    /// The visible range in local coordinates: `[x_left, x_right, y_bottom, y_top]`
    /// (reversed axes have `x_left > x_right` etc.).
    pub view: [f64; 4],
}

impl AxisXform {
    /// `(sx, sy, tx, ty)` so that `px = local * s + t` in device pixels at `ppu`, y down.
    pub fn affine(&self, ppu: f64) -> [f64; 4] {
        let [x0, x1, y0, y1] = self.view;
        let sx = self.rect.w * ppu / (x1 - x0);
        let tx = self.rect.x * ppu - x0 * sx;
        let sy = -self.rect.h * ppu / (y1 - y0);
        let ty = (self.rect.y + self.rect.h) * ppu - y0 * sy;
        [sx, sy, tx, ty]
    }

    /// Local -> figure units.
    pub fn to_units(&self, p: [f32; 2]) -> [f64; 2] {
        let [sx, sy, tx, ty] = self.affine(1.0);
        [p[0] as f64 * sx + tx, p[1] as f64 * sy + ty]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Space {
    Figure,
    Data(u16),
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub z: f32,
    pub seq: u32,
    pub clip: Option<Rect>,
    pub space: Space,
    pub prim: Prim,
}

#[derive(Clone, Debug)]
pub(crate) enum Prim {
    /// Filled axis-aligned rectangles (backgrounds, spines, ticks, grid lines). Always figure space.
    Rects(Vec<RectPrim>),
    /// Filled triangles (bars, bands, legend patches).
    Mesh(MeshPrim),
    Markers(MarkersPrim),
    Lines(LinesPrim),
    Glyphs(GlyphsPrim),
    Field(FieldPrim),
    /// 3D primitives of an `Axis3`, depth-tested against the other 3D items of their view group.
    Lines3d(Lines3dPrim),
    Markers3d(Markers3dPrim),
    Mesh3d(Mesh3dPrim),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RectPrim {
    pub rect: Rect,
    pub color: Color,
    /// Snap the position (not the size) to device pixels for crisp thin lines.
    pub snap: bool,
}

/// Triangle list vertex: position in the item's space and a premultiplied RGBA8 color.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshVertex {
    pub pos: [f32; 2],
    pub color: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct MeshPrim {
    /// Triangle list (3 vertices per triangle).
    pub verts: Buf<MeshVertex>,
}

/// Colormap application parameters.
#[derive(Clone, Debug)]
pub(crate) struct ColorMapping {
    /// 256 premultiplied-free sRGB colors.
    pub lut: Arc<Vec<Color>>,
    /// Colorrange in the stored value space (after `(v - off) * k`).
    pub range: [f32; 2],
    pub lowclip: Option<Color>,
    pub highclip: Option<Color>,
    pub nan_color: Color,
    pub alpha: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum PrimColor {
    Uniform(Color),
    /// Premultiplied RGBA8 per element.
    PerElement(Buf<u32>),
    /// Values mapped through a colormap per element.
    Values(Buf<f32>, ColorMapping),
}

#[derive(Clone, Debug)]
pub(crate) struct MarkersPrim {
    pub pos: Buf<[f32; 2]>,
    pub color: PrimColor,
    /// Marker size in units, or per-point sizes.
    pub size: f32,
    pub sizes: Option<Buf<f32>>,
    pub marker: Marker,
    pub stroke_color: Color,
    /// Stroke width in units.
    pub stroke_width: f32,
    pub rotation: f32,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct LinesPrim {
    /// Points; NaN breaks a polyline.
    pub pts: Buf<[f32; 2]>,
    /// Uniform, per-point, or per-point values (interpolated along each segment, then colormapped).
    pub color: PrimColor,
    /// Line width in units.
    pub width: f32,
    /// Cumulative dash boundaries in units of linewidth (None = solid).
    pub pattern: Option<Vec<f32>>,
    pub cap: LineCap,
    pub join: JoinStyle,
    /// Makie's `miter_limit`: joints turning by more than `pi - miter_limit` radians are
    /// beveled (π/3 by default, i.e. SVG `stroke-miterlimit = 1 / sin(miter_limit / 2) = 2`).
    pub miter_limit: f32,
    /// `true`: independent segments (pairs of points); `false`: a polyline with NaN breaks.
    pub segments: bool,
    /// A polyline whose last point repeats the first: its ends are joined instead of capped
    /// (Makie closes such loops of 3+ segments without NaNs).
    pub closed: bool,
    /// `pts.key.rev` is an append-only revision (`data::points::append_rev`): backends holding
    /// an older buffer of the same generation only need the new tail.
    pub append: bool,
}

/// One glyph to draw, positioned in figure units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphInst {
    pub font: Font,
    pub glyph: u16,
    /// Baseline origin in figure units.
    pub pos: [f32; 2],
    /// Font size in units (Makie fontsize).
    pub size: f32,
    pub color: Color,
    /// Rotation about `pos`, radians counter-clockwise.
    pub angle: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphsPrim {
    pub glyphs: Vec<GlyphInst>,
}

/// Cell boundaries along one heatmap dimension, in local coordinates.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum GridAxis {
    /// `n` equal cells from `e0` to `e1`.
    Regular { e0: f64, e1: f64 },
    /// `n + 1` monotone edges.
    Edges(Buf<f32>),
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct FieldPrim {
    /// x-fastest values, `nx * ny`.
    pub values: Buf<f32>,
    pub nx: u32,
    pub ny: u32,
    pub x: GridAxis,
    pub y: GridAxis,
    pub map: ColorMapping,
    pub interpolate: bool,
}

/// The camera of one `Axis3` for one frame, shared by its 3D items (`Space::Figure`; the items
/// carry their own projection).
#[derive(Clone, Debug)]
pub(crate) struct View3d {
    /// Local coordinates -> Makie's clip space of `area` (OpenGL conventions: x, y, z in -1..1,
    /// y up).
    pub mvp: [[f64; 4]; 4],
    /// The scene area clip space maps to (figure units, y down).
    pub area: Rect,
    /// Local -> world (the normalized box the lights live in): `world = local * scale + offset`.
    pub world_scale: [f64; 3],
    pub world_offset: [f64; 3],
    /// Data-space normal -> world-space direction: multiply componentwise, then normalize
    /// (Makie's normal matrix `inv(model)ᵀ`).
    pub normal_scale: [f64; 3],
    /// Content outside this local-space box `[min, max]` is hidden (Makie's `clip`).
    pub clip: Option<[[f32; 3]; 2]>,
    /// World-space direction the light travels (Makie's camera-relative default light).
    pub light_dir: [f64; 3],
    /// World-space camera position.
    pub eye: [f64; 3],
    /// Makie's `ambient` and directional `light_color` (gray levels).
    pub ambient: f32,
    pub light_color: f32,
    /// Items of the same group share one depth buffer (one Axis3 in one frame).
    pub group: u64,
}

/// A 3D polyline (NaN breaks it), `width` in units, drawn with round joins.
#[derive(Clone, Debug)]
pub(crate) struct Lines3dPrim {
    pub view: Arc<View3d>,
    pub pts: Buf<[f32; 3]>,
    /// Uniform, per-point, or per-point values (interpolated along each segment).
    pub color: PrimColor,
    pub width: f32,
    /// `pts.key.rev` is an append-only revision (see `LinesPrim::append`).
    pub append: bool,
}

/// Screen-space markers at 3D positions (depth at their centre).
#[derive(Clone, Debug)]
pub(crate) struct Markers3dPrim {
    pub view: Arc<View3d>,
    pub pos: Buf<[f32; 3]>,
    pub color: PrimColor,
    /// Marker size in units, or per-point sizes.
    pub size: f32,
    pub sizes: Option<Buf<f32>>,
    pub marker: Marker,
    pub stroke_color: Color,
    pub stroke_width: f32,
    /// `pos.key.rev` is an append-only revision (see `LinesPrim::append`).
    pub append: bool,
}

/// A mesh vertex in local coordinates with its data-space normal.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex3d {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

/// Makie's shading attributes (`FastShading`: ambient + one directional light, Blinn-Phong).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Material {
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
}

/// A triangle list in 3D with per-vertex colors.
#[derive(Clone, Debug)]
pub(crate) struct Mesh3dPrim {
    pub view: Arc<View3d>,
    /// Triangle list (3 vertices per triangle).
    pub verts: Buf<Vertex3d>,
    /// Uniform, per-vertex, or per-vertex values (interpolated, then colormapped).
    pub color: PrimColor,
    /// Lit by the view's light (`None`: flat colors, Makie's `NoShading`).
    pub shading: Option<Material>,
}

/// Everything needed to draw one frame.
#[derive(Clone, Debug)]
pub(crate) struct DrawList {
    /// Figure size in units.
    pub size: [f64; 2],
    pub background: Color,
    pub axes: Vec<AxisXform>,
    pub items: Vec<Item>,
}

impl DrawList {
    /// Stable-sorts items by `(z, seq)` (painter's order).
    pub fn sort(&mut self) {
        self.items.sort_by(|a, b| a.z.total_cmp(&b.z).then(a.seq.cmp(&b.seq)));
    }
}

/// Collects items in submission order.
pub(crate) struct Emitter {
    pub items: Vec<Item>,
    seq: u32,
}

impl Emitter {
    pub fn new() -> Self {
        Emitter { items: Vec::new(), seq: 0 }
    }
    pub fn push(&mut self, z: f32, clip: Option<Rect>, space: Space, prim: Prim) {
        self.items.push(Item { z, seq: self.seq, clip, space, prim });
        self.seq += 1;
    }
}
