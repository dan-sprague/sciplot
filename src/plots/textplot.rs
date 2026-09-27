//! `text`: text annotations at data positions (Makie's `text!`).
//!
//! Provenance: defaults follow Makie 0.24.14 `src/basic_plots.jl` (`@recipe Text`:
//! `align = (:left, :bottom)`, `rotation = 0`, `offset = (0, 0)`). MIT licensed; see
//! THIRD_PARTY_NOTICES.md.

use super::{PlotImpl, PlotKind, add_to_axis, plot_common, point_bounds};
use crate::attrs::attributes;
use crate::color::Color;
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{GlyphsPrim, Prim};
use crate::style::{HAlign, VAlign};
use crate::text::{Font, RichText};
use crate::transform::Scale;
use std::sync::Arc;

/// A text-annotation handle (Makie's `Text` plot).
#[derive(Clone)]
pub struct TextPlot {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct TextState {
    pub pos: Arc<Vec<[f64; 2]>>,
    pub texts: Arc<Vec<RichText>>,
    pub attrs: TextAttrs,
}

attributes! {
    TextPlot(TextAttrs, TextResolved, TextTheme) via with_attrs {
        fontsize: f64 = |g| g.fontsize, STYLE;
        font: Font = |_| Font::Regular, STYLE;
        color: Color = |g| g.textcolor, STYLE;
        /// Which point of the text box sits at the position (Makie default `(:left, :bottom)`).
        align: (HAlign, VAlign) = |_| (HAlign::Left, VAlign::Bottom), STYLE;
        /// Rotation in radians (counter-clockwise).
        rotation: f64 = |_| 0.0, STYLE;
        /// Offset in figure units (x right, y up).
        offset: [f64; 2] = |_| [0.0, 0.0], STYLE;
    }
}

crate::attrs::conv_identity!((HAlign, VAlign));

plot_common!(TextPlot);

/// Text for each position: one string for all, or one per position.
pub trait IntoTexts {
    #[doc(hidden)]
    fn into_texts(self) -> Vec<RichText>;
}
impl IntoTexts for &str {
    fn into_texts(self) -> Vec<RichText> {
        vec![self.into()]
    }
}
impl IntoTexts for String {
    fn into_texts(self) -> Vec<RichText> {
        vec![self.into()]
    }
}
impl IntoTexts for RichText {
    fn into_texts(self) -> Vec<RichText> {
        vec![self]
    }
}
impl IntoTexts for &[&str] {
    fn into_texts(self) -> Vec<RichText> {
        self.iter().map(|s| (*s).into()).collect()
    }
}
impl<const N: usize> IntoTexts for [&str; N] {
    fn into_texts(self) -> Vec<RichText> {
        self.iter().map(|s| (*s).into()).collect()
    }
}
impl IntoTexts for Vec<String> {
    fn into_texts(self) -> Vec<RichText> {
        self.into_iter().map(RichText::from).collect()
    }
}
impl IntoTexts for Vec<RichText> {
    fn into_texts(self) -> Vec<RichText> {
        self
    }
}

impl PlotImpl for TextState {
    fn cycle_group(&self) -> &'static str {
        "text"
    }

    fn color_is_auto(&self, _theme: &crate::theme::Theme) -> bool {
        false
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        point_bounds(&self.pos, xs, ys)
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.text, ctx.g);
        let mut glyphs = Vec::new();
        for (i, p) in self.pos.iter().enumerate() {
            let Some(t) = self.texts.get(if self.texts.len() == 1 { 0 } else { i }) else { continue };
            let Some(u) = ctx.to_units(p[0], p[1]) else { continue };
            let l = crate::text::layout(t, r.fontsize, r.font, r.color);
            let anchor = [u[0] + r.offset[0], u[1] - r.offset[1]];
            glyphs.extend(crate::text::place(&l, anchor, (r.align.0.frac(), r.align.1.frac()), r.rotation));
        }
        if !glyphs.is_empty() {
            ctx.push_figure(Prim::Glyphs(GlyphsPrim { glyphs }));
        }
    }
}

impl TextPlot {
    fn with_attrs(&self, f: impl FnOnce(&mut TextAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Text(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    /// Replaces positions and texts.
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D, texts: impl IntoTexts) -> TextPlot {
        let pos = Arc::new(super::zip_xy("TextPlot::set_data", x.to_vec_f64(), y.to_vec_f64()));
        let texts = Arc::new(texts.into_texts());
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Text(s) = &mut p.kind {
                s.pos = pos;
                s.texts = texts;
                p.data_rev += 1;
            }
        });
        self.clone()
    }
}

impl crate::Axis {
    /// Makie's `text!(ax, x, y; text = ...)`: one string for all positions or one per position.
    #[track_caller]
    pub fn text(&self, x: impl Data1D, y: impl Data1D, texts: impl IntoTexts) -> TextPlot {
        let pos = super::zip_xy("text", x.to_vec_f64(), y.to_vec_f64());
        let texts = texts.into_texts();
        assert!(
            texts.len() == 1 || texts.len() == pos.len(),
            "text: {} strings for {} positions (give one string or one per position)",
            texts.len(),
            pos.len()
        );
        let st = TextState { pos: Arc::new(pos), texts: Arc::new(texts), attrs: TextAttrs::default() };
        TextPlot { sh: self.sh.clone(), id: add_to_axis(self, PlotKind::Text(st)) }
    }
}
