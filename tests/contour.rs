//! `contourf` renders without seams: on a saddle-rich random field with many bands (row-merged
//! quads, T-junctions along grid lines, band boundaries), no background shows through, neither on
//! the GPU (MSAA) nor through the SVG backend rasterized on the CPU. `contour` draws closed loops.

use ezviz::prelude::*;

fn random_field(nx: usize, ny: usize) -> Vec<f64> {
    let mut s = 0x2545_f491_4f6c_dd1du64;
    (0..nx * ny)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 33) % 1000) as f64 / 1000.0
        })
        .collect()
}

/// A contourf whose bands are all dark (red channel 0) on a white figure, so any seam lets red
/// through.
fn seam_figure(alpha: f64) -> Figure {
    let (nx, ny) = (40, 30);
    let z = random_field(nx, ny);
    let fig = Figure!(size = (400, 300), backgroundcolor = WHITE);
    let ax = Axis!(fig.at(1, 1); xgridvisible = false, ygridvisible = false);
    let dark = Colormap::from_colors(&[Color::rgb(0.0, 0.0, 0.35), Color::rgb(0.0, 0.35, 0.1)]);
    contourf!(ax, Field::new(&z, nx, ny); levels = 12, colormap = dark, alpha = alpha);
    fig
}

/// The plot area: the bounding box of the dark-but-colored pixels, shrunk by `inset` pixels.
fn plot_area(img: &ezviz::RgbaImage, inset: u32) -> [u32; 4] {
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    for (k, p) in img.data.as_chunks::<4>().0.iter().enumerate() {
        if p[0] < 20 && (p[1] > 30 || p[2] > 30) {
            let (x, y) = (k as u32 % img.width, k as u32 / img.width);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    assert!(b[2] > b[0] + 2 * inset && b[3] > b[1] + 2 * inset, "no plot found: {b:?}");
    [b[0] + inset, b[1] + inset, b[2] - inset, b[3] - inset]
}

/// Largest red-channel value inside `area`.
fn max_red(img: &ezviz::RgbaImage, area: [u32; 4]) -> u8 {
    let mut m = 0;
    for y in area[1]..=area[3] {
        for x in area[0]..=area[2] {
            m = m.max(img.data[((y * img.width + x) * 4) as usize]);
        }
    }
    m
}

fn render(fig: &Figure, opts: &Save) -> Option<ezviz::RgbaImage> {
    match fig.render_rgba(opts) {
        Ok(img) => Some(img),
        Err(ezviz::Error::NoGpuAdapter(_)) => None,
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn opaque_bands_have_no_seams_on_gpu_and_cpu() {
    let fig = seam_figure(1.0);
    for (what, opts) in [("gpu", Save::new().px_per_unit(2)), ("cpu", Save::new().px_per_unit(2).cpu(true))] {
        let Some(img) = render(&fig, &opts) else { continue };
        let area = plot_area(&img, 3);
        let red = max_red(&img, area);
        assert!(red <= 2, "{what}: background shows through (red = {red}) in {area:?}");
    }
}

/// Translucent bands partition the grid exactly: under MSAA every pixel is covered once.
#[test]
fn translucent_bands_have_no_seams_or_overlaps_on_gpu() {
    let fig = seam_figure(0.5);
    let Some(img) = render(&fig, &Save::new().px_per_unit(2)) else { return };
    // Half-transparent dark bands over white: red is 255 · 0.5 everywhere (±1 for rounding);
    // a seam is brighter, an overlap darker.
    let area = plot_area_translucent(&img);
    let (mut lo, mut hi) = (255, 0);
    for y in area[1]..=area[3] {
        for x in area[0]..=area[2] {
            let r = img.data[((y * img.width + x) * 4) as usize];
            (lo, hi) = (lo.min(r), hi.max(r));
        }
    }
    assert!(lo >= 126 && hi <= 129, "red channel varies {lo}..={hi} in {area:?}");
}

/// The plot area for the translucent figure: pixels with red near 128 and a colored tint.
fn plot_area_translucent(img: &ezviz::RgbaImage) -> [u32; 4] {
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    for (k, p) in img.data.as_chunks::<4>().0.iter().enumerate() {
        if (120..=135).contains(&p[0]) && p[1].max(p[2]) > p[0] + 10 {
            let (x, y) = (k as u32 % img.width, k as u32 / img.width);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    assert!(b[2] > b[0] + 8 && b[3] > b[1] + 8, "no plot found: {b:?}");
    [b[0] + 4, b[1] + 4, b[2] - 4, b[3] - 4]
}

/// Levels, labels and set_data through the public API; SVG output has one path per band color.
#[test]
fn contour_api_and_svg() {
    let n = 50;
    let xs = linspace(-1.0, 1.0, n);
    let z: Vec<f64> = (0..n * n).map(|k| xs[k % n].powi(2) + xs[k / n].powi(2)).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    let cf = contourf!(ax, &xs, &xs, Field::new(&z, n, n); levels = 5);
    let c = contour!(ax, &xs, &xs, Field::new(&z, n, n); levels = 5, color = BLACK, labels = true);
    assert_eq!(c.resolved_levels().len(), 5);
    assert_eq!(cf.resolved_levels().len(), 6);
    let svg = fig.to_svg_string(&Save::new()).unwrap();
    // Each of the 5 opaque band colors is one fill group (one merged path).
    let viridis_first = "#440154";
    assert_eq!(svg.matches(viridis_first).count(), 1, "one path for the first band");
    let z2: Vec<f64> = z.iter().map(|v| -v).collect();
    c.set_data(Field::new(&z2, n, n));
    cf.set_data(Field::new(&z2, n, n));
    assert!(c.resolved_levels().iter().all(|l| *l < 0.0));
    fig.to_svg_string(&Save::new()).unwrap();
}
