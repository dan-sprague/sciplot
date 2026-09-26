//! The window interaction state machine against Makie's formulas (`interactions.jl`).
#![cfg(feature = "window")]

use ezviz::Scale;
use ezviz::interact::{AxisView, Button, Effect, Input, InteractState, Key, Modifiers, apply_to_views, handle};

/// A 400 × 300 axis at (100, 50) showing [0, 10] × [0, 10].
fn view() -> AxisView {
    AxisView::new([100.0, 50.0, 400.0, 300.0], [0.0, 10.0, 0.0, 10.0])
}

fn run(st: &mut InteractState, views: &mut [AxisView], inputs: &[Input]) -> Vec<Effect> {
    let mut all = Vec::new();
    for i in inputs {
        let fx = handle(st, *i, views);
        apply_to_views(views, &fx);
        all.extend(fx);
    }
    all
}

fn limits(fx: &[Effect]) -> Vec<(usize, [f64; 4])> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::SetLimits { axis, limits } => Some((*axis, *limits)),
            _ => None,
        })
        .collect()
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9 * (1.0 + y.abs()))
}

#[track_caller]
fn assert_lims(got: [f64; 4], want: [f64; 4]) {
    assert!(close(got, want), "got {got:?}, want {want:?}");
}

const fn press(button: Button, time: f64) -> Input {
    Input::Button { button, pressed: true, time }
}
const fn release(button: Button, time: f64) -> Input {
    Input::Button { button, pressed: false, time }
}

#[test]
fn scroll_zooms_about_the_cursor() {
    let mut views = [view()];
    let mut st = InteractState::default();
    // Cursor at 25 % of the width and 75 % of the height (from the bottom).
    let fx = run(&mut st, &mut views, &[Input::CursorMoved([200.0, 125.0]), Input::Scroll([0.0, 1.0])]);
    let l = limits(&fx);
    assert_eq!(l.len(), 1);
    // Makie: width *= 0.9; origin += f * (w - new_w).
    assert_lims(l[0].1, [0.25, 9.25, 0.75, 9.75]);
    // The data point under the cursor stays put.
    let v = &views[0];
    assert!((v.limits[0] + 0.25 * (v.limits[1] - v.limits[0]) - 2.5).abs() < 1e-12);
    // Scrolling back restores the limits; outside the axis nothing happens.
    run(&mut st, &mut views, &[Input::Scroll([0.0, -1.0])]);
    assert_lims(views[0].limits, [0.0, 10.0, 0.0, 10.0]);
    let fx = run(&mut st, &mut views, &[Input::CursorMoved([20.0, 20.0]), Input::Scroll([0.0, 3.0])]);
    assert!(fx.is_empty());
}

#[test]
fn zoom_key_locks_restrict_to_one_dimension() {
    let mut views = [view()];
    let mut st = InteractState::default();
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0])]);
    run(&mut st, &mut views, &[Input::Key { key: Key::X, pressed: true }, Input::Scroll([0.0, 1.0])]);
    assert_lims(views[0].limits, [0.5, 9.5, 0.0, 10.0]);
    run(&mut st, &mut views, &[Input::Key { key: Key::X, pressed: false }]);
    run(&mut st, &mut views, &[Input::Key { key: Key::Y, pressed: true }, Input::Scroll([0.0, 1.0])]);
    assert_lims(views[0].limits, [0.5, 9.5, 0.5, 9.5]);
}

#[test]
fn trackpad_pinch_zooms_about_the_cursor() {
    let mut views = [view()];
    let mut st = InteractState::default();
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0]), Input::Pinch(0.25)]);
    // Magnify by 1.25 about the center: width 10 / 1.25 = 8.
    assert_lims(views[0].limits, [1.0, 9.0, 1.0, 9.0]);
}

#[test]
fn right_drag_pans() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let fx = run(
        &mut st,
        &mut views,
        &[Input::CursorMoved([300.0, 200.0]), press(Button::Right, 0.0), Input::CursorMoved([340.0, 170.0])],
    );
    // Moving 10 % right and 10 % up moves the limits 10 % left and down.
    assert_lims(limits(&fx)[0].1, [-1.0, 9.0, -1.0, 9.0]);
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 170.0]), release(Button::Right, 0.1)]);
    assert_lims(views[0].limits, [0.0, 10.0, -1.0, 9.0]);
    // Holding x pans x only.
    run(
        &mut st,
        &mut views,
        &[
            Input::Key { key: Key::X, pressed: true },
            press(Button::Right, 1.0),
            Input::CursorMoved([260.0, 140.0]),
            release(Button::Right, 1.1),
        ],
    );
    assert_lims(views[0].limits, [1.0, 11.0, -1.0, 9.0]);
}

#[test]
fn option_left_drag_pans() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let alt = Modifiers { alt: true, ..Default::default() };
    run(
        &mut st,
        &mut views,
        &[
            Input::Modifiers(alt),
            Input::CursorMoved([300.0, 200.0]),
            press(Button::Left, 0.0),
            Input::CursorMoved([340.0, 200.0]),
            release(Button::Left, 0.1),
        ],
    );
    assert_lims(views[0].limits, [-1.0, 9.0, 0.0, 10.0]);
}

#[test]
fn rectangle_zoom_and_x_restriction() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let fx = run(
        &mut st,
        &mut views,
        &[Input::CursorMoved([140.0, 80.0]), press(Button::Left, 0.0), Input::CursorMoved([141.0, 80.0])],
    );
    assert!(fx.is_empty(), "no drag before 2 px");
    assert!(st.selection(&views).is_none());
    let fx = run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0])]);
    assert_eq!(fx, vec![Effect::Overlay]);
    let (axis, sel) = st.selection(&views).unwrap();
    assert_eq!(axis, 0);
    assert_lims(sel, [1.0, 5.0, 5.0, 9.0]);
    // Holding x restricts the zoom to x (the full y range is kept), live while dragging.
    let fx = run(&mut st, &mut views, &[Input::Key { key: Key::X, pressed: true }]);
    assert_eq!(fx, vec![Effect::Overlay]);
    assert_lims(st.selection(&views).unwrap().1, [1.0, 5.0, 0.0, 10.0]);
    let fx = run(&mut st, &mut views, &[release(Button::Left, 0.5)]);
    assert_lims(limits(&fx)[0].1, [1.0, 5.0, 0.0, 10.0]);
    assert!(fx.contains(&Effect::Overlay), "the shade disappears");
    assert!(st.selection(&views).is_none());
}

#[test]
fn rectangle_zoom_is_clamped_and_rejects_empty_selections() {
    let mut views = [view()];
    let mut st = InteractState::default();
    // Dragging past the axis corner clamps the selection to the visible limits.
    run(
        &mut st,
        &mut views,
        &[
            Input::CursorMoved([300.0, 200.0]),
            press(Button::Left, 0.0),
            Input::CursorMoved([700.0, 0.0]),
            release(Button::Left, 0.1),
        ],
    );
    assert_lims(views[0].limits, [5.0, 10.0, 5.0, 10.0]);
    // A purely horizontal drag selects zero height: ignored.
    let fx = run(
        &mut st,
        &mut views,
        &[
            Input::CursorMoved([200.0, 200.0]),
            press(Button::Left, 1.0),
            Input::CursorMoved([400.0, 200.0]),
            release(Button::Left, 1.1),
        ],
    );
    assert!(limits(&fx).is_empty());
}

#[test]
fn ctrl_click_resets_and_ctrl_shift_click_autolimits() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let ctrl = Modifiers { ctrl: true, ..Default::default() };
    let fx = run(
        &mut st,
        &mut views,
        &[
            Input::Modifiers(ctrl),
            Input::CursorMoved([300.0, 200.0]),
            press(Button::Left, 0.0),
            Input::CursorMoved([301.0, 200.0]), // under the drag threshold: still a click
            release(Button::Left, 0.05),
        ],
    );
    assert_eq!(fx, vec![Effect::ResetLimits { axis: 0 }]);
    let fx = run(
        &mut st,
        &mut views,
        &[
            Input::Modifiers(Modifiers { ctrl: true, shift: true, alt: false }),
            press(Button::Left, 1.0),
            release(Button::Left, 1.05),
        ],
    );
    assert_eq!(fx, vec![Effect::AutoLimits { axis: 0 }]);
}

#[test]
fn double_click_resets() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let click = |t: f64| [press(Button::Left, t), release(Button::Left, t + 0.05)];
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0])]);
    assert!(run(&mut st, &mut views, &click(0.0)).is_empty());
    assert!(run(&mut st, &mut views, &click(0.5)).is_empty(), "too slow for a double-click");
    assert_eq!(run(&mut st, &mut views, &click(0.6)), vec![Effect::ResetLimits { axis: 0 }]);
    assert!(run(&mut st, &mut views, &click(0.7)).is_empty(), "a third click starts over");
}

#[test]
fn log_axes_zoom_in_scaled_space() {
    let mut v = view();
    v.xscale = Scale::Log10;
    v.limits = [1.0, 1000.0, 0.0, 10.0];
    let mut views = [v];
    let mut st = InteractState::default();
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0]), Input::Scroll([0.0, 1.0])]);
    // log10 limits [0, 3] -> width 2.7 about the middle: [0.15, 2.85].
    assert_lims(views[0].limits, [10f64.powf(0.15), 10f64.powf(2.85), 0.5, 9.5]);
    // Panning a log axis by 10 % of the width shifts by a factor, never below zero.
    let mut views = [AxisView { limits: [1.0, 1000.0, 0.0, 10.0], ..views[0].clone() }];
    run(
        &mut st,
        &mut views,
        &[press(Button::Right, 0.0), Input::CursorMoved([340.0, 200.0]), release(Button::Right, 0.1)],
    );
    assert_lims(views[0].limits, [10f64.powf(-0.3), 10f64.powf(2.7), 0.0, 10.0]);
}

#[test]
fn reversed_axes_zoom_and_pan_the_right_way() {
    let mut v = view();
    v.xreversed = true;
    let mut views = [v];
    let mut st = InteractState::default();
    // 25 % from the left of a reversed axis is data x = 7.5.
    run(&mut st, &mut views, &[Input::CursorMoved([200.0, 200.0]), Input::Scroll([0.0, 1.0])]);
    assert_lims(views[0].limits, [0.75, 9.75, 0.5, 9.5]);
    let under = views[0].limits[1] - 0.25 * (views[0].limits[1] - views[0].limits[0]);
    assert!((under - 7.5).abs() < 1e-12);
    // Dragging right moves toward smaller x on screen, so the limits increase.
    let mut views = [AxisView { limits: [0.0, 10.0, 0.0, 10.0], ..views[0].clone() }];
    run(
        &mut st,
        &mut views,
        &[press(Button::Right, 0.0), Input::CursorMoved([240.0, 200.0]), release(Button::Right, 0.1)],
    );
    assert_lims(views[0].limits, [1.0, 11.0, 0.0, 10.0]);
}

#[test]
fn linked_axes_follow() {
    let mut a = view();
    let mut b = AxisView::new([100.0, 400.0, 400.0, 300.0], [0.0, 10.0, 100.0, 200.0]);
    let mut c = AxisView::new([600.0, 50.0, 400.0, 300.0], [-5.0, 5.0, 0.0, 10.0]);
    a.xlinks = vec![1];
    b.xlinks = vec![0];
    a.ylinks = vec![2];
    c.ylinks = vec![0];
    let mut views = [a, b, c];
    let mut st = InteractState::default();
    let fx = run(&mut st, &mut views, &[Input::CursorMoved([200.0, 125.0]), Input::Scroll([0.0, 1.0])]);
    let l = limits(&fx);
    assert_eq!(l.len(), 3);
    assert_eq!(l[0].0, 0);
    // b shares x only, c shares y only.
    assert_lims(views[1].limits, [0.25, 9.25, 100.0, 200.0]);
    assert_lims(views[2].limits, [-5.0, 5.0, 0.75, 9.75]);
    // Zooming the linked axis carries x back to the first one (and on to nothing else).
    run(&mut st, &mut views, &[Input::CursorMoved([300.0, 550.0]), Input::Scroll([0.0, -1.0])]);
    assert!((views[0].limits[1] - views[0].limits[0] - 10.0).abs() < 1e-9);
    assert_eq!(views[0].limits[0], views[1].limits[0]);
    assert_lims([views[0].limits[2], views[0].limits[3], 0.0, 0.0], [0.75, 9.75, 0.0, 0.0]);
}

#[test]
fn invalid_zoom_is_ignored() {
    let mut views = [view()];
    let mut st = InteractState::default();
    let fx = run(&mut st, &mut views, &[Input::CursorMoved([300.0, 200.0]), Input::Scroll([0.0, 1e5])]);
    assert!(fx.is_empty());
    assert_lims(views[0].limits, [0.0, 10.0, 0.0, 10.0]);
}

#[test]
fn focus_loss_cancels_drags() {
    let mut views = [view()];
    let mut st = InteractState::default();
    run(
        &mut st,
        &mut views,
        &[Input::CursorMoved([140.0, 80.0]), press(Button::Left, 0.0), Input::CursorMoved([300.0, 200.0])],
    );
    assert!(st.dragging());
    let fx = run(&mut st, &mut views, &[Input::FocusLost, release(Button::Left, 1.0)]);
    assert_eq!(fx, vec![Effect::Overlay]);
    assert!(!st.dragging());
    assert_lims(views[0].limits, [0.0, 10.0, 0.0, 10.0]);
}
