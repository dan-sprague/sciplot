//! macOS: tag the window's CAMetalLayer as sRGB.
//!
//! wgpu 30 leaves the layer colorspace `nil` for `SurfaceColorSpace::Srgb`. Core Animation then
//! skips color matching, so on wide-gamut (P3) panels sRGB colors show oversaturated and would not
//! match the PNG export. Setting the colorspace explicitly fixes that. Reapplied after every
//! `configure`, which resets it.

#![allow(unsafe_code)]

use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};

pub(crate) fn force_srgb_colorspace(surface: &wgpu::Surface<'static>) {
    // SAFETY: `as_hal` gives access to the Metal surface of this wgpu surface; we only set a
    // property on its layer, on the main thread, while no frame is in flight.
    unsafe {
        if let Some(hal) = surface.as_hal::<wgpu::hal::api::Metal>() {
            let layer = hal.render_layer().lock();
            let cs = CGColorSpace::with_name(Some(kCGColorSpaceSRGB));
            layer.setColorspace(cs.as_deref());
        }
    }
}
