//! `arrows` and `streamplot`: Makie's seeding, rendering on every backend, colorbars, legends and
//! GPU buffer reuse. `examples/vectorfield_check.rs` + `tools/vectorfield_check.jl` compare the
//! pictures with CairoMakie.

use sciplot::prelude::*;

fn pendulum(th: f64, om: f64) -> (f64, f64) {
    (om, -th.sin() - 0.2 * om)
}

/// A quiver + streamplot figure with a colorbar and a legend.
fn figure() -> (Figure, Arrows, StreamPlot) {
    let fig = Figure!(size = (500, 300));
    let ax = Axis::new(fig.at(1, 1));
    let g = linspace(-3.0, 3.0, 12);
    let ar = arrows!(ax, &g, &g, pendulum; color = Magnitude, normalize = true, lengthscale = 0.3, label = "field");
    let sp = streamplot!(ax, pendulum, -3.0..=3.0, (-3, 3); color = RED, linewidth = 1, label = "streamlines");
    Colorbar::new(fig.at(1, 2), &ar);
    axislegend(&ax);
    (fig, ar, sp)
}

/// Makie 0.24's `streamplot_impl` seeds 177 streamlines for this field and box (and 48 for the
/// Van der Pol settings of the check example); see `tools/vectorfield_check.jl`.
#[test]
fn streamplot_seeds_match_makie() {
    let pi = std::f64::consts::PI;
    assert_eq!(streamplot(pendulum, -pi..=pi, -3.0..=3.0).seeds(), 177);
    let vdp = streamplot(|x, y| (y, (1.0 - x * x) * y - x), -3.0..=3.0, -4.0..=4.0).density(0.8).gridsize((24, 24));
    assert_eq!(vdp.seeds(), 48);
    // The Colorbar follows the lines' speed range.
    let m = streamplot(pendulum, -pi..=pi, -3.0..=3.0).colormapping().unwrap();
    assert!(m.mapped && (m.colorrange.0 - 0.036824).abs() < 1e-6 && (m.colorrange.1 - 3.370254).abs() < 1e-6);
}

#[test]
fn autolimits_span_the_box() {
    let sp = streamplot(|x, y| (-y, x), 0.0..=2.0, -1.0..=1.0);
    let l = sp.axis().geometry().unwrap().limits;
    // Streamlines stay inside the box and reach close to its edges.
    assert!(l[0] >= -0.1 - 1e-9 && l[1] <= 2.1 + 1e-9 && l[2] >= -1.1 - 1e-9 && l[3] <= 1.1 + 1e-9, "{l:?}");
    assert!(l[0] < 0.0 && l[1] > 2.0 && l[2] < -1.0 && l[3] > 1.0, "{l:?}");
}

/// Counts pixels whose color is close to `c`.
fn count(img: &sciplot::RgbaImage, c: [u8; 3]) -> usize {
    img.data.as_chunks::<4>().0.iter().filter(|p| (0..3).all(|k| p[k].abs_diff(c[k]) < 40)).count()
}

#[test]
fn renders_on_the_cpu_and_as_svg() {
    let (fig, _, _) = figure();
    let img = fig.render_rgba(&Save::new().cpu(true)).unwrap();
    assert!(count(&img, [255, 0, 0]) > 200, "red streamlines drawn");
    // Viridis-colored arrows (dark purple to yellow).
    assert!(count(&img, [68, 1, 84]) + count(&img, [253, 231, 37]) > 20);
    let svg = fig.to_svg_string(&Save::new()).unwrap();
    assert!(svg.matches("<path").count() > 10);
}

#[test]
fn renders_on_the_gpu_and_reuses_buffers() {
    let (fig, ar, _) = figure();
    let img = match fig.render_rgba(&Save::new()) {
        Ok(img) => img,
        Err(sciplot::Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    assert!(count(&img, [255, 0, 0]) > 200);
    let mut off = match sciplot::testing::Offscreen::new(1.0) {
        Ok(o) => o,
        Err(sciplot::Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    off.frame(&fig).unwrap();
    let idle = off.frame(&fig).unwrap();
    assert_eq!(idle.data_bytes, 0, "an unchanged frame uploads no plot data");
    // Panning rebuilds the arrow and arrowhead meshes; the streamlines stay on the GPU.
    sciplot::testing::set_interactive_limits(&ar.axis(), [-2.0, 2.0, -2.0, 2.0]);
    let pan = off.frame(&fig).unwrap();
    assert!(pan.data_bytes > 0);
}
