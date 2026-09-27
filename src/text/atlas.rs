//! CPU side of the glyph atlas: grayscale coverage rasterization with ab_glyph, a shelf packer,
//! and the list of pending texture uploads. The GPU texture lives in
//! `render::gpu::pipelines::glyph`, which drains [`Atlas::take_uploads`] every frame.
//!
//! Glyphs are rasterized at their exact device-pixel size (quantized to 1/4 px) and at one of
//! four horizontal subpixel phases, like Cairo's glyph cache, so text drawn with nearest sampling
//! at a snapped baseline looks like Cairo's grayscale antialiasing.

use super::{Font, faces};
use ab_glyph::{Font as _, Glyph, GlyphId, PxScale, point};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};

/// Horizontal subpixel phases per pixel.
pub(crate) const SUBPIXEL_BINS: u32 = 4;
/// Glyph sizes are quantized to 1/`SIZE_STEPS` device pixel.
pub(crate) const SIZE_STEPS: f32 = 4.0;
/// Zero border around every glyph bitmap so edges sample to zero coverage.
pub(crate) const PAD: u32 = 1;

/// Identity of one rasterized glyph bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub font: Font,
    pub glyph: u16,
    /// Font size (Makie `fontsize`, the em size) in device pixels × `SIZE_STEPS`.
    pub size_q: u32,
    /// Horizontal subpixel phase `0..SUBPIXEL_BINS` (the origin sits `bin / SUBPIXEL_BINS` px
    /// right of a pixel boundary).
    pub bin: u8,
}

impl GlyphKey {
    /// The key for a glyph of em size `size_px` device pixels at subpixel phase `bin`.
    pub fn new(font: Font, glyph: u16, size_px: f32, bin: u8) -> GlyphKey {
        GlyphKey { font, glyph, size_q: (size_px * SIZE_STEPS).round().max(0.0) as u32, bin }
    }

    /// The quantized em size in device pixels.
    pub fn size_px(&self) -> f32 {
        self.size_q as f32 / SIZE_STEPS
    }
}

/// Where a glyph bitmap sits in the atlas. The slot includes the `PAD` border; `w == 0` means the
/// glyph has no ink (a space).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Slot {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Top-left of the slot relative to the glyph origin (the pixel boundary left of the origin
    /// on the baseline), device pixels, y down.
    pub off: [i32; 2],
}

/// A rectangle of R8 texels to copy into the atlas texture.
#[derive(Clone, Debug)]
pub(crate) struct Upload {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

/// What [`Atlas::ensure`] had to do to fit the requested glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Glyphs were added in free space (or were already present).
    None,
    /// The atlas grew from the given size; existing slots kept their positions.
    Grew(u32),
    /// The atlas was cleared and only this frame's glyphs were re-rasterized.
    Reset,
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

struct Entry {
    slot: Slot,
    frame: u64,
}

/// A square R8 coverage atlas with shelf packing.
pub(crate) struct Atlas {
    size: u32,
    max: u32,
    shelves: Vec<Shelf>,
    next_y: u32,
    map: HashMap<GlyphKey, Entry>,
    pending: Vec<Upload>,
    frame: u64,
}

/// A glyph bitmap with its placement relative to the origin (padding included).
struct Bitmap {
    w: u32,
    h: u32,
    off: [i32; 2],
    data: Vec<u8>,
}

/// The ab_glyph scale for an em size of `size_px`: ab_glyph's `PxScale` is the
/// ascent − descent height, while Makie's fontsize is the em size.
pub(crate) fn px_scale(font: Font, size_px: f32) -> PxScale {
    let face = faces().get(font);
    let upem = face.units_per_em().unwrap_or(1000.0);
    PxScale::from(size_px * face.height_unscaled() / upem)
}

/// Rasterizes `key` into a padded coverage bitmap (`None` for glyphs without ink).
fn rasterize(key: GlyphKey) -> Option<Bitmap> {
    let face = faces().get(key.font);
    let glyph = Glyph {
        id: GlyphId(key.glyph),
        scale: px_scale(key.font, key.size_px()),
        position: point(key.bin as f32 / SUBPIXEL_BINS as f32, 0.0),
    };
    let outlined = face.outline_glyph(glyph)?;
    let b = outlined.px_bounds();
    let (gw, gh) = (b.width() as u32, b.height() as u32);
    if gw == 0 || gh == 0 {
        return None;
    }
    let (w, h) = (gw + 2 * PAD, gh + 2 * PAD);
    let mut data = vec![0u8; (w * h) as usize];
    outlined.draw(|x, y, c| {
        let i = ((y + PAD) * w + x + PAD) as usize;
        if let Some(p) = data.get_mut(i) {
            *p = (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    });
    Some(Bitmap { w, h, off: [b.min.x as i32 - PAD as i32, b.min.y as i32 - PAD as i32], data })
}

impl Atlas {
    /// An empty atlas of `size`² texels that may grow up to `max`².
    pub fn new(size: u32, max: u32) -> Atlas {
        Atlas {
            size,
            max: max.max(size),
            shelves: Vec::new(),
            next_y: 0,
            map: HashMap::new(),
            pending: Vec::new(),
            frame: 0,
        }
    }

    /// Current edge length in texels.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Number of cached glyphs.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Starts a new frame: glyphs used from now on belong to it and survive eviction.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The slot of a glyph added by [`Atlas::ensure`] (`None` if it has no ink or did not fit).
    pub fn get(&self, key: &GlyphKey) -> Option<Slot> {
        self.map.get(key).map(|e| e.slot).filter(|s| s.w > 0)
    }

    /// Texel rectangles written since the last call.
    pub fn take_uploads(&mut self) -> Vec<Upload> {
        std::mem::take(&mut self.pending)
    }

    /// Makes every glyph in `keys` resident. When the atlas is full it first grows (up to `max`),
    /// then evicts every glyph not used in the current frame and re-rasterizes this frame's set,
    /// so a frame never draws with missing glyphs (unless one frame alone overflows `max`²).
    pub fn ensure(&mut self, keys: impl IntoIterator<Item = GlyphKey>) -> Change {
        let mut change = Change::None;
        let frame = self.frame;
        for key in keys {
            if let Some(e) = self.map.get_mut(&key) {
                e.frame = frame;
                continue;
            }
            let Some(bm) = rasterize(key) else {
                self.map.insert(key, Entry { slot: Slot::default(), frame });
                continue;
            };
            loop {
                if let Some((x, y)) = self.alloc(bm.w, bm.h) {
                    self.place(key, &bm, x, y);
                    break;
                }
                if self.size < self.max {
                    if change == Change::None {
                        change = Change::Grew(self.size);
                    }
                    self.size = (self.size * 2).min(self.max);
                } else if change != Change::Reset {
                    self.reset_to_frame();
                    change = Change::Reset;
                } else {
                    crate::warn_once("glyph atlas full: some text in this frame is not drawn");
                    self.map.insert(key, Entry { slot: Slot::default(), frame });
                    break;
                }
            }
        }
        change
    }

    fn place(&mut self, key: GlyphKey, bm: &Bitmap, x: u32, y: u32) {
        self.pending.push(Upload { x, y, w: bm.w, h: bm.h, data: bm.data.clone() });
        let slot = Slot { x, y, w: bm.w, h: bm.h, off: bm.off };
        self.map.insert(key, Entry { slot, frame: self.frame });
    }

    /// Clears the packer and re-rasterizes the glyphs used in the current frame, tallest first.
    fn reset_to_frame(&mut self) {
        let frame = self.frame;
        let mut keep: Vec<(GlyphKey, Bitmap)> = self
            .map
            .iter()
            .filter(|(_, e)| e.frame == frame && e.slot.w > 0)
            .filter_map(|(k, _)| rasterize(*k).map(|b| (*k, b)))
            .collect();
        keep.sort_by_key(|(_, b)| std::cmp::Reverse(b.h));
        self.map.retain(|_, e| e.frame == frame && e.slot.w == 0);
        self.shelves.clear();
        self.next_y = 0;
        self.pending.clear();
        for (k, b) in keep {
            match self.alloc(b.w, b.h) {
                Some((x, y)) => self.place(k, &b, x, y),
                None => crate::warn_once("glyph atlas full: some text in this frame is not drawn"),
            }
        }
    }

    /// Shelf packing: the lowest-waste shelf that fits, else a new shelf.
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.size || h > self.size {
            return None;
        }
        let size = self.size;
        let best = self
            .shelves
            .iter_mut()
            .filter(|s| s.h >= h && s.h <= h + h / 4 + 2 && s.x + w <= size)
            .min_by_key(|s| s.h - h);
        if let Some(s) = best {
            let x = s.x;
            s.x += w;
            return Some((x, s.y));
        }
        if self.next_y + h > size {
            return None;
        }
        let y = self.next_y;
        self.next_y += h;
        self.shelves.push(Shelf { y, h, x: w });
        Some((0, y))
    }
}

/// Logs a warning the first time a character without a glyph in the bundled fonts is laid out
/// (it is drawn as `.notdef`).
pub(crate) fn warn_missing_glyph(ch: char) {
    static SEEN: Mutex<Option<HashSet<char>>> = Mutex::new(None);
    if ch.is_control() {
        return;
    }
    if SEEN.lock().get_or_insert_with(HashSet::new).insert(ch) {
        let msg = format!("no glyph for {ch:?} (U+{:04X}) in the bundled fonts; drawing .notdef", ch as u32);
        log::warn!("sciplot: {msg}");
        eprintln!("sciplot warning: {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::ScaleFont as _;

    fn key(ch: char, size_px: f32, bin: u8) -> GlyphKey {
        let id = faces().get(Font::Regular).glyph_id(ch);
        GlyphKey::new(Font::Regular, id.0, size_px, bin)
    }

    #[test]
    fn scale_matches_layout_advance() {
        // Rasterization scale and text::layout must agree: advance = h_adv / upem * fontsize.
        let face = faces().get(Font::Regular);
        for ch in ['0', 'M', 'i', 'W', 'x'] {
            let id = face.glyph_id(ch);
            let size = 28.0;
            let layout_adv = face.h_advance_unscaled(id) / face.units_per_em().unwrap() * size;
            let scaled_adv = face.as_scaled(px_scale(Font::Regular, size)).h_advance(id);
            assert!((layout_adv - scaled_adv).abs() < 1e-3, "{ch}: {layout_adv} vs {scaled_adv}");
        }
    }

    #[test]
    fn raster_width_matches_outline_bounds() {
        // The ink of 'H' at 100 px spans its outline bbox scaled by fontsize / upem.
        let face = faces().get(Font::Regular);
        let id = face.glyph_id('H');
        let bb = face.outline(id).unwrap().bounds;
        let upem = face.units_per_em().unwrap();
        let bm = rasterize(key('H', 100.0, 0)).unwrap();
        let ink_w = (bm.w - 2 * PAD) as f32;
        let expect = (bb.max.x - bb.min.x) / upem * 100.0;
        assert!((ink_w - expect).abs() <= 2.0, "{ink_w} vs {expect}");
        // Cap height of TeX Gyre Heros is ~0.72 em; the glyph sits on the baseline.
        let ink_h = (bm.h - 2 * PAD) as f32;
        assert!((ink_h - 0.72 * 100.0).abs() <= 3.0, "{ink_h}");
        assert_eq!(bm.off[1] + bm.h as i32 - PAD as i32, 0, "baseline");
        // Full coverage in the stem.
        assert_eq!(bm.data.iter().copied().max(), Some(255));
    }

    #[test]
    fn subpixel_bins_shift_ink() {
        // The stem of 'l' spans x = 68..151 font units; at 20 px its left edge is at 1.36 px, plus
        // the bin's phase. Recover the edge from the coverage of the first inked column.
        let left_edge = |bin: u8| {
            let b = rasterize(key('l', 20.0, bin)).unwrap();
            let row = (b.h / 2 * b.w) as usize;
            let row = &b.data[row..row + b.w as usize];
            let (col, cov) = row.iter().enumerate().find(|(_, v)| **v > 0).unwrap();
            (col as i32 + b.off[0]) as f32 + 1.0 - *cov as f32 / 255.0
        };
        for bin in 0..4u8 {
            let expect = 1.36 + bin as f32 / 4.0;
            assert!((left_edge(bin) - expect).abs() < 0.01, "bin {bin}: {} vs {expect}", left_edge(bin));
        }
    }

    #[test]
    fn notdef_has_ink() {
        // Missing characters map to glyph 0, which the bundled fonts draw as a box.
        assert_eq!(faces().get(Font::Regular).glyph_id('∫').0, 0);
        let b = rasterize(GlyphKey::new(Font::Regular, 0, 20.0, 0)).expect(".notdef outline");
        assert!(b.w > 2 * PAD && b.h > 2 * PAD);
    }

    #[test]
    fn space_has_no_slot() {
        let mut a = Atlas::new(64, 64);
        a.begin_frame();
        assert_eq!(a.ensure([key(' ', 20.0, 0)]), Change::None);
        assert!(a.get(&key(' ', 20.0, 0)).is_none());
        assert!(a.take_uploads().is_empty());
    }

    #[test]
    fn grows_then_resets_keeping_frame_glyphs() {
        let mut a = Atlas::new(64, 256);
        a.begin_frame();
        let first: Vec<GlyphKey> = "ABCDEFGH".chars().map(|c| key(c, 24.0, 0)).collect();
        a.ensure(first.iter().copied());
        let many: Vec<GlyphKey> =
            "IJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".chars().map(|c| key(c, 24.0, 0)).collect();
        assert_eq!(a.ensure(many.iter().copied()), Change::Grew(64));
        assert_eq!(a.size(), 256);
        // A new frame with big glyphs overflows the max size: old glyphs are evicted, and every
        // glyph of this frame is resident.
        a.begin_frame();
        let big: Vec<GlyphKey> = "0123456789".chars().map(|c| key(c, 100.0, 1)).collect();
        let reused = key('A', 24.0, 0);
        let frame: Vec<GlyphKey> = std::iter::once(reused).chain(big.iter().copied()).collect();
        assert_eq!(a.ensure(frame.iter().copied()), Change::Reset);
        for k in &frame {
            assert!(a.get(k).is_some(), "{k:?}");
        }
        assert!(a.get(&key('z', 24.0, 0)).is_none());
        // Slots never overlap.
        let slots: Vec<Slot> = frame.iter().filter_map(|k| a.get(k)).collect();
        for (i, s) in slots.iter().enumerate() {
            assert!(s.x + s.w <= a.size() && s.y + s.h <= a.size());
            for t in &slots[i + 1..] {
                let apart = s.x + s.w <= t.x || t.x + t.w <= s.x || s.y + s.h <= t.y || t.y + t.h <= s.y;
                assert!(apart, "{s:?} overlaps {t:?}");
            }
        }
        // Uploads after a reset cover exactly the resident glyphs.
        assert_eq!(a.take_uploads().len(), frame.len());
    }
}
