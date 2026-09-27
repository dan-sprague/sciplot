//! Rich text: a sequence of spans that can change font, color, size and baseline.
//!
//! Provenance: defaults and behaviour follow Makie 0.24.14 `src/richtext.jl` (`rich`,
//! `superscript`, `subscript`) and `src/basic_recipes/text.jl` (`new_glyphstate`: scripts at 0.66×
//! size, baseline +0.4 / −0.25 em, `offset` in units of the span's font size). MIT licensed; see
//! THIRD_PARTY_NOTICES.md.

use super::Font;
use crate::color::Color;

/// One run of text with uniform style.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextSpan {
    pub text: String,
    /// `None` inherits the surrounding font.
    pub font: Option<Font>,
    /// `None` inherits the surrounding color.
    pub color: Option<Color>,
    /// Multiplies the surrounding font size (superscripts use 0.66).
    pub size_scale: f32,
    /// Baseline shift in units of the surrounding font size (superscripts +0.4, subscripts -0.25).
    pub baseline_shift: f32,
    /// Extra advance before the span, in units of the span's own font size.
    pub x_offset: f32,
}

impl TextSpan {
    /// Unstyled text.
    pub fn plain(text: impl Into<String>) -> TextSpan {
        TextSpan { text: text.into(), size_scale: 1.0, ..Default::default() }
    }

    /// Makie's `offset` rich-text attribute: shifts the span by `(dx, dy)` in units of the span's
    /// own font size (`dy` up). Makie's log and scientific tick labels use
    /// `superscript(e).offset(0.1, 0.0)`.
    pub fn offset(mut self, dx: f32, dy: f32) -> TextSpan {
        self.x_offset += dx;
        self.baseline_shift += dy * self.size_scale;
        self
    }
}

impl From<&str> for TextSpan {
    fn from(s: &str) -> Self {
        TextSpan::plain(s)
    }
}
impl From<String> for TextSpan {
    fn from(s: String) -> Self {
        TextSpan::plain(s)
    }
}

/// Makie's `superscript("2")`: 0.66× size, raised 0.4 em, no horizontal offset (add one with
/// [`TextSpan::offset`]).
pub fn superscript(s: impl Into<String>) -> TextSpan {
    TextSpan { text: s.into(), size_scale: 0.66, baseline_shift: 0.4, ..Default::default() }
}

/// Makie's `subscript("2")`: 0.66× size, lowered 0.25 em, no horizontal offset.
pub fn subscript(s: impl Into<String>) -> TextSpan {
    TextSpan { text: s.into(), size_scale: 0.66, baseline_shift: -0.25, ..Default::default() }
}

/// A string of styled spans. Plain strings convert into a single span and are never parsed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RichText {
    pub spans: Vec<TextSpan>,
}

impl RichText {
    pub fn from_spans(spans: impl IntoIterator<Item = TextSpan>) -> RichText {
        RichText { spans: spans.into_iter().collect() }
    }
    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }
    /// The concatenated plain text.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

impl From<&str> for RichText {
    fn from(s: &str) -> Self {
        RichText { spans: vec![TextSpan::plain(s)] }
    }
}
impl From<String> for RichText {
    fn from(s: String) -> Self {
        RichText { spans: vec![TextSpan::plain(s)] }
    }
}
impl From<&String> for RichText {
    fn from(s: &String) -> Self {
        RichText { spans: vec![TextSpan::plain(s.clone())] }
    }
}
impl From<TextSpan> for RichText {
    fn from(s: TextSpan) -> Self {
        RichText { spans: vec![s] }
    }
}
