//! `Figure::animate` outside a window: the test harness runs tests off the main thread, where
//! opening a window must fail cleanly without calling the callback. (Frame timing and stop/close
//! control are unit-tested in `src/window/animate.rs`; `examples/s5_animate.rs` is the windowed
//! smoke test.)
#![cfg(feature = "window")]

use sciplot::prelude::*;

#[test]
fn animate_off_main_thread_is_an_error() {
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    let sc = ax.scatter([0.0], [0.0]);
    let mut calls = 0;
    let r = fig.animate(|frame: &mut Frame| {
        calls += 1;
        sc.set_data([frame.t], [frame.dt]);
        frame.stop();
    });
    match r {
        Err(sciplot::Error::NotMainThread) | Err(sciplot::Error::NoGpuAdapter(_)) => {}
        other => panic!("expected NotMainThread (or no GPU), got {other:?}"),
    }
    assert_eq!(calls, 0);
}
