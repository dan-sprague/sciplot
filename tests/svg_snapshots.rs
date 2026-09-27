//! Byte-for-byte SVG snapshots of whole figures (`tests/snapshots/<case>.svg`).
//!
//! `SCIPLOT_BLESS=1 cargo test --test svg_snapshots` rewrites the snapshots. A mismatch writes the
//! new output to `out/snapshots/<case>.svg` for diffing. Documents are also checked with
//! `xmllint --noout` when it is installed. Data is seeded (xorshift), so output is OS-independent.

use sciplot::prelude::*;
use std::path::{Path, PathBuf};

/// Deterministic uniform numbers in [0, 1).
fn rng(mut s: u64) -> impl FnMut() -> f64 {
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn scatter_default() -> Figure {
    let mut r = rng(0x2545F4914F6CDD1D);
    let x: Vec<f64> = (0..100).map(|_| r()).collect();
    let y: Vec<f64> = (0..100).map(|_| r()).collect();
    scatter(&x, &y).figure()
}

fn scatter_styled() -> Figure {
    let fig = Figure!(size = (500, 400));
    let ax = Axis!(fig.at(1, 1); title = "markers", xlabel = "x", ylabel = "y");
    let markers = [
        Marker::Circle,
        Marker::Rect,
        Marker::Diamond,
        Marker::Cross,
        Marker::XCross,
        Marker::UTriangle,
        Marker::DTriangle,
        Marker::LTriangle,
        Marker::RTriangle,
        Marker::Pentagon,
        Marker::Hexagon,
        Marker::Star5,
        Marker::FullCircle,
        Marker::FullRect,
    ];
    for (i, m) in markers.into_iter().enumerate() {
        let (x, y) = ((i % 5) as f64, (i / 5) as f64);
        scatter!(ax, [x], [y]; marker = m, markersize = 24, strokewidth = (i % 2) as f64 * 1.5, strokecolor = BLACK);
    }
    fig
}

fn scatter_colors() -> Figure {
    let fig = Figure::new();
    let a = Axis!(fig.at(1, 1); title = "per point");
    let b = Axis!(fig.at(1, 2); title = "alpha", yreversed = true);
    let n = 12;
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|v| v * v / 10.0).collect();
    let colors: Vec<Color> = (0..n).map(|i| WONG[i % WONG.len()]).collect();
    scatter!(a, &x, &y; color = colors, markersize = 14);
    scatter!(b, &x, &y; color = (WONG[5], 0.5), markersize = 20, strokewidth = 2, strokecolor = (BLACK, 0.5));
    fig
}

fn repo_path(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir).join(format!("{name}.svg"))
}

fn check(name: &str, fig: Figure) {
    let svg = fig.to_svg_string(&Save::new()).unwrap();
    assert_eq!(svg, fig.to_svg_string(&Save::new()).unwrap(), "{name}: output is not deterministic");
    let path = repo_path("tests/snapshots", name);
    if std::env::var_os("SCIPLOT_BLESS").is_some_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &svg).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {}; run with SCIPLOT_BLESS=1", path.display()));
    if want != svg {
        let new = repo_path("out/snapshots", name);
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, &svg).unwrap();
        panic!("SVG snapshot {name} changed; new output in {}; rerun with SCIPLOT_BLESS=1 if intended", new.display());
    }
    xmllint(&path);
}

/// Validates with `xmllint --noout` when it is installed.
fn xmllint(path: &Path) {
    if let Ok(out) = std::process::Command::new("xmllint").arg("--noout").arg(path).output() {
        assert!(out.status.success(), "xmllint {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    }
}

/// One `#[test]` per case: `name => figure expression;`.
macro_rules! snapshots {
    ($($name:ident => $fig:expr;)*) => {
        $(
            #[test]
            fn $name() {
                check(stringify!($name), $fig);
            }
        )*
    };
}

snapshots! {
    scatter_default_svg => scatter_default();
    scatter_styled_svg => scatter_styled();
    scatter_colors_svg => scatter_colors();
}

#[test]
fn svg_size_follows_pt_per_unit() {
    let fig = Figure!(size = (384, 288));
    Axis::new(fig.at(1, 1)).scatter([1.0, 2.0], [3.0, 4.0]);
    let svg = fig.to_svg_string(&Save::new()).unwrap();
    assert!(svg.contains("width=\"288pt\" height=\"216pt\" viewBox=\"0 0 384 288\""));
    let svg = fig.to_svg_string(&Save::new().pt_per_unit(1)).unwrap();
    assert!(svg.contains("width=\"384pt\" height=\"288pt\""));
    let svg = fig.to_svg_string(&Save::new().backgroundcolor(Color::TRANSPARENT)).unwrap();
    assert!(!svg.contains("<rect width=\"384\""));
}

#[test]
fn save_svg_file() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("svg_snapshots_save.svg");
    scatter_default().save(&path).unwrap();
    let s = std::fs::read_to_string(&path).unwrap();
    assert!(s.starts_with("<?xml") && s.ends_with("</svg>\n"));
    xmllint(&path);
}
