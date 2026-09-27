//! Interactive windows (winit + wgpu), native and in the browser, from the same figure code.
//!
//! - Native (`native.rs`, `live.rs`, `pump.rs`): blocking `show()` and `animate()`, live updates
//!   from a worker thread (`show_live`) and pump mode (`display` + `pump`).
//! - Web (`web.rs`, wasm32): `show()`, `show_in(canvas_id)` and `animate()` mount the figure into
//!   a `<canvas>` and return at once; the browser drives the loop (`requestAnimationFrame`).
//!   WebGPU is used when the browser has it, WebGL2 otherwise (`?backend=gl` forces WebGL2).
//!
//! Both share one app core (`app.rs`, a winit `ApplicationHandler`) that owns the windows. Each
//! window keeps the last built frame (draw list, axis frames, the snapshot it came from); hover
//! and the rectangle-zoom shade are overlays appended to that draw list, so they never trigger a
//! relayout. Makie's pan/zoom interactions (`interact.rs`, a pure state machine) get their input
//! from `input.rs` (mouse, wheel, keys, trackpad pinch, touch gestures).

mod animate;
mod app;
mod input;
pub mod interact;
mod live;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod overlay;
#[cfg(not(target_arch = "wasm32"))]
mod pump;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(target_arch = "wasm32")]
mod web;

pub use animate::Frame;
#[cfg(not(target_arch = "wasm32"))]
pub use live::Live;
#[cfg(not(target_arch = "wasm32"))]
pub use native::show_all;
#[cfg(not(target_arch = "wasm32"))]
pub use pump::Screen;
#[cfg(target_arch = "wasm32")]
pub use web::show_all;

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    /// A figure with this uid changed.
    Wake(u64),
    /// Close the windows of this `show_live` session.
    Close(u64),
    /// The simulation of this `show_live` session panicked.
    Panicked(u64, String),
    /// Web: figures to mount or GPU contexts arrived (see `web::INBOX`).
    #[cfg(target_arch = "wasm32")]
    Inbox,
    /// Scripted-test operation for the windows of the figure with this uid.
    #[cfg(feature = "testing")]
    Test(u64, testing::Op),
}
