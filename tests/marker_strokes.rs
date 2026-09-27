//! Marker strokes follow CairoMakie on the GPU and in SVG: centered on the outline, painted over
//! the fill, with sharp (mitered) corners.
use sciplot::prelude::*;
use sciplot::{Error, RgbaImage};

/// One `FullRect` marker (40 units) with an 8 unit black stroke on a white figure at 1 px/unit.
fn figure() -> Figure {
    let fig = Figure::new().size((120, 120)).backgroundcolor(WHITE);
    let ax = Axis::new(fig.at(1, 1)).limits(0.0, 1.0, 0.0, 1.0);
    ax.hidedecorations(true);
    ax.hidespines();
    ax.scatter([0.5], [0.5]).marker(Marker::FullRect).markersize(40).color(RED).strokewidth(8).strokecolor(BLACK);
    fig
}

fn px(img: &RgbaImage, x: usize, y: usize) -> [u8; 3] {
    let i = 4 * (y * img.width as usize + x);
    [img.data[i], img.data[i + 1], img.data[i + 2]]
}

/// Checks the marker's geometry in a rendered image.
fn check(img: &RgbaImage, what: &str) {
    let (w, h) = (img.width as usize, img.height as usize);
    // Bounding box of clearly non-white pixels.
    let ink = |x: usize, y: usize| px(img, x, y).iter().any(|c| *c < 128);
    let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
    for y in 0..h {
        for x in 0..w {
            if ink(x, y) {
                (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            }
        }
    }
    // Centered stroke: 40 + 8 wide (GLMakie's outer stroke would make it 40 + 16).
    assert!((x1 + 1 - x0).abs_diff(48) <= 1 && (y1 + 1 - y0).abs_diff(48) <= 1, "{what}: {x0}..{x1} x {y0}..{y1}");
    // Mitered corners: the bounding box corner is stroke (a round join would leave it white).
    assert!(px(img, x0 + 1, y0 + 1).iter().all(|c| *c < 60), "{what}: corner {:?}", px(img, x0 + 1, y0 + 1));
    // Along the middle row: 8 px of stroke, then the red fill.
    let cy = (y0 + y1) / 2;
    assert!(px(img, x0 + 6, cy).iter().all(|c| *c < 60), "{what}: stroke {:?}", px(img, x0 + 6, cy));
    let fill = px(img, x0 + 10, cy);
    assert!(fill[0] > 200 && fill[1] < 60 && fill[2] < 60, "{what}: fill {fill:?}");
}

#[test]
fn gpu_marker_stroke_is_centered_and_mitered() {
    match figure().render_rgba(&Save::new().px_per_unit(1.0)) {
        Ok(img) => check(&img, "gpu"),
        Err(Error::NoGpuAdapter(_)) => {}
        Err(e) => panic!("{e}"),
    }
}

#[cfg(feature = "cpu-png")]
#[test]
fn svg_marker_stroke_is_centered_and_mitered() {
    let img = figure().render_rgba(&Save::new().px_per_unit(1.0).cpu(true)).unwrap();
    check(&img, "svg");
}
