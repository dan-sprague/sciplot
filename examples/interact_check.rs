//! Interaction check: four axes (linear, log x, reversed y, and one x-linked to the first) to try
//! Makie's bindings by hand:
//!
//! - scroll: zoom about the cursor (hold `x` or `y` to zoom one dimension); trackpad pinch too
//! - right drag, or Option/Alt + left drag: pan
//! - left drag: rectangle zoom (hold `x` or `y` to restrict it)
//! - Ctrl + click or double-click: reset; Ctrl + Shift + click: full autolimits
//! - hover a point: tooltip with its coordinates
//!
//! With `--features testing` the example scripts a hover and a rectangle zoom instead and writes
//! the frames, overlays included, to `out/interact_*.png`:
//!
//! `cargo run --example interact_check --features testing`
use ezviz::prelude::*;

fn main() -> ezviz::Result<()> {
    let fig = Figure::new().size((900, 700)).window_title("ezviz: interaction check");
    let x: Vec<f64> = (0..40).map(|i| 0.25 * i as f64).collect();
    let y: Vec<f64> = x.iter().map(|v| v.sin()).collect();

    let lin = Axis::new(fig.at(1, 1)).title("linear");
    lin.scatter(&x, &y);
    let log = Axis::new(fig.at(1, 2)).title("log x").xscale(Scale::Log10);
    let xl: Vec<f64> = (0..40).map(|i| 10f64.powf(i as f64 / 13.0)).collect();
    log.scatter(&xl, &y);
    let rev = Axis::new(fig.at(2, 1)).title("reversed y").yreversed(true);
    rev.scatter(&x, &y);
    let linked = Axis::new(fig.at(2, 2)).title("x linked to linear");
    linked.scatter(&x, x.iter().map(|v| v.cos()));
    linkxaxes(&[&lin, &linked]);

    #[cfg(feature = "testing")]
    {
        let r = fig.show_live(|live| script(live, &x, &y))?;
        if let Err(e) = r {
            eprintln!("interact_check: {e}");
        }
        Ok(())
    }
    #[cfg(not(feature = "testing"))]
    fig.show()
}

/// Hovers a point of the linear axis, drags a rectangle zoom over the log axis (x restricted),
/// and dumps the frames.
#[cfg(feature = "testing")]
fn script(live: &Live, x: &[f64], y: &[f64]) -> Result<(), String> {
    use ezviz::interact::{Button, Key};
    use ezviz::window_testing::{self as wt, Synthetic as S};
    use std::time::Duration;

    std::fs::create_dir_all("out").map_err(|e| e.to_string())?;
    let send = |evs: &[S]| -> Result<(), String> {
        for e in evs {
            wt::inject(live, *e);
        }
        if !wt::flush(live) {
            return Err("the window did not process the events".into());
        }
        live.wait_frame_timeout(Duration::from_secs(2));
        Ok(())
    };
    live.wait_frame_timeout(Duration::from_secs(5));
    let views = wt::axis_views(live.figure());
    let (Some(v_lin), Some(v_log)) = (views.first(), views.get(1)) else {
        return Err("expected four axes".into());
    };
    let [gx, gy, gw, gh] = v_log.rect;

    // Hover near point 13 of the linear axis.
    let p = v_lin.to_units(x[13], y[13]).ok_or("point outside the axis")?;
    send(&[S::CursorMoved { x: p[0] + 3.0, y: p[1] - 2.0 }])?;
    println!("hover: {:?}", wt::hover_text(live.figure()));
    wt::dump(live, "out/interact_hover.png")?;

    // Rectangle zoom on the log axis, restricted to x; dump mid-drag, then after release.
    send(&[
        S::CursorMoved { x: gx + 0.3 * gw, y: gy + 0.3 * gh },
        S::Button { button: Button::Left, pressed: true },
        S::CursorMoved { x: gx + 0.5 * gw, y: gy + 0.5 * gh },
        S::CursorMoved { x: gx + 0.7 * gw, y: gy + 0.6 * gh },
    ])?;
    wt::dump(live, "out/interact_rectzoom.png")?;
    send(&[S::Key { key: Key::X, pressed: true }])?;
    wt::dump(live, "out/interact_rectzoom_x.png")?;
    send(&[S::Button { button: Button::Left, pressed: false }, S::Key { key: Key::X, pressed: false }])?;
    wt::dump(live, "out/interact_zoomed.png")?;
    println!("wrote out/interact_{{hover,rectzoom,rectzoom_x,zoomed}}.png");
    live.close();
    Ok(())
}
