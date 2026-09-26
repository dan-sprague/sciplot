//! Architecture check: panning a 1M-point scatter re-uploads no plot data.

use ezviz::prelude::*;
use ezviz::testing::{Offscreen, set_interactive_limits};
use std::time::Instant;

#[test]
fn pan_1m_scatter_without_reupload() {
    let n = 1_000_000;
    let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.618034).fract()).collect();
    let y: Vec<f64> = (0..n).map(|i| (i as f64 * 0.414214).fract()).collect();
    let fig = Figure::new();
    let ax = Axis::new(fig.at(1, 1));
    ax.scatter(&x, &y).markersize(4);

    let mut off = match Offscreen::new(2.0) {
        Ok(o) => o,
        Err(ezviz::Error::NoGpuAdapter(_)) => return, // no GPU on this machine
        Err(e) => panic!("{e}"),
    };
    let first = off.frame(&fig).unwrap();
    assert!(
        first.data_bytes >= (n * 8) as u64,
        "first frame uploads the points"
    );

    let mut times = Vec::new();
    for k in 0..20 {
        let d = 0.01 * k as f64;
        set_interactive_limits(&ax, [0.1 + d, 0.6 + d, 0.2, 0.7]);
        let t = Instant::now();
        let s = off.frame(&fig).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(s.data_bytes, 0, "pan frame {k} re-uploaded plot data");
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "1M scatter pan frame (offscreen incl. readback): median {:.1} ms, max {:.1} ms",
        times[10], times[19]
    );
}
