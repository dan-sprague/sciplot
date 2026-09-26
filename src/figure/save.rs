//! Export: `fig.save("out.png")`, `fig.save("out.svg")`, `fig.render_rgba(..)`.

use super::Figure;
use crate::error::{Error, Result};
use std::path::Path;

/// Export options. `Save::new()` gives Makie's CairoMakie defaults: 2 px per unit for bitmaps,
/// 0.75 pt per unit for vector output.
///
/// ```no_run
/// # use ezviz::prelude::*;
/// # let fig = Figure::new();
/// fig.save_with("fig.png", Save::dpi(300)).unwrap();   // px_per_unit = 300 / 96
/// ```
#[derive(Clone, Debug)]
pub struct Save {
    pub(crate) px_per_unit: f64,
    pub(crate) pt_per_unit: f64,
    pub(crate) background: Option<crate::color::Color>,
}

impl Default for Save {
    fn default() -> Self {
        Save { px_per_unit: 2.0, pt_per_unit: 0.75, background: None }
    }
}

impl Save {
    pub fn new() -> Save {
        Save::default()
    }
    /// Bitmap resolution in dots per inch (`px_per_unit = dpi / 96`).
    pub fn dpi(dpi: impl crate::attrs::Conv<f64>) -> Save {
        Save { px_per_unit: dpi.conv() / 96.0, ..Save::default() }
    }
    /// Device pixels per figure unit for bitmaps (default 2).
    pub fn px_per_unit(mut self, v: impl crate::attrs::Conv<f64>) -> Save {
        self.px_per_unit = v.conv();
        self
    }
    /// Points per figure unit for vector output (default 0.75, i.e. 1 unit = 1 CSS px).
    pub fn pt_per_unit(mut self, v: impl crate::attrs::Conv<f64>) -> Save {
        self.pt_per_unit = v.conv();
        self
    }
    /// Overrides the figure background (use `Color::TRANSPARENT` for a transparent PNG).
    pub fn backgroundcolor(mut self, c: impl crate::attrs::Conv<crate::color::Color>) -> Save {
        self.background = Some(c.conv());
        self
    }
}

/// An RGBA8 image (straight alpha, row-major, top row first).
#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Figure {
    /// Saves the figure; the format follows the extension (`.png` or `.svg`).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.save_with(path, Save::default())
    }

    /// Saves with explicit options.
    pub fn save_with(&self, path: impl AsRef<Path>, opts: Save) -> Result<()> {
        let path = path.as_ref();
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "png" => {
                let img = self.render_rgba(&opts)?;
                write_png(path, &img, opts.px_per_unit)
            }
            "svg" => {
                let s = self.to_svg_string(&opts)?;
                std::fs::write(path, s)?;
                Ok(())
            }
            other => Err(Error::UnsupportedFormat(other.to_string())),
        }
    }

    /// Renders to an RGBA8 image at `opts.px_per_unit` (headless; any thread).
    pub fn render_rgba(&self, opts: &Save) -> Result<RgbaImage> {
        let st = self.sh.snapshot();
        let gpu = crate::render::gpu::gpu()?;
        let mut r = crate::render::gpu::Renderer::new(gpu);
        let (mut dl, _) = crate::scene::build(&st, None, &mut r.scene);
        if let Some(bg) = opts.background {
            dl.background = bg;
        }
        let (width, height, data) = r.render_rgba(&dl, opts.px_per_unit)?;
        Ok(RgbaImage { width, height, data })
    }

    /// The figure as an SVG document.
    pub fn to_svg_string(&self, _opts: &Save) -> Result<String> {
        Err(Error::UnsupportedFormat("svg (not implemented yet)".into()))
    }
}

pub(crate) fn write_png(path: &Path, img: &RgbaImage, px_per_unit: f64) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let ppm = (96.0 * px_per_unit / 0.0254).round() as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
    let mut w = enc.write_header().map_err(|e| Error::Encode(e.to_string()))?;
    w.write_image_data(&img.data).map_err(|e| Error::Encode(e.to_string()))?;
    w.finish().map_err(|e| Error::Encode(e.to_string()))?;
    Ok(())
}
