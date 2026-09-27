//! Real-window smoke test (needs a display): scripted interactions through `show_live`, a worker
//! panic, and pump mode. Windows need the main thread, hence `harness = false`.
//!
//! `SCIPLOT_WINDOW_TESTS=1 cargo test --features testing --test window_smoke`

use sciplot::Error;
use sciplot::interact::{Button, Key, Modifiers};
use sciplot::prelude::*;
use sciplot::window_testing::{self as wt, Synthetic};
use std::time::{Duration, Instant};

fn main() {
    if std::env::var("SCIPLOT_WINDOW_TESTS").as_deref() != Ok("1") {
        println!("window_smoke: skipped (set SCIPLOT_WINDOW_TESTS=1 to open windows)");
        return;
    }
    if matches!(sciplot::testing::Offscreen::new(1.0), Err(Error::NoGpuAdapter(_))) {
        println!("window_smoke: skipped (no GPU adapter)");
        return;
    }
    scripted_interactions();
    jitter_guard();
    worker_panic_is_reported();
    pump_mode();
    println!("window_smoke: ok");
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6 * (1.0 + y.abs()))
}

fn scripted_interactions() {
    let fig = Figure::new();
    // Fixed tick label space: the axis rectangle must not move as the tick labels change.
    let ax = Axis::new(fig.at(1, 1)).limits(0.0, 10.0, 0.0, 10.0).xticklabelspace(20.0).yticklabelspace(40.0);
    let xs: Vec<f64> = (0..=10).map(f64::from).collect();
    let sc = ax.scatter(&xs, &xs);
    let t0 = Instant::now();
    let failures = fig
        .show_live(|live| {
            let mut fail = Vec::new();
            let mut check = |ok: bool, what: String| {
                if !ok {
                    fail.push(what);
                }
            };
            let send = |evs: &[Synthetic]| {
                for e in evs {
                    wt::inject(live, *e);
                }
                assert!(wt::flush(live), "window did not process events");
                live.wait_frame_timeout(Duration::from_secs(3));
            };
            live.wait_frame_timeout(Duration::from_secs(5));
            let Some([x, y, w, h]) = wt::axis_rects(live.figure()).first().copied() else {
                live.close();
                return vec!["no axis frame".to_string()];
            };
            let at = |fx: f64, fy: f64| Synthetic::CursorMoved { x: x + fx * w, y: y + (1.0 - fy) * h };
            let click =
                |b| [Synthetic::Button { button: b, pressed: true }, Synthetic::Button { button: b, pressed: false }];

            // Scroll zoom about the center; hover the point at the center.
            send(&[at(0.5, 0.5), Synthetic::ScrollLines { dx: 0.0, dy: 1.0 }]);
            let l = wt::interactive_limits(&ax);
            check(l.is_some_and(|l| close(l, [0.5, 9.5, 0.5, 9.5])), format!("scroll zoom: {l:?}"));
            let hover = wt::hover_text(live.figure());
            check(hover.as_deref() == Some("x: 5\ny: 5"), format!("hover: {hover:?}"));

            // Right-drag pan by 10 % of the width.
            send(&[
                Synthetic::Button { button: Button::Right, pressed: true },
                at(0.55, 0.5),
                at(0.6, 0.5),
                Synthetic::Button { button: Button::Right, pressed: false },
            ]);
            let l = wt::interactive_limits(&ax);
            check(l.is_some_and(|l| close(l, [-0.4, 8.6, 0.5, 9.5])), format!("pan: {l:?}"));

            // Ctrl+click resets to the user limits.
            let ctrl = Modifiers { ctrl: true, ..Default::default() };
            send(&[Synthetic::Modifiers(ctrl)]);
            send(&click(Button::Left));
            send(&[Synthetic::Modifiers(Modifiers::default())]);
            let l = wt::interactive_limits(&ax);
            check(l.is_none(), format!("reset: {l:?}"));

            // Rectangle zoom restricted to x.
            send(&[
                Synthetic::Key { key: Key::X, pressed: true },
                at(0.2, 0.5),
                Synthetic::Button { button: Button::Left, pressed: true },
                at(0.4, 0.6),
                at(0.6, 0.7),
                Synthetic::Button { button: Button::Left, pressed: false },
                Synthetic::Key { key: Key::X, pressed: false },
            ]);
            let l = wt::interactive_limits(&ax);
            check(l.is_some_and(|l| close(l, [2.0, 6.0, 0.0, 10.0])), format!("rect zoom: {l:?}"));

            // Trackpad-style pixel scroll zooms too.
            send(&[at(0.5, 0.5), Synthetic::ScrollPixels { dx: 0.0, dy: -16.0 }]);
            let l = wt::interactive_limits(&ax);
            check(
                l.is_some_and(|l| close(l, [4.0 - 2.0 / 0.9, 4.0 + 2.0 / 0.9, 5.0 - 5.0 / 0.9, 5.0 + 5.0 / 0.9])),
                format!("pixel scroll: {l:?}"),
            );

            // Ctrl+Shift+click: full autolimits (the user limits are forgotten too).
            send(&[Synthetic::Modifiers(Modifiers { ctrl: true, shift: true, alt: false })]);
            send(&click(Button::Left));
            send(&[Synthetic::Modifiers(Modifiers::default())]);
            check(wt::interactive_limits(&ax).is_none(), "autolimits".into());

            // Minimized: waiting for frames must not hang.
            wt::set_minimized(live, true);
            std::thread::sleep(Duration::from_millis(800));
            let t = Instant::now();
            for i in 0..20 {
                sc.set_data(&xs, xs.iter().map(|v| v + i as f64));
                live.wait_frame_timeout(Duration::from_secs(2));
            }
            check(t.elapsed() < Duration::from_secs(5), format!("minimized wait took {:?}", t.elapsed()));
            wt::set_minimized(live, false);

            live.close();
            fail
        })
        .expect("show_live failed");
    assert!(failures.is_empty(), "scripted interactions failed: {failures:#?}");
    println!("window_smoke: scripted interactions ok ({:.1} s)", t0.elapsed().as_secs_f64());
}

/// While the user zooms, the axis keeps its tick-label space (the axis does not move under the
/// cursor); 0.2 s after the last event the layout adapts to the new, wider labels.
fn jitter_guard() {
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1)).limits(0.0, 10.0, 0.0, 10.0);
    ax.scatter([5.0], [5.0]);
    let result = fig
        .show_live(|live| {
            live.wait_frame_timeout(Duration::from_secs(5));
            let Some(before) = wt::axis_rects(live.figure()).first().copied() else {
                live.close();
                return Err("no axis frame".to_string());
            };
            let [x, y, w, h] = before;
            // Zoom 0.9^60 about the center in one burst: y tick labels go from "0".."10" to
            // "4.999".."5.001".
            let t = Instant::now();
            wt::inject(live, Synthetic::CursorMoved { x: x + 0.5 * w, y: y + 0.5 * h });
            for _ in 0..6 {
                wt::inject(live, Synthetic::ScrollLines { dx: 0.0, dy: 10.0 });
            }
            assert!(wt::flush(live), "window did not process events");
            live.wait_frame_timeout(Duration::from_secs(3));
            let during = wt::axis_rects(live.figure()).first().copied();
            // On a loaded machine the frame may come after the freeze ended: no verdict then.
            let in_time = t.elapsed() < Duration::from_millis(150);
            let zoomed = wt::interactive_limits(&ax);
            std::thread::sleep(Duration::from_millis(500));
            live.wait_frame_timeout(Duration::from_secs(3));
            let after = wt::axis_rects(live.figure()).first().copied();
            live.close();
            if zoomed.is_none_or(|l| l[3] - l[2] > 0.1) {
                return Err(format!("no zoom: {zoomed:?}"));
            }
            if in_time && during != Some(before) {
                return Err(format!("the axis moved during the zoom: {before:?} -> {during:?}"));
            }
            match after {
                Some(a) if a[0] > before[0] + 1.0 => Ok(in_time),
                _ => Err(format!("the layout did not adapt after the zoom: {before:?} -> {after:?}")),
            }
        })
        .expect("show_live failed");
    match result {
        Ok(true) => println!("window_smoke: jitter guard ok"),
        Ok(false) => println!("window_smoke: jitter guard: frame too late to check the freeze (machine busy)"),
        Err(e) => panic!("jitter guard: {e}"),
    }
}

fn worker_panic_is_reported() {
    let fig = Figure::new();
    Axis::new(fig.at(1, 1)).scatter([1.0, 2.0], [1.0, 2.0]);
    let r: Result<(), Error> = fig.show_live(|live| {
        std::thread::scope(|s| {
            // Close the window shortly after the panic below starts unwinding.
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(500));
                live.close();
            });
            panic!("boom");
        })
    });
    match r {
        Err(Error::WorkerPanicked(msg)) => assert!(msg.contains("boom"), "{msg}"),
        other => panic!("expected WorkerPanicked, got {other:?}"),
    }
    println!("window_smoke: worker panic ok");
}

fn pump_mode() {
    let fig = Figure::new();
    let sc = Axis::new(fig.at(1, 1)).scatter([0.0], [0.0]);
    let screen = fig.display().expect("display failed");
    let t = Instant::now();
    let mut n = 0;
    while screen.is_open() && t.elapsed() < Duration::from_millis(600) {
        n += 1;
        let a = n as f64 * 0.05;
        sc.set_data([a.cos()], [a.sin()]);
        screen.pump().expect("pump failed");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(screen.is_open(), "the pump window closed by itself");
    screen.close();
    println!("window_smoke: pump mode ok ({n} steps)");
}
