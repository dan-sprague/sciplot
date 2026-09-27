//! Building rich text: the opt-in [`tex`] mini-markup, the [`rich!`](crate::rich) macro's
//! [`IntoSpans`] conversion, and span styling helpers.
//!
//! Provenance: the parser is original. Its script geometry (0.66× size, baseline +0.4 / −0.25 of
//! the enclosing size, compounding when nested) follows Makie 0.24.14 `src/basic_recipes/text.jl`
//! (`new_glyphstate`), and the U+2212 minus in scripts follows `src/tick_format.jl` (`MINUS_SIGN`).
//! MIT licensed; see THIRD_PARTY_NOTICES.md.

use super::{Font, RichText, TextSpan, faces};
use crate::color::IntoColor;
use ab_glyph::Font as _;
use std::iter::Peekable;
use std::str::Chars;

impl TextSpan {
    /// Sets the span's color: `superscript("2").color(RED)`.
    pub fn color(mut self, c: impl IntoColor) -> TextSpan {
        self.color = Some(c.into_color());
        self
    }

    /// Sets the span's font: `TextSpan::from("bold").font(Font::Bold)`.
    pub fn font(mut self, f: Font) -> TextSpan {
        self.font = Some(f);
        self
    }
}

/// Anything the [`rich!`](crate::rich) macro accepts: strings, [`TextSpan`]s (from
/// [`superscript`](super::superscript), [`subscript`](super::subscript), `.color(..)`) and whole
/// [`RichText`]s (e.g. from [`tex`]).
pub trait IntoSpans {
    /// Appends `self` as spans.
    fn push_into(self, spans: &mut Vec<TextSpan>);
}

impl IntoSpans for TextSpan {
    fn push_into(self, spans: &mut Vec<TextSpan>) {
        spans.push(self);
    }
}
impl IntoSpans for &str {
    fn push_into(self, spans: &mut Vec<TextSpan>) {
        spans.push(TextSpan::plain(self));
    }
}
impl IntoSpans for String {
    fn push_into(self, spans: &mut Vec<TextSpan>) {
        spans.push(TextSpan::plain(self));
    }
}
impl IntoSpans for &String {
    fn push_into(self, spans: &mut Vec<TextSpan>) {
        spans.push(TextSpan::plain(self.clone()));
    }
}
impl IntoSpans for RichText {
    fn push_into(self, spans: &mut Vec<TextSpan>) {
        spans.extend(self.spans);
    }
}

/// Makie's script geometry: size factor and baseline shifts (em of the enclosing text).
const SCRIPT_SCALE: f32 = 0.66;
const SUP_SHIFT: f32 = 0.4;
const SUB_SHIFT: f32 = -0.25;

/// `\name` -> character. Every entry has a glyph in the bundled TeX Gyre Heros Makie Regular
/// (checked by a unit test); anything else stays literal.
const SYMBOLS: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'ϑ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("omicron", 'ο'),
    ("pi", 'π'),
    ("varpi", 'ϖ'),
    ("rho", 'ρ'),
    ("varrho", 'ϱ'),
    ("sigma", 'σ'),
    ("varsigma", 'ς'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'ϕ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Alpha", 'Α'),
    ("Beta", 'Β'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Epsilon", 'Ε'),
    ("Zeta", 'Ζ'),
    ("Eta", 'Η'),
    ("Theta", 'Θ'),
    ("Iota", 'Ι'),
    ("Kappa", 'Κ'),
    ("Lambda", 'Λ'),
    ("Mu", 'Μ'),
    ("Nu", 'Ν'),
    ("Xi", 'Ξ'),
    ("Omicron", 'Ο'),
    ("Pi", 'Π'),
    ("Rho", 'Ρ'),
    ("Sigma", 'Σ'),
    ("Tau", 'Τ'),
    ("Upsilon", 'Υ'),
    ("Phi", 'Φ'),
    ("Chi", 'Χ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("pm", '±'),
    ("mp", '∓'),
    ("times", '×'),
    ("cdot", '·'),
    ("div", '÷'),
    ("infty", '∞'),
    ("partial", '∂'),
    ("degree", '°'),
    ("approx", '≈'),
    ("neq", '≠'),
    ("ne", '≠'),
    ("leq", '≤'),
    ("le", '≤'),
    ("geq", '≥'),
    ("ge", '≥'),
    ("sqrt", '√'),
    ("sum", '∑'),
    ("ell", 'ℓ'),
    ("AA", 'Å'),
    ("to", '→'),
    ("rightarrow", '→'),
    ("leftarrow", '←'),
    ("uparrow", '↑'),
    ("downarrow", '↓'),
    ("ldots", '…'),
    ("dots", '…'),
    ("bullet", '•'),
    ("dagger", '†'),
];

/// The character for `\name`, if the bundled font can draw it.
fn symbol(name: &str) -> Option<char> {
    let (_, c) = SYMBOLS.iter().find(|(n, _)| *n == name)?;
    (faces().get(Font::Regular).glyph_id(*c).0 != 0).then_some(*c)
}

/// Font switches: `\mathbf{..}` etc.; `None` keeps the current font (`\mathrm`, `\text`).
fn font_command(name: &str) -> Option<Option<Font>> {
    match name {
        "mathrm" | "text" | "textrm" | "mathsf" | "textsf" | "operatorname" => Some(None),
        "mathbf" | "textbf" => Some(Some(Font::Bold)),
        "mathit" | "textit" | "emph" => Some(Some(Font::Italic)),
        _ => None,
    }
}

fn combine(a: Option<Font>, b: Font) -> Font {
    match (a, b) {
        (Some(Font::Bold | Font::BoldItalic), Font::Italic) | (Some(Font::Italic | Font::BoldItalic), Font::Bold) => {
            Font::BoldItalic
        }
        _ => b,
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Style {
    scale: f32,
    shift: f32,
    font: Option<Font>,
    script: bool,
}

struct Parser<'a> {
    it: Peekable<Chars<'a>>,
    spans: Vec<TextSpan>,
    last: Option<Style>,
}

impl Parser<'_> {
    fn emit(&mut self, c: char, st: Style) {
        let c = if st.script && c == '-' { '\u{2212}' } else { c };
        if self.last == Some(st)
            && let Some(s) = self.spans.last_mut()
        {
            s.text.push(c);
            return;
        }
        self.spans.push(TextSpan {
            text: c.to_string(),
            font: st.font,
            color: None,
            size_scale: st.scale,
            baseline_shift: st.shift,
            x_offset: 0.0,
        });
        self.last = Some(st);
    }

    fn emit_str(&mut self, s: &str, st: Style) {
        for c in s.chars() {
            self.emit(c, st);
        }
    }

    /// Parses until the end of input, or a closing brace when `braced`.
    fn group(&mut self, st: Style, braced: bool) {
        while let Some(c) = self.it.next() {
            match c {
                '}' if braced => return,
                '{' => self.group(st, true),
                '$' => {}
                '^' | '_' => {
                    let shift = if c == '^' { SUP_SHIFT } else { SUB_SHIFT };
                    let inner = Style {
                        scale: st.scale * SCRIPT_SCALE,
                        shift: st.shift + shift * st.scale,
                        script: true,
                        ..st
                    };
                    match self.it.next() {
                        Some('{') => self.group(inner, true),
                        Some('\\') => self.command(inner),
                        Some(a) => self.emit(a, inner),
                        None => self.emit(c, st),
                    }
                }
                '\\' => self.command(st),
                c => self.emit(c, st),
            }
        }
    }

    /// After a backslash: an escape, a spacing command, a font switch or a symbol.
    fn command(&mut self, st: Style) {
        let mut name = String::new();
        while let Some(&c) = self.it.peek() {
            if !c.is_ascii_alphabetic() {
                break;
            }
            name.push(c);
            self.it.next();
        }
        if name.is_empty() {
            match self.it.next() {
                Some(c @ ('^' | '_' | '{' | '}' | '\\' | '$')) => self.emit(c, st),
                Some(',' | ':' | ';' | ' ') => self.emit(' ', st),
                Some(c) => {
                    self.emit('\\', st);
                    self.emit(c, st);
                }
                None => self.emit('\\', st),
            }
            return;
        }
        if let Some(f) = font_command(&name) {
            let inner = Style { font: f.map(|f| combine(st.font, f)).or(st.font), ..st };
            // Braced argument, or a single character/command like TeX.
            match self.it.next() {
                Some('{') => self.group(inner, true),
                Some('\\') => self.command(inner),
                Some(c) => self.emit(c, inner),
                None => {}
            }
            return;
        }
        match symbol(&name) {
            Some(c) => {
                self.emit(c, st);
                self.skip_spaces_before_letter();
            }
            None => {
                self.emit('\\', st);
                self.emit_str(&name, st);
            }
        }
    }

    /// Like TeX, a space ending a command name is dropped when a letter follows (`\mu m` -> μm,
    /// `\Delta x` -> Δx); before anything else it is kept (`\alpha = 1` -> α = 1).
    fn skip_spaces_before_letter(&mut self) {
        let mut ahead = self.it.clone();
        while ahead.next_if_eq(&' ').is_some() {}
        if ahead.peek().is_some_and(|c| c.is_alphabetic()) {
            self.it = ahead;
        }
    }
}

/// Opt-in TeX-like mini-markup (plain strings are never parsed):
/// - `^{..}` / `_{..}` superscripts and subscripts (nestable), `^x` / `_x` for one character or
///   command; `-` inside scripts becomes the minus sign U+2212, as in Makie's tick labels;
/// - `\alpha`..`\omega`, `\Gamma`..`\Omega`, and symbols such as `\pm \times \cdot \infty
///   \partial \approx \leq \degree \to`, as far as the bundled font has the glyph (unknown
///   commands stay literal);
/// - `\mathrm{..}`, `\text{..}`, `\mathbf{..}`, `\mathit{..}`; `{..}` groups; `$` is ignored;
/// - escapes `\^ \_ \{ \} \\ \$`, and `\,` `\;` `\ ` for a space.
///
/// Spaces are kept as typed (text mode, not math mode), except that spaces after a symbol command
/// are dropped before a letter, as in TeX: `\mu m` gives μm and `\Delta x` gives Δx, while
/// `\alpha = 1` keeps its spaces. Use `\ ` to force a space.
///
/// ```
/// use sciplot::text::tex;
/// let t = tex("k^{-5/3}");
/// assert_eq!(t.plain_text(), "k\u{2212}5/3");
/// assert_eq!(t.spans[1].size_scale, 0.66);
/// ```
pub fn tex(s: &str) -> RichText {
    let mut p = Parser { it: s.chars().peekable(), spans: Vec::new(), last: None };
    p.group(Style { scale: 1.0, shift: 0.0, font: None, script: false }, false);
    RichText { spans: p.spans }
}

/// Shorthand for a plain span with a color: `rich!("a ", colored("b", RED))`.
pub fn colored(text: impl Into<String>, color: impl IntoColor) -> TextSpan {
    TextSpan::plain(text).color(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::text::{subscript, superscript};

    #[test]
    fn scripts() {
        let t = tex("k^{-5/3}");
        assert_eq!(t.spans.len(), 2);
        assert_eq!(t.spans[0].text, "k");
        assert_eq!(t.spans[0].size_scale, 1.0);
        assert_eq!(t.spans[1].text, "\u{2212}5/3");
        assert_eq!(t.spans[1].size_scale, 0.66);
        assert_eq!(t.spans[1].baseline_shift, 0.4);

        let t = tex("x_i + y^2 - 1");
        let texts: Vec<&str> = t.spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["x", "i", " + y", "2", " - 1"], "minus only inside scripts");
        assert_eq!(t.spans[1].baseline_shift, -0.25);
    }

    #[test]
    fn nested_scripts_follow_makie() {
        // Makie: a script inside a script scales again and shifts by 0.4 of the parent size.
        let t = tex("e^{x^2}");
        let s = &t.spans[2];
        assert_eq!(s.text, "2");
        assert!((s.size_scale - 0.66 * 0.66).abs() < 1e-6);
        assert!((s.baseline_shift - (0.4 + 0.4 * 0.66)).abs() < 1e-6);
    }

    #[test]
    fn commands_and_escapes() {
        assert_eq!(tex(r"\alpha + \Omega \pm \infty").plain_text(), "α + Ω ± ∞");
        assert_eq!(tex(r"k (\mu m^{-1}), \Delta x, \Delta\ x").plain_text(), "k (μm\u{2212}1), Δx, Δ x");
        assert_eq!(tex(r"\partial_t u = 3 \times 10^{-4}").plain_text(), "∂t u = 3 × 10\u{2212}4");
        assert_eq!(tex(r"a\^b\_c \{x\} \\").plain_text(), r"a^b_c {x} \");
        assert_eq!(tex(r"\notacommand").plain_text(), r"\notacommand");
        assert_eq!(tex(r"$n (m^{-3})$").plain_text(), "n (m\u{2212}3)");
        assert_eq!(tex(r"5\,\mathrm{mm}").plain_text(), "5 mm");
        let t = tex(r"x_\alpha");
        assert_eq!(t.spans[1].text, "α");
        assert_eq!(t.spans[1].size_scale, 0.66);
        let b = tex(r"\mathbf{v} = \mathit{\mathbf{w}}");
        assert_eq!(b.spans[0].font, Some(Font::Bold));
        assert_eq!(b.spans[1].font, None);
        assert_eq!(b.spans[2].font, Some(Font::BoldItalic));
    }

    #[test]
    fn lenient_on_malformed_input() {
        assert_eq!(tex("x^").plain_text(), "x^");
        assert_eq!(tex("x^{2").plain_text(), "x2");
        assert_eq!(tex("a}b").plain_text(), "a}b");
        assert_eq!(tex("\\").plain_text(), "\\");
        assert!(tex("").is_empty());
    }

    #[test]
    fn symbol_table_has_glyphs() {
        let face = faces().get(Font::Regular);
        let missing: Vec<&str> = SYMBOLS.iter().filter(|(_, c)| face.glyph_id(*c).0 == 0).map(|(n, _)| *n).collect();
        assert!(missing.is_empty(), "no glyph for {missing:?}");
    }

    #[test]
    fn rich_macro() {
        let r = crate::rich!("E = mc", superscript("2"), colored(" (exact)", Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(r.plain_text(), "E = mc2 (exact)");
        assert_eq!(r.spans[1].size_scale, 0.66);
        assert_eq!(r.spans[2].color, Some(Color::rgb(1.0, 0.0, 0.0)));
        let r = crate::rich!(String::from("a"), subscript("i").font(Font::Italic), tex("^{-1}"));
        assert_eq!(r.plain_text(), "ai\u{2212}1");
        assert_eq!(r.spans[1].font, Some(Font::Italic));
        assert_eq!(crate::rich!().spans.len(), 0);
    }
}
