//! Lines / ScatterLines: API, live appends (upload only the tail), and translucent joints.

use sciplot::prelude::*;
use sciplot::testing::{Offscreen, set_interactive_limits};

fn offscreen() -> Option<Offscreen> {
    match Offscreen::new(2.0) {
        Ok(o) => Some(o),
        Err(sciplot::Error::NoGpuAdapter(_)) => None, // no GPU on this machine
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn api_and_live_data() {
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    let l = lines!(ax, [1.0, 2.0, 3.0], [3, 1, 2]; color = RED, linewidth = 3, linestyle = Linestyle::Dash);
    assert_eq!(l.len(), 3);
    l.push(4, 5.0).push(5.0f32, 6u8);
    assert_eq!(l.len(), 5);
    l.extend([6.0, 7.0], vec![1.0, 2.0]);
    assert_eq!(l.len(), 7);
    l.clear();
    assert!(l.is_empty());
    l.set_points(&[(0, 1), (1, 2)]);
    assert_eq!(l.len(), 2);
    l.set_data(0..10, (0..10).map(|i| i * i));
    assert_eq!(l.len(), 10);

    let y_only = lines!(ax, [3.0, 1.0, 2.0]; joinstyle = JoinStyle::Round, linecap = LineCap::Round);
    assert_eq!(y_only.len(), 3);
    let pts = lines!(ax, &[[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(pts.len(), 2);
    let sl = scatterlines!(ax, [1, 2, 3], [1, 4, 9]; markersize = 12, markercolor = BLUE, miter_limit = 0.5);
    sl.push(4, 16);
    assert_eq!(sl.len(), 4);
    assert_eq!(scatterlines!(ax, vec![(1.0, 2.0)]).len(), 1);

    let (f2, _, p) = lines(0..5, [1, 2, 3, 4, 5]).unpack();
    assert_eq!(p.len(), 5);
    assert_eq!(f2.axes().len(), 1);
    assert_eq!(fig.at(1, 2).scatterlines_points([1.0, 2.0]).len(), 2);
    let _ = lines_points([1.0, 2.0]);
    let _ = scatterlines([1.0], [2.0]);
}

#[test]
#[should_panic(expected = "x has 2 values but y has 3")]
fn extend_length_mismatch_panics() {
    let fig = Figure::new();
    Axis::new(fig.at(1, 1)).lines([0.0], [0.0]).extend([1.0, 2.0], [1.0, 2.0, 3.0]);
}

#[test]
fn theme_defaults_apply() {
    let t = Theme::new().lines(|l| l.linewidth(5).color(RED)).scatterlines(|s| s.markersize(20));
    let fig = Figure::new().theme(t);
    let ax = Axis::new(fig.at(1, 1));
    ax.lines([0.0, 1.0], [0.0, 1.0]);
    ax.scatterlines([0.0, 1.0], [1.0, 0.0]);
    let Some(_) = offscreen() else { return };
    let img = fig.render_rgba(&Save::new()).unwrap();
    // Red (themed) line pixels exist.
    let red = img.data.chunks_exact(4).filter(|p| p[0] > 200 && p[1] < 40 && p[2] < 40).count();
    assert!(red > 500, "expected a themed red line, found {red} red pixels");
}

/// A 1M-point line pans without re-uploading its points, and `push` uploads only the tail.
#[test]
fn pan_and_push_1m_line_upload_only_what_changed() {
    let n = 1_000_000;
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let y: Vec<f64> = (0..n).map(|i| (i as f64 * 0.001).sin()).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    let l = ax.lines(&x, &y);
    let Some(mut off) = offscreen() else { return };

    let first = off.frame(&fig).unwrap();
    assert!(first.data_bytes >= (n * 8) as u64, "first frame uploads the points");
    for k in 0..10 {
        let d = 1000.0 * k as f64;
        set_interactive_limits(&ax, [1e5 + d, 2e5 + d, -1.0, 1.0]);
        let s = off.frame(&fig).unwrap();
        assert_eq!(s.data_bytes, 0, "pan frame {k} re-uploaded plot data");
    }

    l.push(n as f64, 0.5);
    let s = off.frame(&fig).unwrap();
    assert_eq!(s.data_bytes, 8, "push of one point uploads one point");

    l.extend((0..1000).map(|i| (n + 1 + i) as f64), (0..1000).map(|_| 0.0));
    let s = off.frame(&fig).unwrap();
    assert_eq!(s.data_bytes, 8000, "extend uploads only the new points");

    // Following the data (autolimits) keeps the rebase, so appends stay incremental.
    ax.reset_limits();
    off.frame(&fig).unwrap();
    for i in 0..5 {
        l.push((n + 2000 + i) as f64, 0.0);
        let s = off.frame(&fig).unwrap();
        assert_eq!(s.data_bytes, 8, "live push {i}");
    }
}

/// Translucent lines: joints (miter, bevel, round, acute, reversals) are never darker than the
/// line itself, i.e. no pixel is blended twice.
#[test]
fn translucent_joints_blend_once() {
    let fig = Figure::new().size((500, 400));
    let ax = Axis::new(fig.at(1, 1)).hidedecorations(true).hidespines();
    let zig = |y0: f64, dx: f64| -> (Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..6).map(|i| 0.5 + dx * i as f64).collect();
        let y: Vec<f64> = (0..6).map(|i| y0 + if i % 2 == 0 { 0.0 } else { 1.5 }).collect();
        (x, y)
    };
    for (k, join) in [JoinStyle::Miter, JoinStyle::Bevel, JoinStyle::Round].into_iter().enumerate() {
        // Apex angles from ~42° down to ~4.5°; non-adjacent legs stay apart.
        for (j, dx) in [1.6, 0.8, 0.3, 0.2].into_iter().enumerate() {
            let (x, y) = zig(0.3 + 1.7 * j as f64, dx);
            let x: Vec<f64> = x.iter().map(|v| v + 10.0 * k as f64).collect();
            lines!(ax, &x, &y; linewidth = 5, joinstyle = join, color = (BLACK, 0.5));
        }
    }
    ax.limits(0.0, 30.0, 0.0, 7.2);
    let Some(_) = offscreen() else { return };
    if std::env::var_os("SCIPLOT_TEST_DUMP").is_some() {
        std::fs::create_dir_all("out").ok();
        fig.save("out/test_translucent_joints.png").unwrap();
    }
    let img = fig.render_rgba(&Save::new()).unwrap();
    // 50% black over white is 127.5; allow AA/rounding slack but nothing near 64 (double blend).
    let darkest = img.data.chunks_exact(4).map(|p| p[0]).min().unwrap();
    let covered = img.data.chunks_exact(4).filter(|p| p[0] < 140).count();
    assert!(covered > 5_000, "lines were drawn ({covered} px)");
    assert!(darkest >= 120, "a joint was blended twice (darkest pixel {darkest})");
}

/// Segments ending far off-screen (zoomed in 10^4×) are clipped to a guard band and still land
/// exactly where they should, dashed or not.
#[test]
fn far_offscreen_points_stay_precise() {
    let fig = Figure::new().size((200, 200));
    let ax = Axis::new(fig.at(1, 1)).hidedecorations(true).hidespines();
    ax.lines([-1e4, 1e4], [-1e4, 1e4]).color(BLACK).linewidth(4);
    ax.lines([-1e4, 1e4], [0.5, 0.5]).color(BLACK).linewidth(4).linestyle(Linestyle::Dash);
    ax.limits(-1.0, 1.0, -1.0, 1.0);
    let Some(_) = offscreen() else { return };
    let img = fig.render_rgba(&Save::new().px_per_unit(1)).unwrap();
    let w = img.width as usize;
    let at = |x: usize, y: usize| img.data[(y * w + x) * 4];
    // Axis area is 168×168 units starting at 16: the diagonal passes through the center.
    assert!(at(100, 100) < 40, "diagonal through the center");
    assert!(at(100, 70) > 200, "nothing off the diagonal");
    let dashed_row = 16 + 42; // y = 0.5
    let dark = (16..184).filter(|&x| at(x, dashed_row) < 60).count();
    assert!(dark > 50 && dark < 120, "about half of the dashed line is drawn ({dark} px)");
}
