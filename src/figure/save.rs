//! Export: `fig.save("out.png")`, `fig.save("out.svg")`, `fig.render_rgba(..)`.
//!
//! Provenance: defaults follow Makie 0.24.14 `src/theming.jl` (CairoMakie `px_per_unit = 2`,
//! `pt_per_unit = 0.75`) and `src/display.jl` (PNG resolution `dpi = 96 * px_per_unit`). MIT
//! licensed; see THIRD_PARTY_NOTICES.md.

use super::Figure;
use crate::error::{Error, Result};
use std::path::Path;

/// Export options. `Save::new()` gives Makie's CairoMakie defaults: 2 px per unit for bitmaps,
/// 0.75 pt per unit for vector output.
///
/// ```no_run
/// # use sciplot::prelude::*;
/// # let fig = Figure::new();
/// fig.save_with("fig.png", Save::dpi(300)).unwrap();   // px_per_unit = 300 / 96
/// ```
#[derive(Clone, Debug)]
pub struct Save {
    pub(crate) px_per_unit: f64,
    pub(crate) pt_per_unit: f64,
    pub(crate) background: Option<crate::color::Color>,
    pub(crate) cpu: bool,
}

impl Default for Save {
    fn default() -> Self {
        Save { px_per_unit: 2.0, pt_per_unit: 0.75, background: None, cpu: false }
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
    /// Rasterizes bitmaps on the CPU (the SVG through resvg) instead of the GPU. This is what
    /// happens automatically when no GPU adapter exists; `SCIPLOT_FORCE_CPU=1` forces it globally.
    /// Needs the `cpu-png` feature (on by default).
    pub fn cpu(mut self, v: bool) -> Save {
        self.cpu = v;
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

    /// The figure as PNG file bytes (no file I/O; works in the browser with the `cpu-png`
    /// feature). Same rendering as [`Figure::render_rgba`]; the PNG records the resolution.
    pub fn to_png_bytes(&self, opts: &Save) -> Result<Vec<u8>> {
        let img = self.render_rgba(opts)?;
        let mut out = Vec::new();
        encode_png(&mut out, &img, opts.px_per_unit)?;
        Ok(out)
    }

    /// Renders to an RGBA8 image at `opts.px_per_unit` (headless; any thread). Uses the GPU, or
    /// the CPU rasterizer when no GPU adapter exists or `opts.cpu(true)` is set. In the browser
    /// (wasm32) bitmaps always come from the CPU rasterizer: a GPU readback can't be awaited
    /// synchronously there.
    pub fn render_rgba(&self, opts: &Save) -> Result<RgbaImage> {
        let st = self.sh.snapshot();
        #[cfg(feature = "cpu-png")]
        let force_cpu = opts.cpu || crate::render::cpu::forced_by_env();
        #[cfg(not(feature = "cpu-png"))]
        let force_cpu = false;
        #[cfg(not(target_arch = "wasm32"))]
        if !force_cpu {
            match crate::render::gpu::gpu() {
                Ok(gpu) => {
                    let mut r = crate::render::gpu::Renderer::new(gpu);
                    let (mut dl, _) = crate::scene::build(&st, None, &mut r.scene);
                    if let Some(bg) = opts.background {
                        dl.background = bg;
                    }
                    let (width, height, data) = r.render_rgba(&dl, opts.px_per_unit)?;
                    return Ok(RgbaImage { width, height, data });
                }
                #[cfg(feature = "cpu-png")]
                Err(Error::NoGpuAdapter(e)) => {
                    log::info!("sciplot: no GPU adapter ({e})");
                    crate::warn_once("no GPU adapter found; rendering bitmaps on the CPU (resvg)");
                }
                Err(e) => return Err(e),
            }
        }
        #[cfg(feature = "cpu-png")]
        {
            let _ = force_cpu;
            let (mut dl, _) = crate::scene::build(&st, None, &mut crate::scene::SceneCache::new());
            if let Some(bg) = opts.background {
                dl.background = bg;
            }
            let (width, height, data) = crate::render::cpu::render_rgba(&dl, opts.px_per_unit)?;
            Ok(RgbaImage { width, height, data })
        }
        #[cfg(not(feature = "cpu-png"))]
        {
            let _ = (st, force_cpu, opts);
            Err(Error::Gpu("the CPU rasterizer needs the `cpu-png` feature".into()))
        }
    }

    /// Like [`Figure::to_png_bytes`], but renders on the GPU in the browser too: the readback is
    /// awaited instead of blocked on. Falls back to the CPU rasterizer (feature `cpu-png`) when no
    /// GPU adapter exists or `opts.cpu(true)` is set.
    ///
    /// In the browser the GPU context of a mounted canvas is reused (on WebGL2 the first one);
    /// before any figure is mounted a WebGPU context is created on demand.
    pub async fn to_png_bytes_async(&self, opts: &Save) -> Result<Vec<u8>> {
        let img = self.render_rgba_async(opts).await?;
        let mut out = Vec::new();
        encode_png(&mut out, &img, opts.px_per_unit)?;
        Ok(out)
    }

    /// Like [`Figure::render_rgba`], awaiting the GPU readback (works in the browser).
    pub async fn render_rgba_async(&self, opts: &Save) -> Result<RgbaImage> {
        #[cfg(feature = "cpu-png")]
        let force_cpu = opts.cpu || crate::render::cpu::forced_by_env();
        #[cfg(not(feature = "cpu-png"))]
        let force_cpu = false;
        if !force_cpu {
            match crate::render::gpu::gpu_async(None).await {
                Ok(gpu) => {
                    let st = self.sh.snapshot();
                    let mut r = crate::render::gpu::Renderer::new(gpu);
                    let (mut dl, _) = crate::scene::build(&st, None, &mut r.scene);
                    if let Some(bg) = opts.background {
                        dl.background = bg;
                    }
                    let (width, height, data) = r.render_rgba_async(&dl, opts.px_per_unit).await?;
                    return Ok(RgbaImage { width, height, data });
                }
                #[cfg(feature = "cpu-png")]
                Err(Error::NoGpuAdapter(e)) => log::info!("sciplot: no GPU adapter ({e}); rendering on the CPU"),
                Err(e) => return Err(e),
            }
        }
        #[cfg(feature = "cpu-png")]
        {
            let mut opts = opts.clone();
            opts.cpu = true;
            self.render_rgba(&opts)
        }
        #[cfg(not(feature = "cpu-png"))]
        Err(Error::Gpu("the CPU rasterizer needs the `cpu-png` feature".into()))
    }

    /// Browser: renders the figure as a PNG ([`Figure::to_png_bytes_async`]) and offers it as
    /// a download named `name`.
    #[cfg(target_arch = "wasm32")]
    pub async fn download_png(&self, name: &str, opts: &Save) -> Result<()> {
        use wasm_bindgen::JsCast;
        let bytes = self.to_png_bytes_async(opts).await?;
        let err = |e: wasm_bindgen::JsValue| Error::Encode(format!("{e:?}"));
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes.as_slice()));
        let props = web_sys::BlobPropertyBag::new();
        props.set_type("image/png");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &props).map_err(err)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(err)?;
        let doc = web_sys::window().and_then(|w| w.document()).ok_or_else(|| Error::Encode("no document".into()))?;
        let a: web_sys::HtmlAnchorElement = doc.create_element("a").map_err(err)?.unchecked_into();
        a.set_href(&url);
        a.set_download(name);
        a.click();
        web_sys::Url::revoke_object_url(&url).map_err(err)
    }

    /// The figure as a standalone SVG document (`width`/`height` in points from
    /// `opts.pt_per_unit`, `viewBox` in figure units). Text is drawn as glyph outlines.
    pub fn to_svg_string(&self, opts: &Save) -> Result<String> {
        let st = self.sh.snapshot();
        let (mut dl, _) = crate::scene::build(&st, None, &mut crate::scene::SceneCache::new());
        if let Some(bg) = opts.background {
            dl.background = bg;
        }
        let svg_opts = crate::render::svg::SvgOptions { pt_per_unit: opts.pt_per_unit, snap_ppu: None };
        Ok(crate::render::svg::document(&dl, &svg_opts))
    }
}

pub(crate) fn write_png(path: &Path, img: &RgbaImage, px_per_unit: f64) -> Result<()> {
    encode_png(std::io::BufWriter::new(std::fs::File::create(path)?), img, px_per_unit)
}

/// Encodes `img` as an sRGB RGBA8 PNG with its physical resolution (`px_per_unit` at 96 units
/// per inch).
pub(crate) fn encode_png(out: impl std::io::Write, img: &RgbaImage, px_per_unit: f64) -> Result<()> {
    let mut enc = png::Encoder::new(out, img.width, img.height);
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
