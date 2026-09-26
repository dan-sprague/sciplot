//! Colormaps: 256-entry lookup tables sampled from ColorSchemes.jl exactly as Makie does, plus the
//! value encoding shared by every colormapped plot.

use super::{Color, IntoColor, cmap_data};
use crate::scene::drawlist::ColorMapping;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

/// Number of entries in every colormap lookup table.
pub const LUT_SIZE: usize = 256;

/// A continuous colormap: a 256-entry sRGB lookup table and a name.
///
/// Built-in maps are associated constants (`Colormap::MAGMA`) and can be named by their Makie
/// names (`"magma"`, `"RdBu"`; case-insensitive). Any list of colors becomes a colormap through
/// [`Colormap::from_colors`], and `.reversed()` flips a map (Makie's `Reverse(:viridis)`).
///
/// ```
/// use ezviz::color::Colormap;
/// let c = Colormap::VIRIDIS.sample(0.0);
/// assert!((c.r - 0.267004).abs() < 1e-6);
/// assert_eq!(Colormap::named("magma"), Some(Colormap::MAGMA));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Colormap(Repr);

#[derive(Clone, Debug)]
enum Repr {
    /// A built-in table (index into `cmap_data::TABLES`).
    Builtin {
        id: u8,
        reversed: bool,
    },
    Custom {
        name: Arc<str>,
        lut: Arc<Vec<Color>>,
    },
}

impl PartialEq for Repr {
    fn eq(&self, other: &Repr) -> bool {
        match (self, other) {
            (Repr::Builtin { id: a, reversed: ra }, Repr::Builtin { id: b, reversed: rb }) => a == b && ra == rb,
            (Repr::Custom { name: na, lut: a }, Repr::Custom { name: nb, lut: b }) => na == nb && a == b,
            _ => false,
        }
    }
}

const fn builtin(id: u8) -> Colormap {
    Colormap(Repr::Builtin { id, reversed: false })
}

impl Colormap {
    /// Makie's default: matplotlib's viridis.
    pub const VIRIDIS: Colormap = builtin(0);
    /// matplotlib's magma (black to light yellow through purple).
    pub const MAGMA: Colormap = builtin(1);
    /// matplotlib's inferno (black to yellow through red).
    pub const INFERNO: Colormap = builtin(2);
    /// matplotlib's plasma (blue to yellow through magenta).
    pub const PLASMA: Colormap = builtin(3);
    /// cividis (blue to yellow, color-vision-deficiency friendly).
    pub const CIVIDIS: Colormap = builtin(4);
    /// Google's turbo (an improved jet).
    pub const TURBO: Colormap = builtin(5);
    /// Makie's `:grays` (gray 5 % to 95 %).
    pub const GRAYS: Colormap = builtin(6);
    /// ColorBrewer `Blues` (white to dark blue).
    pub const BLUES: Colormap = builtin(7);
    /// ColorBrewer `Reds` (white to dark red).
    pub const REDS: Colormap = builtin(8);
    /// cmocean `balance` (diverging blue-white-red).
    pub const BALANCE: Colormap = builtin(9);
    /// ColorBrewer `RdBu` (diverging red-white-blue).
    pub const RDBU: Colormap = builtin(10);
    /// Moreland's `coolwarm` (diverging blue-red).
    pub const COOLWARM: Colormap = builtin(11);

    /// All built-in colormaps.
    pub const BUILTIN: [Colormap; 12] = [
        Colormap::VIRIDIS,
        Colormap::MAGMA,
        Colormap::INFERNO,
        Colormap::PLASMA,
        Colormap::CIVIDIS,
        Colormap::TURBO,
        Colormap::GRAYS,
        Colormap::BLUES,
        Colormap::REDS,
        Colormap::BALANCE,
        Colormap::RDBU,
        Colormap::COOLWARM,
    ];

    /// A colormap interpolating linearly (in sRGB-encoded space, like Makie) through `colors`,
    /// resampled to 256 entries.
    ///
    /// # Panics
    /// If `colors` is empty.
    #[track_caller]
    pub fn from_colors(colors: &[Color]) -> Colormap {
        assert!(!colors.is_empty(), "Colormap::from_colors: at least one color is required");
        let lut = (0..LUT_SIZE).map(|i| interpolate(colors, i as f32 / (LUT_SIZE - 1) as f32)).collect();
        Colormap(Repr::Custom { name: "custom".into(), lut: Arc::new(lut) })
    }

    /// Looks up a built-in colormap by its Makie name (case-insensitive), e.g. `"magma"`, `"RdBu"`.
    pub fn named(name: &str) -> Option<Colormap> {
        let key = name.trim();
        let tables = &cmap_data::TABLES;
        tables
            .iter()
            .position(|(n, _)| *n == key)
            .or_else(|| tables.iter().position(|(n, _)| n.eq_ignore_ascii_case(key)))
            .map(|i| builtin(i as u8))
    }

    /// The same colormap in reverse order (Makie's `Reverse(cmap)`).
    pub fn reversed(&self) -> Colormap {
        match &self.0 {
            Repr::Builtin { id, reversed } => Colormap(Repr::Builtin { id: *id, reversed: !reversed }),
            Repr::Custom { name, lut } => {
                let name = match name.strip_suffix(" (reversed)") {
                    Some(n) => n.into(),
                    None => format!("{name} (reversed)").into(),
                };
                Colormap(Repr::Custom { name, lut: Arc::new(lut.iter().rev().copied().collect()) })
            }
        }
    }

    /// The name (`"viridis"`, `"RdBu"`, `"custom"`); reversed maps end in `" (reversed)"`.
    pub fn name(&self) -> String {
        match &self.0 {
            Repr::Builtin { id, reversed } => {
                let n = cmap_data::TABLES[*id as usize].0;
                if *reversed { format!("{n} (reversed)") } else { n.to_string() }
            }
            Repr::Custom { name, .. } => name.to_string(),
        }
    }

    /// The color at `t` in 0..=1 (clamped; NaN gives the first color), interpolated linearly
    /// between table entries like Makie's `interpolated_getindex` and the GPU lookup.
    pub fn sample(&self, t: f64) -> Color {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        interpolate(&self.lut(), t as f32)
    }

    /// The 256-entry lookup table. Built-in tables are shared process-wide, so the GPU uploads
    /// each one once.
    pub fn lut(&self) -> Arc<Vec<Color>> {
        match &self.0 {
            Repr::Custom { lut, .. } => lut.clone(),
            Repr::Builtin { id, reversed } => {
                static LUTS: [OnceLock<Arc<Vec<Color>>>; 2 * cmap_data::TABLES.len()] =
                    [const { OnceLock::new() }; 2 * cmap_data::TABLES.len()];
                let slot = 2 * *id as usize + *reversed as usize;
                LUTS[slot]
                    .get_or_init(|| {
                        let t = cmap_data::TABLES[*id as usize].1;
                        let it = t.iter().map(|c| Color::rgb(c[0], c[1], c[2]));
                        Arc::new(if *reversed { it.rev().collect() } else { it.collect() })
                    })
                    .clone()
            }
        }
    }

    /// The first color (Makie's default `lowclip`).
    pub fn first(&self) -> Color {
        self.sample(0.0)
    }

    /// The last color (Makie's default `highclip`).
    pub fn last(&self) -> Color {
        self.sample(1.0)
    }
}

impl Default for Colormap {
    fn default() -> Colormap {
        Colormap::VIRIDIS
    }
}

/// Linear interpolation through `colors` at `t` in 0..=1 (Makie's `interpolated_getindex`).
fn interpolate(colors: &[Color], t: f32) -> Color {
    let Some(last) = colors.len().checked_sub(1) else {
        return Color::TRANSPARENT;
    };
    let f = t.clamp(0.0, 1.0) * last as f32;
    let i = (f.floor() as usize).min(last);
    let j = (i + 1).min(last);
    colors[i].lerp(colors[j], f - i as f32)
}

/// Anything usable as a colormap: a [`Colormap`], a Makie name (`"magma"`), or a list of colors.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a colormap",
    label = "expected a Colormap (Colormap::MAGMA), a colormap name like \"magma\", or a list of colors"
)]
pub trait IntoColormap {
    /// Converts to a [`Colormap`].
    fn into_colormap(self) -> Colormap;
}

impl IntoColormap for Colormap {
    fn into_colormap(self) -> Colormap {
        self
    }
}
impl IntoColormap for &Colormap {
    fn into_colormap(self) -> Colormap {
        self.clone()
    }
}
impl IntoColormap for &str {
    /// # Panics
    /// On an unknown name, listing the closest built-in names.
    #[track_caller]
    fn into_colormap(self) -> Colormap {
        match Colormap::named(self) {
            Some(c) => c,
            None => panic!("{}", unknown_name(self)),
        }
    }
}
impl IntoColormap for String {
    #[track_caller]
    fn into_colormap(self) -> Colormap {
        self.as_str().into_colormap()
    }
}
impl IntoColormap for &[Color] {
    #[track_caller]
    fn into_colormap(self) -> Colormap {
        Colormap::from_colors(self)
    }
}
impl IntoColormap for Vec<Color> {
    #[track_caller]
    fn into_colormap(self) -> Colormap {
        Colormap::from_colors(&self)
    }
}
impl<const N: usize> IntoColormap for [Color; N] {
    #[track_caller]
    fn into_colormap(self) -> Colormap {
        Colormap::from_colors(&self)
    }
}

impl<C: IntoColormap> crate::attrs::Conv<Colormap> for C {
    #[track_caller]
    fn conv(self) -> Colormap {
        self.into_colormap()
    }
}

/// `lowclip` / `highclip`: a color, or `None` for the colormap's end color.
impl<C: IntoColor> crate::attrs::Conv<Option<Color>> for C {
    #[track_caller]
    fn conv(self) -> Option<Color> {
        Some(self.into_color())
    }
}
impl crate::attrs::Conv<Option<Color>> for Option<Color> {
    fn conv(self) -> Option<Color> {
        self
    }
}

/// `colorrange`: `(lo, hi)`, or `None` for automatic (the data's finite extrema).
impl<A: crate::Num, B: crate::Num> crate::attrs::Conv<Option<[f64; 2]>> for (A, B) {
    fn conv(self) -> Option<[f64; 2]> {
        Some([self.0.to_f64(), self.1.to_f64()])
    }
}
impl crate::attrs::Conv<Option<[f64; 2]>> for Option<[f64; 2]> {
    fn conv(self) -> Option<[f64; 2]> {
        self
    }
}
impl crate::attrs::Conv<Option<[f64; 2]>> for [f64; 2] {
    fn conv(self) -> Option<[f64; 2]> {
        Some(self)
    }
}

fn unknown_name(name: &str) -> String {
    let key = name.to_ascii_lowercase();
    let mut scored: Vec<(usize, &str)> = cmap_data::TABLES
        .iter()
        .map(|(n, _)| {
            let l = n.to_ascii_lowercase();
            let d = if l.contains(&key) || key.contains(&l) { 0 } else { levenshtein(&key, &l) };
            (d, *n)
        })
        .collect();
    scored.sort();
    let close: Vec<String> = scored.iter().filter(|(d, _)| *d <= 3).take(3).map(|(_, n)| format!("{n:?}")).collect();
    let all: Vec<&str> = cmap_data::TABLES.iter().map(|(n, _)| *n).collect();
    let hint = if close.is_empty() { String::new() } else { format!("did you mean {}? ", close.join(" or ")) };
    format!("unknown colormap {name:?}; {hint}available: {}", all.join(", "))
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + (ca != *cb) as usize).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Values stored for the GPU as f32 `(v - off) * k`, with `off`/`k` chosen from the f64 extrema so
/// that fields of any magnitude (1e40, 1e9 ± 1) keep their relative resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ValueEncoding {
    pub off: f64,
    pub k: f64,
    /// Finite f64 extrema of the raw values.
    pub extrema: Option<(f64, f64)>,
}

impl ValueEncoding {
    /// Maps the extrema to about -1..=1.
    pub fn new(extrema: Option<(f64, f64)>) -> ValueEncoding {
        let (off, k) = match extrema {
            Some((lo, hi)) => {
                let (mid, half) = (0.5 * lo + 0.5 * hi, 0.5 * hi - 0.5 * lo);
                if half > 0.0 && (1.0 / half).is_finite() { (mid, 1.0 / half) } else { (mid, 1.0) }
            }
            None => (0.0, 1.0),
        };
        ValueEncoding { off, k, extrema }
    }

    pub fn encode(&self, v: f64) -> f32 {
        let r = (v - self.off) * self.k;
        if r.is_nan() { f32::NAN } else { r.clamp(f32::MIN as f64, f32::MAX as f64) as f32 }
    }

    /// Makie's automatic colorrange (`distinct_extrema_nan`): the finite extrema, widened by ±0.5
    /// when they are equal; `(0, 1)` without finite values.
    pub fn auto_range(&self) -> [f64; 2] {
        match self.extrema {
            Some((lo, hi)) if lo == hi => [lo - 0.5, hi + 0.5],
            Some((lo, hi)) => [lo, hi],
            None => [0.0, 1.0],
        }
    }
}

/// Resolved colormap attributes of a plot.
pub(crate) struct MappingAttrs<'a> {
    pub colormap: &'a Colormap,
    pub colorrange: Option<[f64; 2]>,
    pub lowclip: Option<Color>,
    pub highclip: Option<Color>,
    pub nan_color: Color,
    pub alpha: f64,
}

impl MappingAttrs<'_> {
    /// The draw-list colormapping for values stored with `enc`.
    pub fn mapping(&self, enc: &ValueEncoding) -> ColorMapping {
        let [lo, hi] = self.colorrange.unwrap_or_else(|| enc.auto_range());
        ColorMapping {
            lut: self.colormap.lut(),
            range: [enc.encode(lo), enc.encode(hi)],
            lowclip: self.lowclip,
            highclip: self.highclip,
            nan_color: self.nan_color,
            alpha: self.alpha as f32,
        }
    }
}

/// Encoded f32 values for an `Arc<Vec<f64>>` of color values.
#[derive(Clone)]
pub(crate) struct EncodedValues {
    pub data: Arc<Vec<f32>>,
    pub enc: ValueEncoding,
    /// Unique per source vector (GPU cache revision).
    pub rev: u64,
}

/// Encodes `values` for the GPU, memoized on the vector's identity so unchanged values are
/// converted once, not every frame.
pub(crate) fn encoded_values(values: &Arc<Vec<f64>>) -> EncodedValues {
    // A live `Weak` pins the allocation, so pointer equality with it means the same vector.
    type Memo = HashMap<usize, (Weak<Vec<f64>>, EncodedValues)>;
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    let key = Arc::as_ptr(values) as usize;
    let memo = MEMO.get_or_init(Default::default);
    if let Some((w, e)) = memo.lock().get(&key)
        && w.strong_count() > 0
    {
        return e.clone();
    }
    let enc = ValueEncoding::new(crate::data::finite_extrema(values.iter().copied()));
    let e = EncodedValues {
        data: Arc::new(values.iter().map(|v| enc.encode(*v)).collect()),
        enc,
        rev: crate::figure::next_uid(),
    };
    let mut m = memo.lock();
    m.retain(|_, (w, _)| w.strong_count() > 0);
    m.insert(key, (Arc::downgrade(values), e.clone()));
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(c: Color, rgb: [f32; 3]) -> bool {
        (c.r - rgb[0]).abs() < 1e-6 && (c.g - rgb[1]).abs() < 1e-6 && (c.b - rgb[2]).abs() < 1e-6
    }

    /// Endpoints of `Makie.to_colormap(name)` (ColorSchemes.jl 3.x, Makie 0.24.14).
    #[test]
    fn endpoints_match_colorschemes() {
        let cases: [(Colormap, [f32; 3], [f32; 3]); 12] = [
            (Colormap::VIRIDIS, [0.267004, 0.004874, 0.329415], [0.993248, 0.906157, 0.143936]),
            (Colormap::MAGMA, [0.001462, 0.000466, 0.013866], [0.987053, 0.991438, 0.749504]),
            (Colormap::INFERNO, [0.001462, 0.000466, 0.013866], [0.988362, 0.998364, 0.644924]),
            (Colormap::PLASMA, [0.050383, 0.029803, 0.527975], [0.940015, 0.975158, 0.131326]),
            (Colormap::CIVIDIS, [0.0, 0.1262, 0.3015], [1.0, 0.9169, 0.2731]),
            (Colormap::TURBO, [0.18995, 0.07176, 0.23217], [0.4796, 0.01583, 0.01055]),
            (Colormap::GRAYS, [0.05, 0.05, 0.05], [0.95, 0.95, 0.95]),
            (Colormap::BLUES, [0.969, 0.984, 1.0], [0.031, 0.188, 0.42]),
            (Colormap::REDS, [1.0, 0.961, 0.941], [0.404, 0.0, 0.051]),
            (Colormap::BALANCE, [0.093176305, 0.11117333, 0.2615124], [0.23605636, 0.03529748, 0.069437444]),
            (Colormap::RDBU, [0.404, 0.0, 0.122], [0.02, 0.188, 0.38]),
            (Colormap::COOLWARM, [0.2298057, 0.29871798, 0.75368315], [0.70567316, 0.01555616, 0.1502328]),
        ];
        for (cm, first, last) in cases {
            assert_eq!(cm.lut().len(), LUT_SIZE);
            assert!(close(cm.first(), first), "{} first {:?}", cm.name(), cm.first());
            assert!(close(cm.last(), last), "{} last {:?}", cm.name(), cm.last());
            assert!(close(cm.reversed().first(), last));
            assert_eq!(cm.reversed().reversed(), cm);
        }
    }

    #[test]
    fn interpolated_like_makie() {
        // Blues has 9 ColorBrewer colors; the 5th, #6baed6 = (0.42, 0.682, 0.839), sits at t = 0.5.
        // The 256-entry table reproduces the piecewise-linear gradient to within its resolution.
        let c = Colormap::BLUES.sample(0.5);
        assert!((c.r - 0.42).abs() < 2e-3 && (c.g - 0.682).abs() < 2e-3 && (c.b - 0.839).abs() < 2e-3, "{c:?}");
        let bw = Colormap::from_colors(&[Color::rgb(0.0, 0.0, 0.0), Color::rgb(1.0, 1.0, 1.0)]);
        assert!(close(bw.sample(0.25), [0.25, 0.25, 0.25]));
        assert!(close(bw.sample(-3.0), [0.0; 3]) && close(bw.sample(f64::NAN), [0.0; 3]));
        assert!(Arc::ptr_eq(&Colormap::MAGMA.lut(), &Colormap::MAGMA.lut()));
    }

    #[test]
    fn names() {
        assert_eq!(Colormap::named("rdbu"), Some(Colormap::RDBU));
        assert_eq!("magma".into_colormap(), Colormap::MAGMA);
        assert_eq!(Colormap::MAGMA.reversed().name(), "magma (reversed)");
        let msg = unknown_name("magam");
        assert!(msg.contains("did you mean \"magma\""), "{msg}");
        let r = std::panic::catch_unwind(|| "virdis".into_colormap());
        assert!(r.is_err());
    }

    #[test]
    fn value_encoding() {
        let e = ValueEncoding::new(Some((1e40, 3e40)));
        assert_eq!(e.encode(2e40), 0.0);
        assert_eq!(e.encode(3e40), 1.0);
        assert!(e.encode(f64::NAN).is_nan());
        let e = ValueEncoding::new(Some((1e9, 1e9 + 1.0)));
        assert!(e.encode(1e9 + 0.25) < e.encode(1e9 + 0.26));
        assert_eq!(ValueEncoding::new(Some((2.0, 2.0))).auto_range(), [1.5, 2.5]);
        let v = Arc::new(vec![0.0, 1.0, f64::NAN]);
        let a = encoded_values(&v);
        let b = encoded_values(&v);
        assert_eq!(a.rev, b.rev);
        assert_eq!(a.data[1], 1.0);
    }
}
