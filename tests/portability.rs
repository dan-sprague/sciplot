//! WebGPU / WebGL2 portability of the GPU backend, checked natively:
//! - every pipeline is created and every figure renders on a device limited to
//!   `wgpu::Limits::downlevel_webgl2_defaults()` (no storage buffers, 8 vertex buffers, 2048 px
//!   textures...) without validation errors, and the result matches the full-limits render;
//! - BGRA and RGBA targets give the same image;
//! - every shader translates to GLSL ES 3.00 for WebGL2 (naga's writer, as wgpu's GL backend
//!   does in the browser).
//!
//! GPU parts are skipped when there is no adapter. Set `SCIPLOT_TEST_DUMP=1` to write the renders
//! to `out/portability_*.png`.

use sciplot::gpu_testing::{GpuContext, wgsl_sources};
use sciplot::prelude::*;
use sciplot::{Error, RgbaImage};

fn rng(mut s: u64) -> impl FnMut() -> f64 {
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn markers() -> Figure {
    let fig = Figure!(size = (640, 420));
    let ax = Axis!(fig.at(1, 1); title = "every marker, rotated, stroked");
    let all = [
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
    for (k, m) in all.into_iter().enumerate() {
        let x = k as f64;
        scatter!(ax, [x], [0.0]; marker = m, markersize = 22, color = WONG[k % 7]);
        scatter!(ax, [x], [1.0]; marker = m, markersize = 22, rotation = 0.4, color = (WONG[k % 7], 0.6));
        scatter!(ax, [x], [2.0]; marker = m, markersize = 18, strokewidth = 2, strokecolor = BLACK, color = "white");
    }
    let per_point: Vec<Color> = (0..14).map(|k| WONG[k % 7]).collect();
    scatter!(ax, (0..14).map(|k| k as f64), [3.0; 14]; color = per_point, marker = Marker::Star5, markersize = 16);
    fig
}

fn values() -> Figure {
    let fig = Figure!(size = (500, 400));
    let ax = Axis!(fig.at(1, 1); title = "value-colored scatter");
    let mut r = rng(7);
    let n = 400;
    let x: Vec<f64> = (0..n).map(|_| r()).collect();
    let y: Vec<f64> = (0..n).map(|_| r()).collect();
    let mut v: Vec<f64> = x.iter().zip(&y).map(|(x, y)| (6.0 * x).sin() + y).collect();
    v[3] = f64::NAN;
    scatter!(ax, &x, &y; color = &v, colormap = Colormap::MAGMA, colorrange = (-0.5, 1.5), markersize = 12, highclip = "red");
    fig
}

fn lines() -> Figure {
    let fig = Figure!(size = (800, 600));
    let a = Axis!(fig.at(1, 1); title = "joins, caps, duplicates, loops");
    let half = (BLACK, 0.5);
    for (k, join) in [JoinStyle::Miter, JoinStyle::Bevel, JoinStyle::Round].into_iter().enumerate() {
        let y = 1.0 + 2.5 * k as f64;
        lines!(a, [0.5, 1.5, 2.5, 3.0, 4.5], [y, y + 1.5, y, y + 1.8, y]; linewidth = 10, joinstyle = join, color = (WONG[k], 0.5));
    }
    lines!(a, [5.0, 6.5, 6.5, 6.5, 8.0], [1.0, 3.0, 3.0, 3.0, 1.0]; linewidth = 10, color = half);
    lines!(a, [5.0, 6.0, f64::NAN, 7.0, 8.0], [4.0, 5.0, f64::NAN, 4.0, 5.0]; linewidth = 8, linecap = LineCap::Round, color = half);
    lines!(a, [5.5, 5.5, 7.5, 7.5, 5.5], [6.0, 8.0, 8.0, 6.0, 6.0]; linewidth = 8, color = (WONG[4], 0.5));
    lines!(a, [8.5, 8.5, 9.5, 9.5, 8.5, 8.5], [6.0, 8.0, 8.0, 6.0, 6.0, 6.0]; linewidth = 8, color = (WONG[5], 0.5));
    a.limits(0.0, 10.0, 0.0, 9.0);

    let b = Axis!(fig.at(1, 2); title = "dashes and colors");
    let x = linspace(0.0, 10.0, 120);
    let styles =
        [Linestyle::Dash, Linestyle::Dot, Linestyle::DashDot, Linestyle::Custom(vec![0.0, 4.0, 5.0, 6.0, 7.0])];
    for (k, s) in styles.into_iter().enumerate() {
        let y0 = 1.0 + 1.5 * k as f64;
        lines!(b, &x, x.iter().map(|x| y0 + 0.4 * (2.0 * x).sin()); linestyle = s, linewidth = 3, color = BLACK);
    }
    let rainbow: Vec<Color> = (0..120).map(|i| WONG[i * 7 / 120]).collect();
    lines!(b, &x, x.iter().map(|x| 7.5 + 0.4 * x.cos()); color = rainbow, linewidth = 6);
    let v: Vec<f64> = x.iter().map(|x| x.cos()).collect();
    lines!(b, &x, x.iter().map(|x| 9.0 + 0.4 * x.sin()); color = &v, linewidth = 6);
    scatterlines!(b, [1.0, 3.0, 5.0, 7.0, 9.0], [0.2, 0.6, 0.1, 0.5, 0.3]; markersize = 12, linewidth = 2);
    fig
}

fn heatmaps() -> Figure {
    let fig = Figure!(size = (900, 700));
    let (nx, ny) = (60, 40);
    let z: Vec<f64> = (0..nx * ny).map(|k| ((k % nx) as f64 * 0.2).sin() * ((k / nx) as f64 * 0.15).cos()).collect();
    let a = Axis!(fig.at(1, 1); title = "regular");
    heatmap!(a, Field::new(&z, nx, ny); colormap = Colormap::MAGMA);
    let b = Axis!(fig.at(1, 2); title = "irregular, NaN");
    let xe = [0.0, 1.0, 1.5, 3.0, 3.2, 5.0];
    let ye = [0.0, 0.5, 2.0, 2.2, 4.0];
    let mut w: Vec<f64> = (0..20).map(|k| (k % 5) as f64 + 0.5 * (k / 5) as f64).collect();
    w[7] = f64::NAN;
    b.heatmap_xy(xe, ye, Field::new(&w, 5, 4)).colorrange((1.0, 4.0)).lowclip("cyan").highclip("red");
    let c = Axis!(fig.at(2, 1); title = "interpolate");
    let (mx, my) = (8, 6);
    let v: Vec<f64> = (0..mx * my).map(|k| ((k % mx) as f64 * 0.8).sin() * ((k / mx) as f64 * 0.9).cos()).collect();
    heatmap!(c, Field::new(&v, mx, my); interpolate = true, colormap = "RdBu");
    // Wider than WebGL2's 2048 px textures: drawn in tiles there, in one piece natively.
    let d = Axis!(fig.at(2, 2); title = "3000 × 3 cells, irregular x");
    let (lx, ly) = (3000, 3);
    let big: Vec<f64> = (0..lx * ly).map(|k| ((k % lx) as f64 * 0.01).sin() + (k / lx) as f64).collect();
    let bx: Vec<f64> = (0..=lx).map(|i| (i as f64 / lx as f64).powf(1.5)).collect();
    d.heatmap_xy(&bx, Edges(0.0, 1.0), Field::new(&big, lx, ly)).interpolate(true);
    fig
}

fn text_and_stats() -> Figure {
    let mut u = rng(11);
    let samples: Vec<f64> =
        (0..4000).map(|_| (-2.0 * u().max(1e-12).ln()).sqrt() * (std::f64::consts::TAU * u()).cos()).collect();
    let fig = Figure!(size = (900, 600));
    let a1 =
        Axis!(fig.at(1, 1); title = rich!("E = mc", superscript("2")), xlabel = tex(r"\alpha (\mu m)"), ylabel = "pdf");
    hist!(a1, &samples; bins = 30, normalization = Normalization::Pdf);
    let a2 = Axis!(fig.at(1, 2); title = "bars");
    barplot!(a2, [1, 1, 2, 2, 3, 3], [1.0, 2.0, 2.0, 1.5, 3.0, 2.5]).dodge([1, 2, 1, 2, 1, 2]);
    let a3 = Axis!(fig.at(2, 1..=2); title = "band and text");
    let t = linspace(0.0, 10.0, 100);
    let m: Vec<f64> = t.iter().map(|t| (0.6 * t).sin()).collect();
    band!(a3, &t, m.iter().map(|m| m - 0.3), m.iter().map(|m| m + 0.3); alpha = 0.5);
    a3.text([1.0, 5.0, 8.0], [0.0, 0.5, -0.5], ["upright", "plain", "text"]).fontsize(18);
    a3.text([3.0], [-0.8], ["slanted 0.3 rad"]).rotation(0.3);
    a3.text([6.0], [-1.0], ["quarter turn"]).rotation(std::f64::consts::FRAC_PI_2);
    fig
}

/// Fraction of pixels whose channels all differ by at most 1, and the largest difference.
fn close_fraction(a: &RgbaImage, b: &RgbaImage) -> (f64, u8) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut ok = 0usize;
    let mut worst = 0u8;
    for (p, q) in a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0.iter()) {
        let d = (0..4).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
        worst = worst.max(d);
        ok += (d <= 1) as usize;
    }
    (ok as f64 / (a.width * a.height) as f64, worst)
}

fn dump(img: &RgbaImage, name: &str) {
    if std::env::var_os("SCIPLOT_TEST_DUMP").is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(format!("portability_{name}.png"))).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img.data).unwrap();
}

#[test]
fn webgl2_limits_render_like_full_limits() {
    let full = match GpuContext::native() {
        Ok(c) => c,
        Err(Error::NoGpuAdapter(e)) => return eprintln!("no GPU adapter ({e}); skipping"),
        Err(e) => panic!("{e}"),
    };
    let gl = GpuContext::webgl2().expect("a device with WebGL2 limits");
    let (tex, vbufs, storage) = gl.limits();
    assert_eq!((tex, vbufs, storage), (2048, 8, 0), "the WebGL2 device really is limited");

    let figures = [
        ("markers", markers()),
        ("values", values()),
        ("lines", lines()),
        ("heatmaps", heatmaps()),
        ("text_stats", text_and_stats()),
    ];
    for (name, fig) in &figures {
        let a = full.render(fig, 2.0, false).unwrap();
        let b = gl.render(fig, 2.0, false).unwrap();
        let c = full.render(fig, 1.0, true).unwrap();
        let d = gl.render(fig, 1.0, false).unwrap();
        dump(&a, name);
        dump(&b, &format!("{name}_webgl2"));
        let (frac, worst) = close_fraction(&a, &b);
        eprintln!("{name}: {:.5} of pixels within 1/255 (max diff {worst})", frac);
        assert!(frac > 0.999, "{name}: WebGL2-limited render differs ({frac}, max {worst})");
        assert_eq!(c.data, d.data, "{name}: BGRA and RGBA targets differ");
        // Something was drawn beyond the background.
        assert!(a.data.as_chunks::<4>().0.iter().any(|p| p[0] < 100), "{name}: nothing drawn");
    }
    assert_eq!(full.validation_errors(), 0, "wgpu validation errors with full limits");
    assert_eq!(gl.validation_errors(), 0, "wgpu validation errors with WebGL2 limits");
}

/// Rotated markers turn counter-clockwise on the GPU as in the SVG backend (and CairoMakie).
#[cfg(feature = "cpu-png")]
#[test]
fn marker_rotation_matches_svg_backend() {
    let fig = Figure!(size = (200, 200));
    let ax = Axis::new(fig.at(1, 1)).hidedecorations(true).hidespines();
    scatter!(ax, [0.0], [0.0]; marker = Marker::UTriangle, markersize = 160, rotation = 0.6, color = BLACK);
    ax.limits(-1.0, 1.0, -1.0, 1.0);
    let gpu = match fig.render_rgba(&Save::new().px_per_unit(1)) {
        Ok(img) => img,
        Err(Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    let cpu = fig.render_rgba(&Save::new().px_per_unit(1).cpu(true)).unwrap();
    let ink = |img: &RgbaImage| img.data.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count();
    let off = gpu
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(cpu.data.as_chunks::<4>().0.iter())
        .filter(|(p, q)| p[0].abs_diff(q[0]) > 128)
        .count();
    // A clockwise turn would leave ~20% of the triangle's area mismatched.
    assert!(off * 100 < ink(&cpu), "GPU and SVG rotations disagree ({off} of {} px)", ink(&cpu));
}

#[test]
fn shaders_translate_to_webgl2_glsl() {
    use naga::back::glsl;
    for (name, src) in wgsl_sources() {
        let module =
            naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
        assert!(
            module.global_variables.iter().all(|(_, v)| !matches!(v.space, naga::AddressSpace::Storage { .. })),
            "{name}: storage buffers are not available in WebGL2"
        );
        for ep in &module.entry_points {
            let options = glsl::Options {
                version: glsl::Version::Embedded { version: 300, is_webgl: true },
                writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE | glsl::WriterFlags::FORCE_POINT_SIZE,
                binding_map: Default::default(),
                zero_initialize_workgroup_memory: true,
            };
            let pipeline =
                glsl::PipelineOptions { shader_stage: ep.stage, entry_point: ep.name.clone(), multiview: None };
            let mut out = String::new();
            glsl::Writer::new(&mut out, &module, &info, &options, &pipeline, Default::default())
                .and_then(|mut w| w.write().map(|_| ()))
                .unwrap_or_else(|e| panic!("{name}::{}: GLSL ES 3.00 translation failed: {e}", ep.name));
            assert!(out.starts_with("#version 300 es"), "{name}::{}", ep.name);
        }
    }
}

/// Compares `out/*.png` with `out/baseline/*.png` (renders before a renderer change): prints the
/// largest and mean channel difference per file. Run with `--ignored` after re-rendering.
#[test]
#[ignore]
fn diff_against_baseline() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let read = |p: &std::path::Path| -> Option<(u32, u32, Vec<u8>)> {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).ok()?));
        let mut r = dec.read_info().ok()?;
        let mut buf = vec![0; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        Some((info.width, info.height, buf))
    };
    let mut names: Vec<_> =
        std::fs::read_dir(root.join("baseline")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    names.sort();
    for base in names {
        let Some(file) = base.file_name() else { continue };
        let (Some(a), Some(b)) = (read(&base), read(&root.join(file))) else { continue };
        if (a.0, a.1) != (b.0, b.1) {
            eprintln!("{file:?}: size changed {}x{} -> {}x{}", a.0, a.1, b.0, b.1);
            continue;
        }
        let (mut max, mut sum, mut changed) = (0u8, 0u64, 0usize);
        let mut bbox = [u32::MAX, u32::MAX, 0, 0];
        for (i, (p, q)) in a.2.as_chunks::<4>().0.iter().zip(b.2.as_chunks::<4>().0.iter()).enumerate() {
            let d = (0..4).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
            max = max.max(d);
            changed += (d > 0) as usize;
            sum += (0..4).map(|c| p[c].abs_diff(q[c]) as u64).sum::<u64>();
            if d > 1 {
                let (x, y) = (i as u32 % a.0, i as u32 / a.0);
                bbox = [bbox[0].min(x), bbox[1].min(y), bbox[2].max(x), bbox[3].max(y)];
            }
        }
        let n = (a.0 * a.1) as usize;
        eprintln!(
            "{file:?}: max diff {max}, mean {:.5}/255 per channel, {changed} of {n} pixels differ{}",
            sum as f64 / (4 * n) as f64,
            if max > 1 {
                format!(" (diffs > 1 within x {}..={}, y {}..={})", bbox[0], bbox[2], bbox[1], bbox[3])
            } else {
                String::new()
            }
        );
    }
}
