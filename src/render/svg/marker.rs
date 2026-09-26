//! Makie marker geometry as vector shapes. Mirrors `marker_sdf` in
//! `render/gpu/pipelines/sprite.wgsl` (all sizes in units of markersize, y up).

use crate::style::Marker;

/// Makie's `Circle` marker radius (the symbol circle is 0.705 × markersize wide).
const CIRCLE_R: f64 = 0.3525;
/// Half side of the `:rect` marker.
const RECT_HALF: f64 = 0.315718;
/// Cross arms: half length and half thickness.
const CROSS_A: f64 = 0.375;
const CROSS_B: f64 = 0.1245;

/// A marker outline in units of markersize, y up.
pub(crate) enum Shape {
    Circle(f64),
    Polygon(Vec<[f64; 2]>),
}

fn rotated(pts: Vec<[f64; 2]>, a: f64) -> Vec<[f64; 2]> {
    let (s, c) = a.sin_cos();
    pts.into_iter().map(|[x, y]| [c * x - s * y, s * x + c * y]).collect()
}

fn ngon(n: usize, r_out: f64, r_in: f64, star: bool) -> Vec<[f64; 2]> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let r = if star && i % 2 == 1 { r_in } else { r_out };
            [a.sin() * r, a.cos() * r]
        })
        .collect()
}

fn square(h: f64) -> Vec<[f64; 2]> {
    vec![[-h, -h], [h, -h], [h, h], [-h, h]]
}

fn cross() -> Vec<[f64; 2]> {
    let (a, b) = (CROSS_A, CROSS_B);
    vec![[b, a], [b, b], [a, b], [a, -b], [b, -b], [b, -a], [-b, -a], [-b, -b], [-a, -b], [-a, b], [-b, b], [-b, a]]
}

/// The outline of `m` (unit markersize, y up, unrotated).
pub(crate) fn shape(m: Marker) -> Shape {
    use std::f64::consts::FRAC_PI_4;
    let t = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| Shape::Polygon(vec![a, b, c]);
    match m {
        Marker::Circle => Shape::Circle(CIRCLE_R),
        Marker::FullCircle => Shape::Circle(0.5),
        Marker::Rect => Shape::Polygon(square(RECT_HALF)),
        Marker::FullRect => Shape::Polygon(square(0.5)),
        Marker::Diamond => Shape::Polygon(rotated(square(RECT_HALF), FRAC_PI_4)),
        Marker::Cross => Shape::Polygon(cross()),
        Marker::XCross => Shape::Polygon(rotated(cross(), FRAC_PI_4)),
        Marker::UTriangle => t([0.0, 0.485], [-0.36375, -0.2425], [0.36375, -0.2425]),
        Marker::DTriangle => t([0.0, -0.485], [0.36375, 0.2425], [-0.36375, 0.2425]),
        Marker::LTriangle => t([-0.485, 0.0], [0.2425, -0.36375], [0.2425, 0.36375]),
        Marker::RTriangle => t([0.485, 0.0], [-0.2425, 0.36375], [-0.2425, -0.36375]),
        Marker::Pentagon => Shape::Polygon(ngon(5, 0.375, 0.375, false)),
        Marker::Hexagon => Shape::Polygon(ngon(6, 0.375, 0.375, false)),
        Marker::Star5 => Shape::Polygon(ngon(10, 0.45, 0.21, true)),
    }
}

/// Polygon vertices in figure units (y down) for markersize `size`, rotated `rotation` radians
/// counter-clockwise on screen (Makie's convention).
pub(crate) fn polygon_units(pts: &[[f64; 2]], size: f64, rotation: f64) -> Vec<[f64; 2]> {
    let (s, c) = rotation.sin_cos();
    pts.iter()
        .map(|&[x, y]| {
            let (rx, ry) = (c * x - s * y, s * x + c * y);
            [rx * size, -ry * size]
        })
        .collect()
}

/// Radius (units of markersize) of a circle enclosing the shape.
pub(crate) fn bounding_radius(sh: &Shape) -> f64 {
    match sh {
        Shape::Circle(r) => *r,
        Shape::Polygon(p) => p.iter().map(|v| v[0].hypot(v[1])).fold(0.0, f64::max),
    }
}
