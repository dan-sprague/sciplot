//! `Label`: a text block in a grid cell (Makie's `Label`), e.g. super-titles and panel labels.

use super::{BlockCtx, BlockImpl, BlockLayout, block_common};
use crate::attrs::attributes;
use crate::color::Color;
use crate::figure::{BlockId, Dirty, FigShared, GridPosition};
use crate::layout::{BlockSize, Protrusion};
use crate::scene::drawlist::{Emitter, GlyphsPrim, Prim, Rect, Space};
use crate::style::{HAlign, VAlign};
use crate::text::{Font, RichText};
use std::sync::Arc;

/// A text label occupying a grid cell (Makie's `Label`).
///
/// ```
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// Axis::new(fig.at(1, 1));
/// Label::new(fig.at(Prepend, ..), "Super title").fontsize(20).font(Font::Bold);
/// Label::new(fig.at(1, 1).side(Side::TopLeft), "A").font(Font::Bold).halign(HAlign::Right);
/// ```
#[derive(Clone)]
pub struct Label {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: BlockId,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LabelState {
    pub attrs: LabelAttrs,
}

attributes! {
    Label(LabelAttrs, LabelResolved, LabelTheme) via with_attrs {
        text: RichText = |_| RichText::default(), LAYOUT;
        fontsize: f64 = |g| g.fontsize, LAYOUT;
        font: Font = |_| Font::Regular, LAYOUT;
        color: Color = |g| g.textcolor, STYLE;
        /// Rotation in radians (counter-clockwise).
        rotation: f64 = |_| 0.0, LAYOUT;
        /// `(left, right, bottom, top)` space around the text.
        padding: [f64; 4] = |_| [0.0; 4], LAYOUT;
        halign: HAlign = |_| HAlign::Center, LAYOUT;
        valign: VAlign = |_| VAlign::Center, LAYOUT;
        tellwidth: bool = |_| true, LAYOUT;
        tellheight: bool = |_| true, LAYOUT;
        visible: bool = |_| true, LAYOUT;
    }
}

block_common!(Label, Label, LabelState);

impl Label {
    fn with_attrs(&self, f: impl FnOnce(&mut LabelAttrs), dirty: u8) {
        self.with_state(dirty, |s| f(&mut s.attrs));
    }

    /// Makie's `Label(fig[r, c], text)`. Use `fig.at(Prepend, ..)` for a super-title row, or
    /// `.side(Side::TopLeft)` for a panel label next to a cell.
    pub fn new(pos: GridPosition, text: impl Into<RichText>) -> Label {
        let sh = pos.fig.sh.clone();
        let mut attrs = LabelAttrs::default();
        attrs.text = Some(text.into());
        let id = sh.update(Dirty::LAYOUT, |st| {
            let place = pos.resolve(st);
            st.add_block(place, super::Block::Label(Box::new(LabelState { attrs })))
        });
        Label { sh, id }
    }
}

impl LabelState {
    fn resolve(&self, ctx: &BlockCtx<'_>) -> LabelResolved {
        self.attrs.resolve(&ctx.st.theme.label, ctx.g)
    }
}

/// Width and height of text rotated by `angle`.
fn rotated_extent(w: f64, h: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (w * c.abs() + h * s.abs(), w * s.abs() + h * c.abs())
}

impl BlockImpl for LabelState {
    fn layout(&self, ctx: &BlockCtx<'_>) -> BlockLayout {
        let r = self.resolve(ctx);
        let (w, h) = if r.visible {
            let l = crate::text::layout(&r.text, r.fontsize, r.font, r.color);
            rotated_extent(l.width, l.height(), r.rotation)
        } else {
            (0.0, 0.0)
        };
        let [pl, pr, pb, pt] = r.padding;
        BlockLayout {
            protrusion: Protrusion::default(),
            width: BlockSize::Auto,
            height: BlockSize::Auto,
            autosize: [Some(w + pl + pr), Some(h + pb + pt)],
            tellwidth: r.tellwidth,
            tellheight: r.tellheight,
            halign: r.halign.frac(),
            valign: r.valign.frac(),
            ..Default::default()
        }
    }

    fn emit(&self, ctx: &BlockCtx<'_>, em: &mut Emitter, rect: Rect) {
        let r = self.resolve(ctx);
        if !r.visible || r.text.is_empty() {
            return;
        }
        let [pl, pr, pb, pt] = r.padding;
        let center = [rect.x + pl + 0.5 * (rect.w - pl - pr), rect.y + pt + 0.5 * (rect.h - pt - pb)];
        let l = crate::text::layout(&r.text, r.fontsize, r.font, r.color);
        let glyphs = crate::text::place(&l, center, (0.5, 0.5), r.rotation);
        em.push(30.0, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
    }
}
