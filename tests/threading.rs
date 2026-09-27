use sciplot::prelude::*;

/// The test harness runs tests off the main thread, which is exactly the case we must reject.
#[cfg(feature = "window")]
#[test]
fn show_off_main_thread_is_an_error() {
    let fig = Figure::new();
    Axis::new(fig.at(1, 1)).scatter([1.0], [1.0]);
    match fig.show() {
        Err(sciplot::Error::NotMainThread) => {}
        other => panic!("expected NotMainThread, got {other:?}"),
    }
}

#[test]
fn handles_are_send_and_update_from_threads() {
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    let sc = ax.scatter([0.0, 1.0], [0.0, 1.0]);
    std::thread::scope(|s| {
        for t in 0..8 {
            let sc = sc.clone();
            let ax = ax.clone();
            s.spawn(move || {
                for i in 0..200 {
                    let v = (t * 1000 + i) as f64;
                    sc.set_data([0.0, v], [0.0, v]);
                    ax.title(format!("{v}"));
                }
            });
        }
    });
    assert_eq!(sc.len(), 2);
}

#[test]
#[should_panic(expected = "1-based")]
fn zero_index_panics() {
    let fig = Figure::new();
    let _ = fig.at(0, 1);
}
