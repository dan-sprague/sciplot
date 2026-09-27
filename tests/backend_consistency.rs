//! The CPU rasterizer (SVG through resvg) and the GPU renderer must agree: mean absolute
//! difference below 1.5 % per channel. Skipped when there is no GPU adapter.
//! Both images and an amplified difference are written to `out/` for inspection.

use sciplot::prelude::*;
use sciplot::{Error, RgbaImage};
use std::path::PathBuf;

fn rng(mut s: u64) -> impl FnMut() -> f64 {
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn has_gpu() -> bool {
    match sciplot::testing::Offscreen::new(1.0) {
        Ok(_) => true,
        Err(Error::NoGpuAdapter(_)) => false,
        Err(e) => panic!("{e}"),
    }
}

fn save_png(img: &RgbaImage, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(name)).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img.data).unwrap();
}

/// Mean absolute difference per RGBA channel, as a fraction of 255.
fn mean_abs_diff(a: &RgbaImage, b: &RgbaImage) -> [f64; 4] {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut sum = [0u64; 4];
    for (p, q) in a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0) {
        for c in 0..4 {
            sum[c] += p[c].abs_diff(q[c]) as u64;
        }
    }
    let n = (a.width * a.height) as f64 * 255.0;
    sum.map(|s| s as f64 / n)
}

fn compare(name: &str, fig: &Figure) {
    if !has_gpu() {
        eprintln!("no GPU adapter; skipping {name}");
        return;
    }
    let gpu = fig.render_rgba(&Save::new()).unwrap();
    let cpu = fig.render_rgba(&Save::new().cpu(true)).unwrap();
    save_png(&gpu, &format!("{name}_gpu.png"));
    save_png(&cpu, &format!("{name}_cpu.png"));
    let diff = RgbaImage {
        width: gpu.width,
        height: gpu.height,
        data: gpu
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(cpu.data.as_chunks::<4>().0)
            .flat_map(|(p, q)| {
                let d = (0..3).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
                let v = 255 - d.saturating_mul(4);
                [v, v, v, 255]
            })
            .collect(),
    };
    save_png(&diff, &format!("{name}_diff.png"));
    let d = mean_abs_diff(&gpu, &cpu);
    eprintln!("{name}: mean abs diff per channel {d:?}");
    assert!(d.iter().all(|v| *v < 0.015), "{name}: CPU and GPU renders differ: {d:?}");
}

#[test]
fn scatter_cpu_matches_gpu() {
    let mut r = rng(0x9E3779B97F4A7C15);
    let fig = Figure::new();
    let ax = Axis!(fig.at(1, 1); title = "backend consistency", xlabel = "x", ylabel = "y");
    let x: Vec<f64> = (0..300).map(|_| r()).collect();
    let y: Vec<f64> = x.iter().map(|x| x * x + 0.2 * r()).collect();
    scatter!(ax, &x, &y; markersize = 10);
    scatter!(ax, [0.2, 0.5, 0.8], [0.8, 0.9, 0.8]; marker = Marker::Star5, markersize = 30, strokewidth = 1.5, strokecolor = BLACK);
    scatter!(ax, [0.3, 0.6], [0.6, 0.6]; marker = Marker::Rect, markersize = 25, color = (WONG[2], 0.5));
    compare("consistency_scatter", &fig);
}

#[test]
fn forced_cpu_png_keeps_size_and_phys() {
    let fig = scatter([1.0, 2.0, 3.0], [1.0, 4.0, 9.0]).figure();
    let img = fig.render_rgba(&Save::new().cpu(true).px_per_unit(1.5)).unwrap();
    assert_eq!((img.width, img.height), (900, 675));
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("consistency_forced_cpu.png");
    fig.save_with(&path, Save::dpi(300).cpu(true)).unwrap();
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).read_info().unwrap();
    let info = dec.info();
    assert_eq!((info.width, info.height), (1875, 1406));
    let phys = info.pixel_dims.unwrap();
    assert_eq!(phys.xppu, 11811);
}

#[test]
fn styled_scatter_cpu_matches_gpu() {
    let fig = Figure::new();
    let a = Axis!(fig.at(1, 1); title = "per point");
    let b = Axis!(fig.at(1, 2); title = "reversed", yreversed = true);
    let x: Vec<f64> = (0..12).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|v| v * v / 10.0).collect();
    let colors: Vec<Color> = (0..12).map(|i| WONG[i % WONG.len()]).collect();
    scatter!(a, &x, &y; color = colors, markersize = 14, marker = Marker::Diamond);
    scatter!(b, &x, &y; color = (WONG[5], 0.5), markersize = 20, strokewidth = 2, strokecolor = BLACK);
    compare("consistency_styled", &fig);
}
