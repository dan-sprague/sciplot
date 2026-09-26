//! Attribute plumbing: value conversion (`Conv`) and the `attributes!` macro, which generates, for
//! one block or plot type:
//! - `XAttrs`: every attribute as `Option<T>` (explicit value, or inherit),
//! - `XResolved`: every attribute concrete, via explicit > figure theme > default(globals),
//! - chainable setters on the handle (`fn title(&self, v) -> Self`),
//! - the same setters on a theme builder (`XTheme`), used by `Theme::axis(|a| a.title(..))`.

use crate::color::{Color, IntoColor};
use crate::data::Num;
use crate::text::RichText;

/// Converts a user-supplied value into an attribute's stored type.
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be used as a value for this attribute (expected something convertible to `{T}`)"
)]
pub trait Conv<T> {
    fn conv(self) -> T;
}

impl<N: Num> Conv<f64> for N {
    #[inline]
    fn conv(self) -> f64 {
        self.to_f64()
    }
}
impl<N: Num> Conv<f32> for N {
    #[inline]
    fn conv(self) -> f32 {
        self.to_f64() as f32
    }
}
impl Conv<bool> for bool {
    fn conv(self) -> bool {
        self
    }
}
impl<S: Into<RichText>> Conv<RichText> for S {
    fn conv(self) -> RichText {
        self.into()
    }
}
impl<C: IntoColor> Conv<Color> for C {
    #[track_caller]
    fn conv(self) -> Color {
        self.into_color()
    }
}
impl<A: Num, B: Num> Conv<[f64; 2]> for (A, B) {
    fn conv(self) -> [f64; 2] {
        [self.0.to_f64(), self.1.to_f64()]
    }
}
impl Conv<[f64; 2]> for [f64; 2] {
    fn conv(self) -> [f64; 2] {
        self
    }
}
/// Padding: one number for all sides, or `(left, right, bottom, top)` like Makie.
impl<N: Num> Conv<[f64; 4]> for N {
    fn conv(self) -> [f64; 4] {
        let v = self.to_f64();
        [v; 4]
    }
}
impl<A: Num, B: Num, C: Num, D: Num> Conv<[f64; 4]> for (A, B, C, D) {
    fn conv(self) -> [f64; 4] {
        [
            self.0.to_f64(),
            self.1.to_f64(),
            self.2.to_f64(),
            self.3.to_f64(),
        ]
    }
}
impl<N: Num> Conv<Option<f64>> for N {
    fn conv(self) -> Option<f64> {
        Some(self.to_f64())
    }
}
impl Conv<Option<f64>> for Option<f64> {
    fn conv(self) -> Option<f64> {
        self
    }
}

/// Identity conversions for enum/struct attribute types.
macro_rules! conv_identity {
    ($($t:ty),* $(,)?) => {$(
        impl $crate::attrs::Conv<$t> for $t { #[inline] fn conv(self) -> $t { self } }
    )*};
}
pub(crate) use conv_identity;

/// See module docs. Syntax:
///
/// ```ignore
/// attributes! {
///     Axis(AxisAttrs, AxisResolved, AxisTheme) via with_axis_attrs {
///         title: RichText = |_| RichText::default(), LAYOUT;
///         titlesize: f64 = |g| g.fontsize, LAYOUT;
///     }
/// }
/// ```
///
/// `via` names a method on the handle, `fn(&self, impl FnOnce(&mut XAttrs), Dirty)`, that applies
/// the change under the figure lock. The default is a non-capturing closure over
/// `&theme::Globals`.
macro_rules! attributes {
    (
        $Handle:ident ($Attrs:ident, $Resolved:ident, $ThemeB:ident) via $via:ident {
            $( $(#[$m:meta])* $field:ident : $ty:ty = $default:expr, $dirty:ident; )*
        }
    ) => {
        #[derive(Clone, Default, Debug)]
        #[allow(dead_code)]
        pub(crate) struct $Attrs { $( pub $field: Option<$ty>, )* }

        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        pub(crate) struct $Resolved { $( pub $field: $ty, )* }

        #[allow(dead_code)]
        impl $Attrs {
            pub(crate) fn resolve(&self, theme: &$Attrs, g: &$crate::theme::Globals) -> $Resolved {
                $Resolved { $(
                    $field: match (&self.$field, &theme.$field) {
                        (Some(v), _) | (None, Some(v)) => v.clone(),
                        (None, None) => { let f: fn(&$crate::theme::Globals) -> $ty = $default; f(g) }
                    },
                )* }
            }
            /// Overwrites fields that are set in `other`.
            pub(crate) fn merge_from(&mut self, other: &$Attrs) {
                $( if other.$field.is_some() { self.$field = other.$field.clone(); } )*
            }
        }

        impl $Handle {
            $(
                $(#[$m])*
                #[track_caller]
                pub fn $field(&self, v: impl $crate::attrs::Conv<$ty>) -> Self {
                    let v = v.conv();
                    self.$via(move |a: &mut $Attrs| a.$field = Some(v), $crate::figure::Dirty::$dirty);
                    self.clone()
                }
            )*
        }

        /// Theme builder: the same attribute names as the handle, set by value.
        #[derive(Clone, Default, Debug)]
        pub struct $ThemeB(pub(crate) $Attrs);

        impl $ThemeB {
            $(
                $(#[$m])*
                #[track_caller]
                pub fn $field(mut self, v: impl $crate::attrs::Conv<$ty>) -> Self {
                    self.0.$field = Some(v.conv());
                    self
                }
            )*
        }
    };
}
pub(crate) use attributes;
