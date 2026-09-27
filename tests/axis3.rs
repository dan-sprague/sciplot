//! Axis3 against Makie: camera matrices, projected corners, ticks, tick labels, axis labels, the
//! title, grid and frame lines (`tests/fixtures/axis3.json`, from `tools/gen_axis3_fixtures.jl`),
//! plus rendering checks (depth occlusion, WebGL2 limits, SVG) and interaction helpers.

use ezviz::gpu_testing::GpuContext;
use ezviz::prelude::*;
use ezviz::{Axis3Geometry, Error};
use serde_json::Value;
use std::f64::consts::PI;

fn fixtures() -> Vec<Value> {
    let s = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/axis3.json")).unwrap();
    serde_json::from_str::<Value>(&s).unwrap().as_array().unwrap().clone()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn vec_f(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(f).collect()
}
fn pts(v: &Value) -> Vec<Vec<f64>> {
    v.as_array().unwrap().iter().map(vec_f).collect()
}
fn align(v: &Value) -> (f64, f64) {
    let a: Vec<&str> = v.as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    let h = match a[0] {
        "left" => 0.0,
        "center" => 0.5,
        _ => 1.0,
    };
    let w = match a[1] {
        "bottom" => 0.0,
        "center" => 0.5,
        _ => 1.0,
    };
    (h, w)
}

/// The Axis3 of a fixture case, laid out.
fn geometry(case: &Value) -> Axis3Geometry {
    let size = vec_f(&case["size"]);
    let fig = Figure::new().size((size[0], size[1]));
    let l = vec_f(&case["limits"]);
    let ax = Axis3::new(fig.at(1, 1)).limits(l[0], l[1], l[2], l[3], l[4], l[5]);
    let a = &case["attrs"];
    if let Some(v) = a.get("azimuth") {
        ax.azimuth(f(v));
    }
    if let Some(v) = a.get("elevation") {
        ax.elevation(f(v));
    }
    if let Some(v) = a.get("perspectiveness") {
        ax.perspectiveness(f(v));
    }
    match a.get("aspect") {
        Some(Value::String(s)) if s == "data" => {
            ax.aspect(Aspect3::Data);
        }
        Some(Value::String(s)) if s == "equal" => {
            ax.aspect(Aspect3::Equal);
        }
        Some(Value::Array(v)) => {
            ax.aspect((f(&v[0]), f(&v[1]), f(&v[2])));
        }
        _ => {}
    }
    match a.get("viewmode").and_then(|v| v.as_str()) {
        Some("fit") => {
            ax.viewmode(ViewMode::Fit);
        }
        Some("stretch") => {
            ax.viewmode(ViewMode::Stretch);
        }
        _ => {}
    }
    if let Some(v) = a.get("protrusions") {
        let p = vec_f(v);
        ax.protrusions((p[0], p[1], p[2], p[3]));
    }
    if a.get("xreversed").is_some() {
        ax.xreversed(true);
    }
    for (k, set) in [("title", 0), ("xlabel", 1), ("zlabel", 2)] {
        if let Some(Value::String(s)) = a.get(k) {
            match set {
                0 => ax.title(s.as_str()),
                1 => ax.xlabel(s.as_str()),
                _ => ax.zlabel(s.as_str()),
            };
        }
    }
    ax.geometry().unwrap()
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{what}: {a} vs Makie {b}");
}

fn close_pt(a: &[f64], b: &[f64], tol: f64, what: &str) {
    for i in 0..a.len().min(b.len()) {
        assert!((a[i] - b[i]).abs() <= tol, "{what}: {a:?} vs Makie {b:?}");
    }
}

#[test]
fn camera_matches_makie() {
    for case in fixtures() {
        let name = case["name"].as_str().unwrap();
        let g = geometry(&case);
        close_pt(&g.bbox, &vec_f(&case["bbox"]), 1e-9, &format!("{name} bbox"));
        close_pt(&g.viewport, &vec_f(&case["viewport"]), 1e-9, &format!("{name} viewport"));
        for (m, key) in [(&g.model, "model"), (&g.view, "view"), (&g.projection, "projection")] {
            let want = pts(&case[key]);
            for r in 0..4 {
                for c in 0..4 {
                    close(m[r][c], want[r][c], 1e-7, &format!("{name} {key}[{r}][{c}]"));
                }
            }
        }
        close_pt(&g.eyeposition, &vec_f(&case["eyeposition"]), 1e-3, &format!("{name} eye"));
        for (p, px) in pts(&case["corners"]).iter().zip(pts(&case["corners_px"])) {
            let q = g.project([p[0], p[1], p[2]]);
            close_pt(&q, &px, 1e-3, &format!("{name} corner {p:?}"));
        }
    }
}

#[test]
fn decorations_match_makie() {
    for case in fixtures() {
        let name = case["name"].as_str().unwrap();
        let g = geometry(&case);
        for (d, want) in case["dims"].as_array().unwrap().iter().enumerate() {
            let what = |s: &str| format!("{name} dim {d} {s}");
            let dg = &g.dims[d];
            let ticks = pts(&want["ticks"]);
            assert_eq!(dg.ticks.len() * 2, ticks.len(), "{}", what("tick count"));
            for (k, seg) in dg.ticks.iter().enumerate() {
                close_pt(&seg[0], &ticks[2 * k], 1e-3, &what("tick start"));
                close_pt(&seg[1], &ticks[2 * k + 1], 1e-3, &what("tick end"));
            }
            for (a, b) in dg.ticklabel_pos.iter().zip(pts(&want["ticklabel_pos"])) {
                close_pt(a, &b, 1e-3, &what("tick label"));
            }
            assert_eq!(dg.ticklabel_align, align(&want["ticklabel_align"]), "{}", what("tick label align"));
            close_pt(&dg.label_pos, &vec_f(&want["label_pos"]), 1e-3, &what("label position"));
            let dr = (dg.label_rot - f(&want["label_rot"])).rem_euclid(2.0 * PI);
            assert!(
                dr < 1e-5 || 2.0 * PI - dr < 1e-5,
                "{}: {} vs {}",
                what("label rotation"),
                dg.label_rot,
                want["label_rot"]
            );
            assert_eq!(dg.label_align, align(&want["label_align"]), "{}", what("label align"));
            let (g1, g2, frame) = &g.lines3d[d];
            for (mine, key) in [(g1, "grid1"), (g2, "grid2"), (frame, "frame")] {
                let want = pts(&want[key]);
                assert_eq!(mine.len(), want.len(), "{}", what(key));
                for (a, b) in mine.iter().zip(&want) {
                    close_pt(a, b, 1e-9, &what(key));
                }
            }
        }
        close_pt(&g.title, &vec_f(&case["title_pos"]), 1e-9, &format!("{name} title"));
    }
}

#[test]
fn autolimits_and_interaction() {
    let fig = Figure::new();
    let ax = Axis3::new(fig.at(1, 1));
    // Makie: (0, 1) on every axis without data.
    assert_eq!(ax.current_limits().unwrap(), [0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
    ax.lines([0.0, 10.0], [-1.0, 1.0], [5.0, 5.0]);
    let l = ax.current_limits().unwrap();
    // 5 % margins; a degenerate z range widens by ±|z|.
    assert_eq!([l[0], l[1]], [-0.5, 10.5]);
    assert_eq!([l[2], l[3]], [-1.1, 1.1]);
    assert_eq!([l[4], l[5]], [0.0, 10.0]);
    ax.zlims(-1.0, None);
    assert_eq!(ax.current_limits().unwrap()[4..], [-1.0, 10.0]);

    // ScrollZoom: limits shrink about their centre by 0.95 per step.
    let before = ax.current_limits().unwrap();
    ax.zoom_by(1.0);
    let after = ax.current_limits().unwrap();
    close(after[1] - after[0], 0.95 * (before[1] - before[0]), 1e-12, "zoomed width");
    close(after[0] + after[1], before[0] + before[1], 1e-12, "zoom centre");
    ax.reset_view();
    assert_eq!(ax.current_limits().unwrap(), before);

    // DragRotate: 0.01 rad per unit; the elevation stops short of the poles.
    let (az, el) = ezviz_drag(1.275 * PI, PI / 8.0, 10.0, -1000.0);
    close(az, 1.275 * PI - 0.1, 1e-12, "azimuth");
    close(el, -(PI / 2.0 - 0.001), 1e-12, "elevation");
}

fn ezviz_drag(az: f64, el: f64, dx: f64, dy: f64) -> (f64, f64) {
    let fig = Figure::new();
    let ax = Axis3::new(fig.at(1, 1)).azimuth(az).elevation(el);
    ax.rotate_by(dx, dy);
    let g = ax.geometry().unwrap();
    // Recover the angles from the eye position.
    let e = g.eyeposition;
    let r = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    let az2 = e[1].atan2(e[0]).rem_euclid(2.0 * PI);
    (az2 + if az2 < az - PI { 2.0 * PI } else { 0.0 }, (e[2] / r).asin())
}

/// A red line behind a surface is hidden on the GPU, the same line in front is drawn; the SVG
/// backend orders them by depth.
#[test]
fn surfaces_hide_what_is_behind_them() {
    let (n, m) = (10, 10);
    let x = linspace(-1.0, 1.0, n);
    let y = linspace(-1.0, 1.0, m);
    let z = vec![0.0; n * m];
    let fig = Figure::new().size((300, 300));
    let ax = Axis3!(fig.at(1, 1); elevation = PI / 2.0 - 0.01, azimuth = -PI / 2.0);
    ax.hidedecorations();
    ax.surface(&x, &y, Field::new(&z, n, m)).color(BLUE).shading(false);
    // Below the plane at x = -0.5, above it at x = 0.5 (seen from above).
    ax.lines([-0.5, -0.5], [-0.8, 0.8], [-0.5, -0.5]).color(RED).linewidth(6.0);
    ax.lines([0.5, 0.5], [-0.8, 0.8], [0.5, 0.5]).color(RED).linewidth(6.0);
    ax.limits(-1.0, 1.0, -1.0, 1.0, -1.0, 1.0);
    let img = match fig.render_rgba(&Save::new().px_per_unit(1.0)) {
        Ok(img) => img,
        Err(Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    let geo = ax.geometry().unwrap();
    let px = |p: [f64; 3]| {
        let q = geo.project(p);
        let (x, y) = (q[0].round() as usize, (300.0 - q[1]).round() as usize);
        let i = (y * img.width as usize + x) * 4;
        [img.data[i], img.data[i + 1], img.data[i + 2]]
    };
    let front = px([0.5, 0.0, 0.5]);
    let back = px([-0.5, 0.0, -0.5]);
    assert!(front[0] > 200 && front[2] < 60, "the line above the surface is drawn: {front:?}");
    assert!(back[2] > 200 && back[0] < 60, "the line below the surface is hidden: {back:?}");

    // The SVG backend: the line below comes before the surface, the one above after it.
    let svg = fig.to_svg_string(&Save::new()).unwrap();
    let red: Vec<usize> = svg.match_indices("#ff0000").map(|(i, _)| i).collect();
    let blue = svg.find("#0072b2").or_else(|| svg.find("#0000ff")).expect("surface in the SVG");
    assert_eq!(red.len(), 2, "two red lines");
    assert!(red[0] < blue && red[1] > blue, "painter's order");
}

/// Every 3D pipeline renders under WebGL2 limits without validation errors, like natively.
#[test]
fn webgl2_limits() {
    let (native, webgl) = match (GpuContext::native(), GpuContext::webgl2()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return,
    };
    let fig = Figure::new().size((400, 300));
    let ax = Axis3::new(fig.at(1, 1));
    let t = linspace(0.0, 12.0, 300);
    ax.lines(t.iter().map(|t| t.cos()), t.iter().map(|t| t.sin()), &t).color(&t);
    ax.scatter(t.iter().map(|t| 0.5 * t.cos()), t.iter().map(|t| 0.5 * t.sin()), &t).marker(Marker::Rect);
    let (n, m) = (12, 9);
    let x = linspace(-1.0, 1.0, n);
    let y = linspace(-1.0, 1.0, m);
    let z: Vec<f64> = (0..n * m).map(|k| 6.0 + 3.0 * x[k % n] * y[k / n]).collect();
    ax.surface(&x, &y, Field::new(&z, n, m));
    let a = native.render(&fig, 1.0, false).unwrap();
    let b = webgl.render(&fig, 1.0, true).unwrap();
    assert_eq!(native.validation_errors() + webgl.validation_errors(), 0);
    let diff: u64 = a.data.iter().zip(&b.data).map(|(p, q)| (*p as i64 - *q as i64).unsigned_abs()).sum();
    assert!((diff as f64 / a.data.len() as f64) < 0.5, "native and WebGL2-limited renders agree");
}

/// Live lines: `push` appends, limits follow, and the draw list keeps working.
#[test]
fn live_push() {
    let fig = Figure::new();
    let ax = Axis3::new(fig.at(1, 1));
    let l = ax.lines(Vec::<f64>::new(), Vec::<f64>::new(), Vec::<f64>::new());
    for i in 0..5000 {
        let t = i as f64 * 0.01;
        l.push(t.cos(), t.sin(), t);
    }
    assert_eq!(l.len(), 5000);
    let lim = ax.current_limits().unwrap();
    close(lim[5], 49.99 + 0.05 * 49.99, 1e-9, "z max follows the data");
    let _ = fig.to_svg_string(&Save::new()).unwrap();
}
