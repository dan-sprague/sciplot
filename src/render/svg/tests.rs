//! SVG output for primitives built directly as `DrawList`s (lines, glyphs, fields, meshes), plus
//! CPU rasterizations in `out/` for looking at them.

use super::*;
use crate::scene::drawlist::{AxisXform, Buf, ColorMapping, FieldPrim, GlyphInst, GridAxis, MeshVertex, RectPrim};
use std::sync::Arc;

const BLUE: Color = Color::hex(0x0072B2);
const ORANGE: Color = Color::hex(0xE69F00);

/// A 600x450 figure with one axis whose local coordinates are 0..10 x 0..10.
fn list(items: Vec<(Space, Prim)>) -> DrawList {
    let rect = Rect::new(60.0, 40.0, 500.0, 360.0);
    let items = items
        .into_iter()
        .enumerate()
        .map(|(i, (space, prim))| Item {
            z: 0.0,
            seq: i as u32,
            clip: matches!(space, Space::Data(_)).then_some(rect),
            space,
            prim,
        })
        .collect();
    DrawList {
        size: [600.0, 450.0],
        background: Color::rgb(1.0, 1.0, 1.0),
        axes: vec![AxisXform { rect, view: [0.0, 10.0, 0.0, 10.0] }],
        items,
    }
}

fn viridis() -> ColorMapping {
    let stops = [Color::hex(0x440154), Color::hex(0x21918C), Color::hex(0xFDE725)];
    let lut = (0..256)
        .map(|i| {
            let t = i as f32 / 255.0 * 2.0;
            let k = (t as usize).min(1);
            stops[k].lerp(stops[k + 1], t - k as f32)
        })
        .collect();
    ColorMapping {
        lut: Arc::new(lut),
        range: [0.0, 1.0],
        lowclip: None,
        highclip: None,
        nan_color: Color::TRANSPARENT,
        alpha: 1.0,
    }
}

fn line(pts: Vec<[f32; 2]>, color: PrimColor, width: f32) -> LinesPrim {
    LinesPrim {
        pts: Buf::transient(pts),
        color,
        width,
        pattern: None,
        cap: LineCap::Butt,
        join: JoinStyle::Miter,
        miter_limit: std::f32::consts::FRAC_PI_3,
        segments: false,
    }
}

fn svg(dl: &DrawList) -> String {
    document(dl, &SvgOptions::default())
}

/// Rasterizes on the CPU into `out/<name>.png` (for looking at it) and checks it is not blank.
fn rasterize(dl: &DrawList, name: &str) {
    #[cfg(feature = "cpu-png")]
    {
        let (w, h, data) = crate::render::cpu::render_rgba(dl, 2.0).unwrap();
        assert_eq!((w, h), (1200, 900));
        assert!(data.as_chunks::<4>().0.iter().any(|p| *p != [255, 255, 255, 255]));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("out");
        let _ = std::fs::create_dir_all(&dir);
        let img = crate::figure::RgbaImage { width: w, height: h, data };
        crate::figure::write_png(&dir.join(format!("{name}.png")), &img, 2.0).unwrap();
        let _ = std::fs::write(dir.join(format!("{name}.svg")), svg(dl));
    }
    #[cfg(not(feature = "cpu-png"))]
    let _ = (dl, name);
}

#[test]
fn root_and_background() {
    let s = svg(&list(vec![]));
    assert!(s.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(s.contains("width=\"450pt\" height=\"337.5pt\" viewBox=\"0 0 600 450\""));
    assert!(s.contains("<rect width=\"600\" height=\"450\" fill=\"#ffffff\"/>"));
    assert!(s.ends_with("</svg>\n"));
}

#[test]
fn lines_nan_breaks_dashes_and_joins() {
    let mut l =
        line(vec![[0.0, 0.0], [5.0, 5.0], [f32::NAN, 0.0], [6.0, 1.0], [9.0, 1.0]], PrimColor::Uniform(BLUE), 2.0);
    l.pattern = Some(vec![0.0, 3.0, 6.0]);
    let s = svg(&list(vec![(Space::Data(0), Prim::Lines(l))]));
    // Local (0,0) is the axis' bottom-left corner (60, 400); (5,5) is its center.
    assert!(s.contains("d=\"M60 400L310 220M360 364L510 364\""), "{s}");
    assert!(s.contains("stroke=\"#0072b2\""));
    assert!(s.contains("stroke-dasharray=\"6 6\""));
    assert!(s.contains("stroke-miterlimit=\"2\""));
    assert!(s.contains("fill=\"none\" stroke-width=\"2\""));
    assert_eq!(s.matches("<clipPath").count(), 1);
    assert!(s.contains("<g clip-path=\"url(#c1)\">"));
}

#[test]
fn closed_polyline_and_segments() {
    let tri = line(vec![[1.0, 1.0], [4.0, 1.0], [2.0, 4.0], [1.0, 1.0]], PrimColor::Uniform(BLUE), 3.0);
    let mut seg = line(vec![[0.0, 5.0], [10.0, 5.0], [5.0, 0.0], [5.0, 10.0]], PrimColor::Uniform(ORANGE), 1.0);
    seg.segments = true;
    seg.cap = LineCap::Round;
    let s = svg(&list(vec![(Space::Data(0), Prim::Lines(tri)), (Space::Data(0), Prim::Lines(seg))]));
    assert!(s.contains("d=\"M110 364L260 364L160 256Z\""), "{s}");
    assert!(s.contains("d=\"M60 220L560 220M310 400L310 40\""), "{s}");
    assert!(s.contains("stroke-linecap=\"round\""));
}

#[test]
fn per_point_line_colors() {
    let c = vec![BLUE, BLUE, BLUE, ORANGE].into_iter().map(|c| c.to_premul_u32()).collect();
    let l = line(vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0]], PrimColor::PerElement(Buf::transient(c)), 2.0);
    let s = svg(&list(vec![(Space::Data(0), Prim::Lines(l))]));
    // One run for the equal-colored segments, one gradient segment for the color change.
    assert_eq!(s.matches("<linearGradient").count(), 1, "{s}");
    assert!(s.contains("stroke=\"url(#l2)\""), "{s}");
    assert!(s.contains("d=\"M60 400L110 364L160 400\" stroke=\"#0072b2\""), "{s}");

    let vals = Buf::transient((0..50).map(|i| i as f32 / 49.0).collect());
    let pts = (0..50).map(|i| [i as f32 / 5.0, 5.0 + 4.0 * (i as f32 / 5.0).sin()]).collect();
    let v = line(pts, PrimColor::Values(vals, viridis()), 4.0);
    let s = svg(&list(vec![(Space::Data(0), Prim::Lines(v))]));
    assert_eq!(s.matches("<linearGradient").count(), 49);
}

#[test]
fn glyph_outlines() {
    let rt: crate::text::RichText = "Ag 10".into();
    let l = crate::text::layout(&rt, 20.0, Font::Regular, Color::rgb(0.0, 0.0, 0.0));
    let mut glyphs = crate::text::place(&l, [100.0, 100.0], (0.0, 0.0), 0.0);
    glyphs.extend(crate::text::place(&l, [50.0, 300.0], (0.5, 0.0), std::f64::consts::FRAC_PI_2));
    let s = svg(&list(vec![(Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }))]));
    // One outline per distinct glyph ('A', 'g', '1', '0'; the space has none) ...
    assert_eq!(s.matches("<path id=\"g").count(), 4, "{s}");
    // ... used once per drawn glyph, in a single black group.
    assert_eq!(s.matches("<use xlink:href=\"#g").count(), 8);
    assert!(s.contains("<g fill=\"#000000\">"));
    assert!(s.contains("transform=\"translate(100 95.64) scale(20)\""), "{s}");
    assert!(s.contains("rotate(-90) scale(20)"));
    // The 'A' advance is 0.667 em: the 'g' starts at 100 + 13.34.
    assert!(s.contains("translate(113.34 95.64)"), "{s}");
}

#[test]
fn fields_regular_and_irregular() {
    // 3x2 cells: value = i + 10 j; cell (0, 0) is at the bottom left.
    let vals: Vec<f32> = vec![0.0, 0.2, 0.4, 0.6, 0.8, f32::NAN];
    let f = FieldPrim {
        values: Buf::transient(vals.clone()),
        nx: 3,
        ny: 2,
        x: GridAxis::Regular { e0: 0.0, e1: 6.0 },
        y: GridAxis::Regular { e0: 0.0, e1: 10.0 },
        map: viridis(),
        interpolate: false,
    };
    let s = svg(&list(vec![(Space::Data(0), Prim::Field(f.clone()))]));
    assert!(s.contains("<image x=\"60\" y=\"40\" width=\"300\" height=\"360\" preserveAspectRatio=\"none\""), "{s}");
    assert!(s.contains("image-rendering=\"pixelated\" style=\"image-rendering:optimizeSpeed\""));
    let b64 = s.split("base64,").nth(1).unwrap().split('"').next().unwrap();
    let png = BASE64_STANDARD.decode(b64).unwrap();
    let mut dec = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut buf = vec![0; dec.output_buffer_size().unwrap()];
    let info = dec.next_frame(&mut buf).unwrap();
    // Upsampled 4x; the top-left image pixel is cell (0, 1) = 0.6, the bottom-left cell (0, 0) = 0.
    assert_eq!((info.width, info.height), (12, 8));
    let px = |x: usize, y: usize| &buf[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4];
    assert_eq!(px(0, 7), &Color::hex(0x440154).to_rgba8());
    assert_eq!(px(11, 0)[3], 0, "NaN cell is transparent");
    assert_eq!(px(0, 0), &field::Mapper::new(&viridis()).color(0.6).to_rgba8());

    let irregular = FieldPrim {
        x: GridAxis::Edges(Buf::transient(vec![0.0, 1.0, 3.0, 7.0])),
        values: Buf::transient(vec![0.0, 0.0, 0.5, 1.0, 1.0, 1.0]),
        ..f
    };
    let s = svg(&list(vec![(Space::Data(0), Prim::Field(irregular))]));
    assert!(!s.contains("<image"));
    // Row 0: two equal cells merged + one; row 1: all three merged.
    assert_eq!(s.matches("<rect x=").count(), 3 + 1, "{s}");
    assert!(s.contains("<rect x=\"60\" y=\"220\" width=\"150\" height=\"180\" fill=\"#440154\"/>"), "{s}");
    assert!(s.contains("shape-rendering=\"crispEdges\""));
}

#[test]
fn mesh_merges_same_color_triangles() {
    let c = BLUE.to_premul_u32();
    let o = ORANGE.with_alpha(0.5).to_premul_u32();
    let v = |x: f32, y: f32, color: u32| MeshVertex { pos: [x, y], color };
    // Two bars: blue (two triangles, opposite winding) and translucent orange.
    let verts = vec![
        v(1.0, 0.0, c),
        v(2.0, 0.0, c),
        v(1.0, 5.0, c),
        v(2.0, 0.0, c),
        v(1.0, 5.0, c),
        v(2.0, 5.0, c),
        v(3.0, 0.0, o),
        v(4.0, 0.0, o),
        v(4.0, 3.0, o),
    ];
    let s = svg(&list(vec![(Space::Data(0), Prim::Mesh(MeshPrim { verts: Buf::transient(verts) }))]));
    assert_eq!(s.matches("<path d=").count(), 2, "{s}");
    // Both triangles wound the same way, so their union has no hole or seam.
    assert!(s.contains("d=\"M110 400L110 220L160 400ZM160 400L110 220L160 220Z\" fill=\"#0072b2\""), "{s}");
    // Mesh colors are premultiplied RGBA8, so translucent colors round slightly.
    assert!(s.contains("fill=\"#e59f00\" fill-opacity=\"0.502\""), "{s}");
}

#[test]
fn rects_group_by_color_and_snap() {
    let grid = |x: f64| RectPrim {
        rect: Rect::new(x, 40.0, 1.0, 360.0),
        color: Color::gray(0.0).with_alpha(0.12),
        snap: true,
    };
    let rects = Prim::Rects(vec![
        grid(100.3),
        grid(200.3),
        RectPrim { rect: Rect::new(60.0, 40.0, 500.0, 360.0), color: BLUE, snap: false },
    ]);
    let dl = list(vec![(Space::Figure, rects)]);
    let s = svg(&dl);
    assert!(s.contains("<g fill=\"#000000\" fill-opacity=\"0.12\">\n<rect x=\"100.3\""), "{s}");
    assert!(s.contains("<rect x=\"60\" y=\"40\" width=\"500\" height=\"360\" fill=\"#0072b2\"/>"));
    // Snapped to the device-pixel grid at ppu 2 (width 2 px: center on a pixel edge).
    let snapped = document(&dl, &SvgOptions { pt_per_unit: 0.75, snap_ppu: Some(2.0) });
    assert!(snapped.contains("<rect x=\"100.5\""), "{snapped}");
}

#[test]
fn markers_all_shapes() {
    use crate::style::Marker::*;
    let shapes = [
        Circle, Rect, Diamond, Cross, XCross, UTriangle, DTriangle, LTriangle, RTriangle, Pentagon, Hexagon, Star5,
        FullCircle, FullRect,
    ];
    let mut items = Vec::new();
    for (i, m) in shapes.iter().enumerate() {
        let x = 0.5 + (i % 7) as f32 * 1.4;
        let y = if i < 7 { 7.0 } else { 3.0 };
        items.push((
            Space::Data(0),
            Prim::Markers(MarkersPrim {
                pos: Buf::transient(vec![[x, y], [x, y - 1.5]]),
                color: PrimColor::Uniform(BLUE),
                size: 40.0,
                sizes: None,
                marker: *m,
                stroke_color: Color::rgb(0.0, 0.0, 0.0),
                stroke_width: if i % 2 == 0 { 2.0 } else { 0.0 },
                rotation: 0.0,
            }),
        ));
    }
    let dl = list(items);
    let s = svg(&dl);
    assert!(
        s.contains(
            "<circle id=\"m2\" r=\"14.1\" stroke=\"#000000\" stroke-width=\"4\" stroke-linejoin=\"round\" \
             paint-order=\"stroke\"/>"
        ),
        "{s}"
    );
    assert_eq!(s.matches("<use").count(), 28);
    assert!(s.contains("<path id=\"m3\" d=\"M-12.629 12.629L12.629 12.629L12.629 -12.629L-12.629 -12.629Z\"/>"), "{s}");
    rasterize(&dl, "svg_unit_markers");
}

#[test]
fn marker_values_and_culling() {
    let pos = vec![[1.0, 1.0], [5.0, 5.0], [50.0, 5.0], [f32::NAN, 1.0]];
    let m = MarkersPrim {
        pos: Buf::transient(pos),
        color: PrimColor::Values(Buf::transient(vec![0.0, 1.0, 0.5, 0.5]), viridis()),
        size: 9.0,
        sizes: Some(Buf::transient(vec![9.0, 18.0, 9.0, 9.0])),
        marker: Marker::Circle,
        stroke_color: Color::rgb(0.0, 0.0, 0.0),
        stroke_width: 0.0,
        rotation: 0.0,
    };
    let s = svg(&list(vec![(Space::Data(0), Prim::Markers(m))]));
    // The far-away and NaN points are dropped; two sizes -> two symbols.
    assert_eq!(s.matches("<use").count(), 2, "{s}");
    assert_eq!(s.matches("<circle id=").count(), 2);
    assert!(s.contains("x=\"110\" y=\"364\" fill=\"#440154\""), "{s}");
    assert!(s.contains("fill=\"#fde725\""));
}

#[test]
fn everything_is_byte_stable_and_rasterizes() {
    let mut items = Vec::new();
    items.push((
        Space::Figure,
        Prim::Rects(vec![RectPrim {
            rect: Rect::new(60.0, 40.0, 500.0, 360.0),
            color: Color::gray(0.95),
            snap: false,
        }]),
    ));
    let n = 40u32;
    let vals: Vec<f32> = (0..n * n)
        .map(|k| {
            let (i, j) = ((k % n) as f32 / n as f32, (k / n) as f32 / n as f32);
            0.5 + 0.5 * ((6.0 * i).sin() * (4.0 * j).cos())
        })
        .collect();
    items.push((
        Space::Data(0),
        Prim::Field(FieldPrim {
            values: Buf::transient(vals),
            nx: n,
            ny: n,
            x: GridAxis::Regular { e0: 0.0, e1: 5.0 },
            y: GridAxis::Regular { e0: 0.0, e1: 10.0 },
            map: viridis(),
            interpolate: false,
        }),
    ));
    items.push((
        Space::Data(0),
        Prim::Field(FieldPrim {
            values: Buf::transient((0..12).map(|k| k as f32 / 11.0).collect()),
            nx: 4,
            ny: 3,
            x: GridAxis::Edges(Buf::transient(vec![5.5, 6.0, 7.0, 8.5, 10.0])),
            y: GridAxis::Edges(Buf::transient(vec![6.0, 7.0, 8.0, 10.0])),
            map: viridis(),
            interpolate: false,
        }),
    ));
    let sine: Vec<[f32; 2]> = (0..200).map(|i| [i as f32 / 20.0, 3.0 + 2.0 * (i as f32 / 10.0).sin()]).collect();
    let mut dashed = line(sine.clone(), PrimColor::Uniform(BLUE), 2.0);
    dashed.pattern = Some(vec![0.0, 3.0, 6.0]);
    items.push((Space::Data(0), Prim::Lines(dashed)));
    let lifted: Vec<[f32; 2]> = sine.iter().map(|p| [p[0], p[1] + 1.5]).collect();
    let vals = Buf::transient((0..200).map(|i| i as f32 / 199.0).collect());
    items.push((Space::Data(0), Prim::Lines(line(lifted, PrimColor::Values(vals, viridis()), 4.0))));
    let zig = vec![[5.5, 1.0], [6.5, 4.5], [7.5, 1.0], [8.5, 4.5], [f32::NAN, 0.0], [9.0, 1.0], [9.8, 1.0]];
    items.push((Space::Data(0), Prim::Lines(line(zig, PrimColor::Uniform(ORANGE.with_alpha(0.7)), 8.0))));
    let rt: crate::text::RichText = "Heatmap — sin(6x)·cos(4y)".into();
    let l = crate::text::layout(&rt, 16.0, Font::Bold, Color::rgb(0.0, 0.0, 0.0));
    let mut glyphs = crate::text::place(&l, [310.0, 30.0], (0.5, 0.0), 0.0);
    let yl = crate::text::layout(&"rotated label".into(), 14.0, Font::Italic, BLUE);
    glyphs.extend(crate::text::place(&yl, [40.0, 220.0], (0.5, 0.0), std::f64::consts::FRAC_PI_2));
    items.push((Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs })));
    let dl = list(items);
    let a = svg(&dl);
    assert_eq!(a, svg(&dl));
    assert_eq!(a.matches("<image").count(), 1);
    rasterize(&dl, "svg_unit_everything");
}

#[test]
fn glyph_inst_is_figure_space() {
    // Guard against silent API drift in GlyphInst (used by the outline path).
    let g = GlyphInst {
        font: Font::Regular,
        glyph: 36,
        pos: [1.0, 2.0],
        size: 10.0,
        color: Color::rgb(0.0, 0.0, 0.0),
        angle: 0.0,
    };
    let s = svg(&list(vec![(Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs: vec![g] }))]));
    assert!(s.contains("translate(1 2) scale(10)"), "{s}");
}

#[test]
fn translucent_marker_stroke_stays_outside() {
    let m = MarkersPrim {
        pos: Buf::transient(vec![[5.0, 5.0]]),
        color: PrimColor::Uniform(BLUE.with_alpha(0.5)),
        size: 20.0,
        sizes: None,
        marker: Marker::Circle,
        stroke_color: Color::rgb(0.0, 0.0, 0.0),
        stroke_width: 2.0,
        rotation: 0.0,
    };
    let s = svg(&list(vec![(Space::Data(0), Prim::Markers(m))]));
    assert!(s.contains("<mask id=\"k2\" maskUnits=\"userSpaceOnUse\""), "{s}");
    assert!(s.contains("<circle r=\"7.05\" fill=\"#000000\"/></mask>"), "{s}");
    assert!(
        s.contains(
            "<g id=\"m2\"><circle r=\"7.05\" fill=\"none\" stroke=\"#000000\" stroke-width=\"4\" \
             stroke-linejoin=\"round\" mask=\"url(#k2)\"/><circle r=\"7.05\"/></g>"
        ),
        "{s}"
    );
    assert!(s.contains("<use xlink:href=\"#m2\" x=\"310\" y=\"220\" fill=\"#0072b2\" fill-opacity=\"0.5\"/>"), "{s}");
}
